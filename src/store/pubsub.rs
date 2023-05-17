use super::{Inner, State};
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

#[derive(Clone, Debug)]
pub enum Event<K, O, V>
where
    K: Clone + Debug + Eq + Hash,
    O: Clone + Debug + Eq + Hash,
    V: Clone + Debug,
{
    Added(K, State<O, V>),
    Modified(K, State<O, V>),
    Removed(K),
    Restart(HashMap<K, State<O, V>>),
}

pub struct Subscription<K, O, V>
where
    K: Clone + Debug + Eq + Hash + Send + Sync + 'static,
    O: Clone + Debug + Eq + Hash + Send + Sync + 'static,
    V: Clone + Debug + Send + Sync + 'static,
{
    id: Option<uuid::Uuid>,
    rx: mpsc::Receiver<Event<K, O, V>>,
    store: Arc<RwLock<Inner<K, O, V>>>,
}

impl<K, O, V> Subscription<K, O, V>
where
    K: Clone + Debug + Eq + Hash + Send + Sync,
    O: Clone + Debug + Eq + Hash + Send + Sync,
    V: Clone + Debug + Send + Sync,
{
    pub fn new(
        id: uuid::Uuid,
        rx: mpsc::Receiver<Event<K, O, V>>,
        store: Arc<RwLock<Inner<K, O, V>>>,
    ) -> Self {
        Self {
            id: Some(id),
            rx,
            store,
        }
    }
}

impl<K, O, V> Drop for Subscription<K, O, V>
where
    K: Clone + Debug + Eq + Hash + Send + Sync + 'static,
    O: Clone + Debug + Eq + Hash + Send + Sync + 'static,
    V: Clone + Debug + Send + Sync + 'static,
{
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            let store = self.store.clone();
            tokio::spawn(async move {
                store.write().await.listeners.remove(&id);
            });
        }
    }
}

impl<K, O, V> Deref for Subscription<K, O, V>
where
    K: Clone + Debug + Eq + Hash + Send + Sync,
    O: Clone + Debug + Eq + Hash + Send + Sync,
    V: Clone + Debug + Send + Sync,
{
    type Target = mpsc::Receiver<Event<K, O, V>>;

    fn deref(&self) -> &Self::Target {
        &self.rx
    }
}

impl<K, O, V> DerefMut for Subscription<K, O, V>
where
    K: Clone + Debug + Eq + Hash + Send + Sync,
    O: Clone + Debug + Eq + Hash + Send + Sync,
    V: Clone + Debug + Send + Sync,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.rx
    }
}
