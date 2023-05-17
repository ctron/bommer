use std::collections::{HashMap, HashSet};
use std::ops::{Deref, DerefMut};

#[derive(
    Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, serde::Deserialize, serde::Serialize,
)]
pub struct ImageRef(pub String);

impl Deref for ImageRef {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A reference to a pod
#[derive(
    Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub struct PodRef {
    pub namespace: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageState {
    pub pods: HashSet<PodRef>,
}

#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct ImagesByNamespace(pub HashMap<String, Images>);

impl Deref for ImagesByNamespace {
    type Target = HashMap<String, Images>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ImagesByNamespace {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct Images(pub HashMap<ImageRef, ImageState>);

impl Deref for Images {
    type Target = HashMap<ImageRef, ImageState>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Images {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
