use crate::api::{ImageRef, PodRef};
use crate::bombastic::client::SBOM;
use crate::pubsub::Event;
use crate::store::Store;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::RwLock;

mod client;
pub use client::BombasticSource;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub pods: HashSet<PodRef>,
    pub sbom: SbomState,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SbomState {
    Scheduled,
    Err(String),
    Missing,
    Found(SBOM),
}

#[derive(Clone, Debug, Default)]
pub struct Map {
    inner: Arc<RwLock<Inner>>,
}

#[derive(Debug, Default)]
struct Inner {
    data: HashMap<ImageRef, Image>,
}

impl Inner {
    fn apply(&mut self, image: ImageRef, pods: HashSet<PodRef>) {
        match self.data.entry(image) {
            Entry::Vacant(entry) => {
                entry.insert(Image {
                    pods,
                    sbom: SbomState::Scheduled,
                });
            }
            Entry::Occupied(mut entry) => {
                entry.get_mut().pods = pods;
            }
        }
    }

    fn remove(&mut self, image: ImageRef) {
        self.data.remove(&image);
    }

    fn reset(&mut self, state: HashMap<ImageRef, HashSet<PodRef>>) {
        self.data = state
            .into_iter()
            .map(|(k, pods)| {
                (
                    k,
                    Image {
                        pods,
                        sbom: SbomState::Scheduled,
                    },
                )
            })
            .collect()
    }
}

pub fn store(
    store: Store<ImageRef, PodRef, ()>,
    source: BombasticSource,
) -> (Map, impl Future<Output = anyhow::Result<()>>) {
    let map = Map::default();

    let runner = {
        let map = map.clone();
        async move {
            loop {
                let mut sub = store.subscribe().await;
                while let Some(evt) = sub.recv().await {
                    match evt {
                        Event::Added(image, state) => {
                            map.inner.write().await.apply(image, state.owners);
                        }
                        Event::Modified(image, state) => {
                            map.inner.write().await.apply(image, state.owners);
                        }
                        Event::Removed(image) => {
                            map.inner.write().await.remove(image);
                        }
                        Event::Restart(state) => {
                            map.inner
                                .write()
                                .await
                                .reset(state.into_iter().map(|(k, v)| (k, v.owners)).collect());
                        }
                    }
                }
            }
        }
    };

    (map, runner)
}
