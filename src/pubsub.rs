use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::ops::{Deref, DerefMut};
use tokio::sync::mpsc;

#[derive(Clone, Debug)]
pub enum Event<K, V>
where
    K: Clone + Debug + Eq + Hash,
    V: Clone + Debug,
{
    Added(K, V),
    Modified(K, V),
    Removed(K),
    Restart(HashMap<K, V>),
}

pub struct Subscription<K, V>
where
    K: Clone + Debug + Eq + Hash + Send + Sync + 'static,
    V: Clone + Debug + Send + Sync + 'static,
{
    rx: mpsc::Receiver<Event<K, V>>,
    unsubscribe: Option<Box<dyn FnOnce() + Send + Sync + 'static>>,
}

impl<K, V> Subscription<K, V>
where
    K: Clone + Debug + Eq + Hash + Send + Sync,
    V: Clone + Debug + Send + Sync,
{
    pub fn new(
        rx: mpsc::Receiver<Event<K, V>>,
        unsubscribe: impl FnOnce() + Send + Sync + 'static,
    ) -> Self {
        Self {
            rx,
            unsubscribe: Some(Box::new(unsubscribe)),
        }
    }
}

impl<K, V> Drop for Subscription<K, V>
where
    K: Clone + Debug + Eq + Hash + Send + Sync + 'static,
    V: Clone + Debug + Send + Sync + 'static,
{
    fn drop(&mut self) {
        if let Some(unsubscribe) = self.unsubscribe.take() {
            unsubscribe();
        }
    }
}

impl<K, V> Deref for Subscription<K, V>
where
    K: Clone + Debug + Eq + Hash + Send + Sync,
    V: Clone + Debug + Send + Sync,
{
    type Target = mpsc::Receiver<Event<K, V>>;

    fn deref(&self) -> &Self::Target {
        &self.rx
    }
}

impl<K, V> DerefMut for Subscription<K, V>
where
    K: Clone + Debug + Eq + Hash + Send + Sync,
    V: Clone + Debug + Send + Sync,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.rx
    }
}
