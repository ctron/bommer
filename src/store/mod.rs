mod pubsub;

use crate::api::{ImageRef, PodRef};
use crate::store::pubsub::Event;
use futures::{stream, Stream, StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::{ContainerStatus, Pod};
use kube::runtime::watcher;
use kube::{Resource, ResourceExt};
use pubsub::Subscription;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::future::Future;
use std::hash::Hash;
use std::pin::pin;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::debug;

#[derive(Clone)]
pub struct Store<K, O, V>
where
    K: Clone + Debug + Eq + Hash,
    O: Clone + Debug + Eq + Hash,
    V: Clone + Debug,
{
    inner: Arc<RwLock<Inner<K, O, V>>>,
}

#[derive(Clone, Debug)]
pub struct State<O, V> {
    pub owners: HashSet<O>,
    pub state: V,
}

impl<O, V> Default for State<O, V>
where
    V: Default,
{
    fn default() -> Self {
        Self {
            owners: Default::default(),
            state: Default::default(),
        }
    }
}

pub struct Inner<K, O, V>
where
    K: Clone + Debug + Eq + Hash,
    O: Clone + Debug + Eq + Hash,
    V: Clone + Debug,
{
    /// Discovered images
    images: HashMap<K, State<O, V>>,

    /// pods, with their images
    ///
    /// This is mainly needed to figure out how to clean up a pod which got removed.
    pods: HashMap<O, HashSet<K>>,

    /// listeners
    listeners: HashMap<uuid::Uuid, mpsc::Sender<Event<K, O, V>>>,
}

impl<K, O, V> Default for Inner<K, O, V>
where
    K: Clone + Debug + Eq + Hash,
    O: Clone + Debug + Eq + Hash,
    V: Clone + Debug,
{
    fn default() -> Self {
        Self {
            images: Default::default(),
            pods: Default::default(),
            listeners: Default::default(),
        }
    }
}

impl<K, O, V> Inner<K, O, V>
where
    K: Clone + Debug + Eq + Hash,
    O: Clone + Debug + Eq + Hash,
    V: Clone + Debug,
{
    /// add or modify an existing pod
    async fn apply<I, A>(&mut self, owner_ref: O, keys: HashSet<K>, initial: I, apply: A)
    where
        I: Fn(&K) -> V,
        A: Fn(&K, V) -> V,
    {
        if let Some(current) = self.pods.get(&owner_ref) {
            if current == &keys {
                // equal, nothing to do
                return;
            }

            // delete pod, and continue adding
            self.delete(&owner_ref, &apply).await;
        }

        // now we can be sure we need to add it

        // add images
        for image in &keys {
            match self.images.entry(image.clone()) {
                Entry::Vacant(entry) => {
                    let state = State {
                        owners: HashSet::from_iter([owner_ref.clone()]),
                        state: initial(image),
                    };
                    entry.insert(state.clone());
                    self.broadcast(Event::Added(image.clone(), state)).await;
                }
                Entry::Occupied(mut entry) => {
                    let state = entry.get_mut();
                    let added = state.owners.insert(owner_ref.clone());

                    state.state = apply(image, state.state.clone());

                    if added {
                        let state = state.clone();
                        self.broadcast(Event::Modified(image.clone(), state)).await;
                    }
                }
            }
        }

        // add pod
        self.pods.insert(owner_ref, keys);
    }

    /// delete a pod
    async fn delete<A>(&mut self, pod_ref: &O, apply: A)
    where
        A: Fn(&K, V) -> V,
    {
        if let Some(images) = self.pods.remove(pod_ref) {
            // we removed a pod, so let's clean up its images

            for image in images {
                if let Some(state) = self.images.get_mut(&image) {
                    state.owners.remove(pod_ref);
                    if state.owners.is_empty() {
                        self.images.remove(&image);
                        self.broadcast(Event::Removed(image)).await;
                    } else {
                        state.state = apply(&image, state.state.clone());
                        let state = state.clone();
                        self.broadcast(Event::Modified(image, state)).await;
                    }
                }
            }
        }
    }

    /// full reset of the state
    async fn reset(&mut self, images: HashMap<K, State<O, V>>, pods: HashMap<O, HashSet<K>>) {
        self.images = images.clone();
        self.pods = pods;
        self.broadcast(Event::Restart(images)).await;
    }

    fn get_state(&self) -> HashMap<K, State<O, V>> {
        self.images.clone()
    }

    fn subscribe(&mut self) -> (uuid::Uuid, mpsc::Receiver<Event<K, O, V>>) {
        let (tx, rx) = mpsc::channel(16);

        // we can "unwrap" here, as we just created the channel and are in control of the two
        // possible error conditions (full, no receiver).
        tx.try_send(Event::Restart(self.images.clone()))
            .expect("Channel must have enough capacity");

        let id = loop {
            let id = uuid::Uuid::new_v4();
            if let Entry::Vacant(entry) = self.listeners.entry(id) {
                entry.insert(tx);
                break id;
            }
        };

        (id, rx)
    }

    async fn broadcast(&mut self, evt: Event<K, O, V>) {
        let listeners = stream::iter(&self.listeners);
        let listeners = listeners.map(|(id, l)| {
            let evt = evt.clone();
            async move {
                if let Err(_) = l.send(evt).await {
                    Some(*id)
                } else {
                    None
                }
            }
        });
        let failed: Vec<uuid::Uuid> = listeners
            .buffer_unordered(10)
            .filter_map(|s| async move { s })
            .collect()
            .await;

        // remove failed subscribers

        for id in failed {
            debug!(?id, "Removing failed listener");
            self.listeners.remove(&id);
        }
    }
}

pub fn image_store<S>(
    stream: S,
) -> (
    Store<ImageRef, PodRef, ()>,
    impl Future<Output = anyhow::Result<()>>,
)
where
    S: Stream<Item = Result<watcher::Event<Pod>, watcher::Error>>,
{
    let inner = Arc::new(RwLock::new(Inner::default()));
    let runner = {
        let inner = inner.clone();
        async move { run(inner, stream).await }
    };

    (Store { inner }, runner)
}

impl<K, O, V> Store<K, O, V>
where
    K: Clone + Debug + Eq + Hash + Send + Sync + 'static,
    O: Clone + Debug + Eq + Hash + Send + Sync + 'static,
    V: Clone + Debug + Send + Sync + 'static,
{
    pub async fn get_state(&self) -> HashMap<K, State<O, V>> {
        self.inner.read().await.get_state()
    }

    pub async fn subscribe(&self) -> Subscription<K, O, V> {
        let (id, rx) = self.inner.write().await.subscribe();
        Subscription::new(id, rx, self.inner.clone())
    }
}

async fn run<S>(inner: Arc<RwLock<Inner<ImageRef, PodRef, ()>>>, stream: S) -> anyhow::Result<()>
where
    S: Stream<Item = Result<watcher::Event<Pod>, watcher::Error>>,
{
    let mut stream = pin!(stream);

    while let Some(evt) = stream.try_next().await? {
        match evt {
            watcher::Event::Applied(pod) => {
                let pod_ref = match to_key(&pod) {
                    Some(pod_ref) => pod_ref,
                    None => continue,
                };

                let images = images_from_pod(pod);

                inner
                    .write()
                    .await
                    .apply(pod_ref, images, |_| (), |_, v| v)
                    .await;
            }
            watcher::Event::Deleted(pod) => {
                if let Some(pod_ref) = to_key(&pod) {
                    inner.write().await.delete(&pod_ref, |_, v| v).await;
                }
            }
            watcher::Event::Restarted(pods) => {
                let (images, pods) = to_state(pods);
                inner.write().await.reset(images, pods).await;
            }
        }
    }

    Ok(())
}

fn to_state(
    pods: Vec<Pod>,
) -> (
    HashMap<ImageRef, State<PodRef, ()>>,
    HashMap<PodRef, HashSet<ImageRef>>,
) {
    let mut by_images: HashMap<ImageRef, State<PodRef, ()>> = Default::default();
    let mut by_pods = HashMap::new();

    for pod in pods {
        let pod_ref = match to_key(&pod) {
            Some(pod_ref) => pod_ref,
            None => continue,
        };

        let images = images_from_pod(pod);
        for image in &images {
            by_images
                .entry(image.clone())
                .or_default()
                .owners
                .insert(pod_ref.clone());
        }

        by_pods.insert(pod_ref, images);
    }

    (by_images, by_pods)
}

/// create a key for a pod
fn to_key(pod: &Pod) -> Option<PodRef> {
    match (pod.namespace(), pod.meta().name.clone()) {
        (Some(namespace), Some(name)) => Some(PodRef { namespace, name }),
        _ => None,
    }
}

/// collect all container images from a pod
fn images_from_pod(pod: Pod) -> HashSet<ImageRef> {
    pod.status
        .into_iter()
        .flat_map(|s| {
            s.container_statuses
                .into_iter()
                .flat_map(|c| c.into_iter().flat_map(to_container_id))
                .chain(
                    s.init_container_statuses
                        .into_iter()
                        .flat_map(|ic| ic.into_iter().flat_map(to_container_id)),
                )
                .chain(
                    s.ephemeral_container_statuses
                        .into_iter()
                        .flat_map(|ic| ic.into_iter().flat_map(to_container_id)),
                )
        })
        .collect()
}

pub fn to_container_id(container: ContainerStatus) -> Option<ImageRef> {
    if container.image_id.is_empty() {
        return None;
    }

    // FIXME: we need some more magic here, as kubernetes has weird ideas on filling the fields image and imageId.
    // see: docs/image_id.md

    // FIXME: this won't work on kind, and maybe others, as they generate broken image ID values
    Some(ImageRef(container.image_id))

    // ImageRef(format!("{} / {}", container.image, container.image_id))
}
