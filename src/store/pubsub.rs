use super::Inner;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

pub struct Subscription<T> {
    id: Option<uuid::Uuid>,
    rx: mpsc::Receiver<T>,
    store: Arc<RwLock<Inner>>,
}

impl<T> Subscription<T> {
    pub fn new(id: uuid::Uuid, rx: mpsc::Receiver<T>, store: Arc<RwLock<Inner>>) -> Self {
        Self {
            id: Some(id),
            rx,
            store,
        }
    }
}

impl<T> Drop for Subscription<T> {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            let store = self.store.clone();
            tokio::spawn(async move {
                store.write().await.listeners.remove(&id);
            });
        }
    }
}

impl<T> Deref for Subscription<T> {
    type Target = mpsc::Receiver<T>;

    fn deref(&self) -> &Self::Target {
        &self.rx
    }
}

impl<T> DerefMut for Subscription<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.rx
    }
}
