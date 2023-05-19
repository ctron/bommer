use crate::api::{ImageRef, PodRef};
use crate::bombastic::client::SBOM;
use crate::pubsub::{Event, State};
use crate::store::Store;
use anyhow::bail;
use packageurl::PackageUrl;
use std::collections::HashSet;
use std::future::Future;
use std::ops::Deref;
use std::time::Duration;
use tracing::{info, warn};

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

    (map.clone(), async move {
        tokio::select! {
            _ = runner(store, map.clone()) => {},
            _ = scanner(map, source) => {},
        }
        Ok(())
    })
}

struct Scanner {
    map: Map,
    source: BombasticSource,
}

impl Scanner {
    async fn lookup(&self, image: &ImageRef) -> Result<SBOM, anyhow::Error> {
        if let Some((base, digest)) = image.0.rsplit_once('@') {
            if let Some(name) = base.split('/').last() {
                let mut purl = PackageUrl::new("oci", name)?;
                if digest.starts_with("sha256:") {
                    purl.with_version(digest);
                    return Ok::<_, anyhow::Error>(self.source.lookup_sbom(purl).await?);
                }
            }
        }
        bail!("Unable to create PURL for: {image}");
    }

    async fn scan(&self, image: &ImageRef) {
        let state = match self.lookup(image).await {
            Ok(result) => SbomState::Found(result),
            Err(err) => SbomState::Err(err.to_string()),
        };
        self.map
            .mutate_state(image.clone(), |current| {
                current.map(|mut current| {
                    current.sbom = state;
                    current
                })
            })
            .await;
    }
}

async fn scanner(map: Map, source: BombasticSource) -> anyhow::Result<()> {
    let scanner = Scanner {
        map: map.clone(),
        source,
    };

    loop {
        info!("Starting subscription ... ");
        let mut sub = map.subscribe(128).await;
        while let Some(evt) = sub.recv().await {
            // FIXME: need to parallelize processing
            match evt {
                Event::Added(image, state) | Event::Modified(image, state) => {
                    if let SbomState::Scheduled = state.sbom {
                        scanner.scan(&image).await;
                    }
                }
                Event::Restart(state) => {
                    for (image, state) in state {
                        if let SbomState::Scheduled = state.sbom {
                            scanner.scan(&image).await;
                        }
                    }
                }
                Event::Removed(_) => {}
            }
        }

        // lost subscription, delay and re-try
        warn!("Lost subscription");
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn runner(store: Store<ImageRef, PodRef, ()>, map: Map) -> anyhow::Result<()> {
    loop {
        let mut sub = store.subscribe(32).await;
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
