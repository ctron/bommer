mod pubsub;

use crate::api::{ImageRef, ImageState, PodRef};
use futures::{stream, Stream, StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::{ContainerStatus, Pod};
use kube::runtime::watcher;
use kube::{Resource, ResourceExt};
use pubsub::Subscription;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::debug;

#[derive(Clone)]
pub struct Store {
    inner: Arc<RwLock<Inner>>,
}

#[derive(Clone, Debug)]
pub enum Event {
    Added(ImageRef),
    Removed(ImageRef),
    Restart(HashSet<ImageRef>),
}

#[derive(Default)]
pub struct Inner {
    /// Discovered images
    images: HashMap<ImageRef, ImageState>,

    /// pods, with their images
    ///
    /// This is mainly needed to figure out how to clean up a pod which got removed.
    pods: HashMap<PodRef, HashSet<ImageRef>>,

    /// listeners
    listeners: HashMap<uuid::Uuid, mpsc::Sender<Event>>,
}

impl Inner {
    /// add or modify an existing pod
    async fn apply(&mut self, object: Pod) {
        let pod_ref = match to_key(&object) {
            Some(pod_ref) => pod_ref,
            None => return,
        };

        let images = images_from_pod(object);

        if let Some(current) = self.pods.get(&pod_ref) {
            if current == &images {
                // equal, nothing to do
                return;
            }
            // delete pod, and continue adding
            self.delete(&pod_ref).await;
        }

        // now we can be sure we need to add it

        // add images
        for image in &images {
            match self.images.entry(image.clone()) {
                Entry::Vacant(entry) => {
                    entry.insert(ImageState {
                        pods: HashSet::from_iter([pod_ref.clone()]),
                    });
                    self.broadcast(Event::Added(image.clone())).await;
                }
                Entry::Occupied(mut entry) => {
                    entry.get_mut().pods.insert(pod_ref.clone());
                }
            }
        }

        // add pod
        self.pods.insert(pod_ref, images);
    }

    /// delete a pod
    async fn delete(&mut self, pod_ref: &PodRef) {
        if let Some(images) = self.pods.remove(pod_ref) {
            // we removed a pod, so let's clean up its images

            for image in images {
                if let Some(state) = self.images.get_mut(&image) {
                    state.pods.remove(pod_ref);
                    if state.pods.is_empty() {
                        self.images.remove(&image);
                        self.broadcast(Event::Removed(image)).await;
                    }
                }
            }
        }
    }

    /// full reset of the state
    async fn reset(
        &mut self,
        images: HashMap<ImageRef, ImageState>,
        pods: HashMap<PodRef, HashSet<ImageRef>>,
    ) {
        let keys = images.keys().cloned().collect::<HashSet<_>>();

        self.images = images;
        self.pods = pods;
        self.broadcast(Event::Restart(keys)).await;
    }

    fn get_state(&self) -> HashMap<ImageRef, ImageState> {
        self.images.clone()
    }

    fn subscribe(&mut self) -> (uuid::Uuid, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::channel(16);

        // we can "unwrap" here, as we just created the channel and are in control of the two
        // possible error conditions (full, no receiver).
        tx.try_send(Event::Restart(self.images.keys().cloned().collect()))
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

    async fn broadcast(&mut self, evt: Event) {
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

impl Store {
    pub fn new<S>(stream: S) -> (Self, impl Future<Output = anyhow::Result<()>>)
    where
        S: Stream<Item = Result<watcher::Event<Pod>, watcher::Error>>,
    {
        let inner = Arc::new(RwLock::new(Inner::default()));
        let runner = {
            let inner = inner.clone();
            async move { run(inner, stream).await }
        };

        (Self { inner }, runner)
    }

    pub async fn get_containers_all(&self) -> HashMap<ImageRef, ImageState> {
        self.inner.read().await.get_state()
    }

    pub async fn subscribe(&self) -> Subscription<Event> {
        let (id, rx) = self.inner.write().await.subscribe();
        Subscription::new(id, rx, self.inner.clone())
    }
}

async fn run<S>(inner: Arc<RwLock<Inner>>, stream: S) -> anyhow::Result<()>
where
    S: Stream<Item = Result<watcher::Event<Pod>, watcher::Error>>,
{
    let mut stream = pin!(stream);

    while let Some(evt) = stream.try_next().await? {
        match evt {
            watcher::Event::Applied(pod) => {
                inner.write().await.apply(pod).await;
            }
            watcher::Event::Deleted(pod) => {
                if let Some(pod_ref) = to_key(&pod) {
                    inner.write().await.delete(&pod_ref).await;
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
    HashMap<ImageRef, ImageState>,
    HashMap<PodRef, HashSet<ImageRef>>,
) {
    let mut by_images: HashMap<ImageRef, ImageState> = Default::default();
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
                .pods
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
