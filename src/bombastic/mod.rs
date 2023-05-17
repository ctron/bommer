use crate::api::{ImageRef, PodRef};
use crate::bombastic::client::SBOM;
use crate::pubsub::{Event, State};
use crate::store::Store;
use std::collections::HashSet;
use std::future::Future;
use std::ops::Deref;

mod client;
pub use client::BombasticSource;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub pods: HashSet<PodRef>,
    pub sbom: SbomState,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SbomState {
    Scheduled,
    Err(String),
    Missing,
    Found(SBOM),
}

#[derive(Clone, Debug, Default)]
pub struct Map {
    state: State<ImageRef, Image>,
}

impl Deref for Map {
    type Target = State<ImageRef, Image>;

    fn deref(&self) -> &Self::Target {
        &self.state
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
                        Event::Added(image, state) | Event::Modified(image, state) => {
                            map.state
                                .mutate_state(image, |current| match current {
                                    Some(mut current) => {
                                        current.pods = state.owners;
                                        Some(current)
                                    }
                                    None => Some(Image {
                                        pods: state.owners,
                                        sbom: SbomState::Scheduled,
                                    }),
                                })
                                .await;
                        }
                        Event::Removed(image) => {
                            map.state.mutate_state(image, |_| None).await;
                        }
                        Event::Restart(state) => {
                            map.state
                                .set_state(
                                    state
                                        .into_iter()
                                        .map(|(k, v)| {
                                            (
                                                k,
                                                Image {
                                                    pods: v.owners,
                                                    sbom: SbomState::Scheduled,
                                                },
                                            )
                                        })
                                        .collect(),
                                )
                                .await;
                        }
                    }
                }
            }
        }
    };

    (map, runner)
}
