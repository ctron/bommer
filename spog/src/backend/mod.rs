mod pkg;

pub use pkg::*;

use url::{ParseError, Url};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backend {
    pub url: Url,
}

impl Backend {
    pub fn join(&self, input: &str) -> Result<Url, Error> {
        Ok(self.url.join(input)?)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Failed to parse backend URL: {0}")]
    Url(#[from] ParseError),
    #[error("Failed to request: {0}")]
    Request(#[from] reqwest::Error),
}
