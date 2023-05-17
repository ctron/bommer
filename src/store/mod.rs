mod pods;

use crate::pubsub::{Event, Subscription};
use futures::{stream, StreamExt};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::debug;

pub use pods::image_store;

#[derive(Clone)]
pub struct Store<K, O, V>
where
    K: Clone + Debug + Eq + Hash,
    O: Clone + Debug + Eq + Hash,
    V: Clone + Debug,
{
    inner: Arc<RwLock<Inner<K, O, V>>>,
}

impl<K, O, V> Default for Store<K, O, V>
where
    K: Clone + Debug + Eq + Hash,
    O: Clone + Debug + Eq + Hash,
    V: Clone + Debug,
{
    fn default() -> Self {
        Self {
            inner: Default::default(),
        }
    }
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
    listeners: Listeners<K, State<O, V>>,
}

pub struct Listeners<K, V>
where
    K: Clone + Debug + Eq + Hash,
    V: Clone + Debug,
{
    /// listeners
    listeners: HashMap<uuid::Uuid, mpsc::Sender<Event<K, V>>>,
}

impl<K, V> Listeners<K, V>
where
    K: Clone + Debug + Eq + Hash,
    V: Clone + Debug,
{
    async fn broadcast(&mut self, evt: Event<K, V>) {
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

    fn subscribe(&mut self) -> (uuid::Uuid, mpsc::Receiver<Event<K, V>>) {
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
}

impl<K, V> Default for Listeners<K, V>
where
    K: Clone + Debug + Eq + Hash,
    V: Clone + Debug,
{
    fn default() -> Self {
        Self {
            listeners: Default::default(),
        }
    }
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
                    self.listeners
                        .broadcast(Event::Added(image.clone(), state))
                        .await;
                }
                Entry::Occupied(mut entry) => {
                    let state = entry.get_mut();
                    let added = state.owners.insert(owner_ref.clone());

                    state.state = apply(image, state.state.clone());

                    if added {
                        let state = state.clone();
                        self.listeners
                            .broadcast(Event::Modified(image.clone(), state))
                            .await;
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
                        self.listeners.broadcast(Event::Removed(image)).await;
                    } else {
                        state.state = apply(&image, state.state.clone());
                        let state = state.clone();
                        self.listeners
                            .broadcast(Event::Modified(image, state))
                            .await;
                    }
                }
            }
        }
    }

    /// full reset of the state
    async fn reset(&mut self, images: HashMap<K, State<O, V>>, pods: HashMap<O, HashSet<K>>) {
        self.images = images.clone();
        self.pods = pods;
        self.listeners.broadcast(Event::Restart(images)).await;
    }

    fn get_state(&self) -> HashMap<K, State<O, V>> {
        self.images.clone()
    }

    fn subscribe(&mut self) -> (uuid::Uuid, mpsc::Receiver<Event<K, State<O, V>>>) {
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

    pub async fn subscribe(&self) -> Subscription<K, State<O, V>> {
        let (id, rx) = self.inner.write().await.subscribe();
        let store = self.inner.clone();
        Subscription::new(rx, move || {
            tokio::spawn(async move {
                store.write().await.listeners.remove(&id);
            });
        })
    }
}
