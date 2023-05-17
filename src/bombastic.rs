use packageurl::PackageUrl;
use reqwest::Url;
use url::ParseError;

#[derive(Clone, Debug)]
pub struct BombasticSource {
    url: Url,
    client: reqwest::Client,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Failed to build URL: {0}")]
    Url(#[from] ParseError),
    #[error("Request error: {0}")]
    Request(#[from] reqwest::Error),
}

#[derive(Debug)]
pub struct SBOM {
    pub data: String,
}

impl BombasticSource {
    pub fn new(url: Url) -> Self {
        Self {
            url,
            client: reqwest::Client::new(),
        }
    }

    pub async fn lookup_sbom(&self, purl: PackageUrl<'_>) -> Result<SBOM, Error> {
        let response = self
            .client
            .get(self.url.join("/api/v1/sbom")?)
            .query(&[("purl", purl.to_string())])
            .send()
            .await?;

        Ok(SBOM {
            data: response.text().await?,
        })
    }
}
