use crate::api::{ImageRef, Images, ImagesByNamespace, PodRef};
use crate::bombastic::BombasticSource;
use futures::{Stream, TryStreamExt};
use k8s_openapi::api::core::v1::{ContainerStatus, Pod};
use kube::runtime::watcher;
use kube::{Resource, ResourceExt};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::pin;
use std::sync::Arc;

#[derive(Clone)]
pub struct Store {
    inner: Arc<RwLock<Inner>>,
}

#[derive(Default)]
struct Inner {
    /// images (by namespace), and which pods use them.
    ///
    /// This is the main structure of data we hold.
    images: ImagesByNamespace,

    /// pods, with their images
    ///
    /// This is mainly needed to figure out how to clean up a pod which got removed.
    pods: HashMap<PodRef, HashSet<ImageRef>>,
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
            self.images
                .entry(pod_ref.namespace.clone())
                .or_default()
                .entry(image.clone())
                .or_default()
                .pods
                .insert(pod_ref.clone());
        }

        // add pod
        self.pods.insert(pod_ref, images);
    }

    /// delete a pod
    async fn delete(&mut self, pod_ref: &PodRef) {
        if let Some(images) = self.pods.remove(pod_ref) {
            // we removed a pod, so let's clean up its images

            for image in images {
                if let Some(ns_images) = self.images.get_mut(&pod_ref.namespace) {
                    if let Some(state) = ns_images.get_mut(&image) {
                        state.pods.remove(pod_ref);
                        if state.pods.is_empty() {
                            ns_images.remove(&image);
                            if ns_images.is_empty() {
                                self.images.remove(&pod_ref.namespace);
                            }
                        }
                    }
                }
            }
        }
    }

    /// full reset of the state
    async fn reset(&mut self, images: ImagesByNamespace, pods: HashMap<PodRef, HashSet<ImageRef>>) {
        self.images = images;
        self.pods = pods;
    }

    /// get the containers for a namespace
    async fn get_containers_ns(&self, namespace: &str) -> Images {
        self.images.get(namespace).cloned().unwrap_or_default()
    }

    async fn get_containers_all(&self) -> ImagesByNamespace {
        self.images.clone()
    }
}

impl Store {
    pub fn new<S>(
        stream: S,
        source: BombasticSource,
    ) -> (Self, impl Future<Output = anyhow::Result<()>>)
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

    pub async fn get_containers_all(&self) -> ImagesByNamespace {
        self.inner.read().get_containers_all().await
    }

    pub async fn get_containers_ns(&self, namespace: &str) -> Images {
        self.inner.read().get_containers_ns(namespace).await
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
                inner.write().apply(pod).await;
            }
            watcher::Event::Deleted(pod) => {
                if let Some(pod_ref) = to_key(&pod) {
                    inner.write().delete(&pod_ref).await;
                }
            }
            watcher::Event::Restarted(pods) => {
                let (images, pods) = to_state(pods);
                inner.write().reset(images, pods).await;
            }
        }
    }

    Ok(())
}

fn to_state(pods: Vec<Pod>) -> (ImagesByNamespace, HashMap<PodRef, HashSet<ImageRef>>) {
    let mut by_images: ImagesByNamespace = Default::default();
    let mut by_pods = HashMap::new();

    for pod in pods {
        let pod_ref = match to_key(&pod) {
            Some(pod_ref) => pod_ref,
            None => continue,
        };

        let images = images_from_pod(pod);
        for image in &images {
            by_images
                .entry(pod_ref.namespace.clone())
                .or_default()
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
                .flat_map(|c| c.into_iter().map(to_container_id))
                .chain(
                    s.init_container_statuses
                        .into_iter()
                        .flat_map(|ic| ic.into_iter().map(to_container_id)),
                )
                .chain(
                    s.ephemeral_container_statuses
                        .into_iter()
                        .flat_map(|ic| ic.into_iter().map(to_container_id)),
                )
        })
        .collect()
}

pub fn to_container_id(container: ContainerStatus) -> ImageRef {
    // FIXME: we need some more magic here, as kubernetes has weird ideas on filling the fields image and imageId.
    // see: docs/image_id.md

    // FIXME: this won't work on kind, and maybe others, as they generate broken image ID values
    ImageRef(container.image_id)

    // ImageRef(format!("{} / {}", container.image, container.image_id))
}
