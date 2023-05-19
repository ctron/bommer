use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use std::ops::Deref;

#[derive(
    Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, serde::Deserialize, serde::Serialize,
)]
pub struct ImageRef(pub String);

impl Display for ImageRef {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

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
