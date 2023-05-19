use super::{Backend, Error};
use bommer_api::data::{Image, ImageRef};
use std::collections::HashMap;

pub struct WorkloadService {
    backend: Backend,
    client: reqwest::Client,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct Workload(pub HashMap<ImageRef, Image>);

#[allow(unused)]
impl WorkloadService {
    pub fn new(backend: Backend) -> Self {
        Self {
            backend,
            client: reqwest::Client::new(),
        }
    }

    pub async fn lookup(&self) -> Result<Workload, Error> {
        Ok(self
            .client
            .get(self.backend.join("/api/v1/workload")?)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
}
