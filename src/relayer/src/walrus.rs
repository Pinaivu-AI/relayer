//! Direct Walrus HTTP client. Walrus has no Node-only SDK requirement
//! (unlike Seal) — the node already talks to it over plain HTTP in
//! `node/src/walrus.rs`; this mirrors that.

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

#[derive(Clone)]
pub struct WalrusClient {
    publisher: String,
    aggregator: String,
    http: reqwest::Client,
    epochs: u32,
}

#[derive(Deserialize)]
struct PublishResp {
    #[serde(rename = "newlyCreated")]
    newly_created: Option<NewlyCreated>,
    #[serde(rename = "alreadyCertified")]
    already_certified: Option<AlreadyCertified>,
}

#[derive(Deserialize)]
struct NewlyCreated {
    #[serde(rename = "blobObject")]
    blob_object: BlobObject,
}

#[derive(Deserialize)]
struct BlobObject {
    #[serde(rename = "blobId")]
    blob_id: String,
}

#[derive(Deserialize)]
struct AlreadyCertified {
    #[serde(rename = "blobId")]
    blob_id: String,
}

impl WalrusClient {
    pub fn new(publisher: String, aggregator: String, epochs: u32) -> Self {
        Self {
            publisher,
            aggregator,
            http: reqwest::Client::new(),
            epochs,
        }
    }

    pub async fn upload(&self, bytes: &[u8]) -> Result<String> {
        let url = format!(
            "{}/v1/blobs?epochs={}",
            self.publisher.trim_end_matches('/'),
            self.epochs
        );
        let resp = self
            .http
            .put(&url)
            .body(bytes.to_vec())
            .send()
            .await
            .context("walrus publisher PUT")?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "walrus publisher {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let body: PublishResp = resp.json().await.context("decode walrus publish resp")?;
        let blob_id = body
            .newly_created
            .map(|n| n.blob_object.blob_id)
            .or_else(|| body.already_certified.map(|a| a.blob_id))
            .ok_or_else(|| anyhow!("walrus publisher returned no blob_id"))?;
        Ok(blob_id)
    }

    pub async fn download(&self, blob_id: &str) -> Result<Option<Vec<u8>>> {
        let url = format!(
            "{}/v1/blobs/{}",
            self.aggregator.trim_end_matches('/'),
            blob_id
        );
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .context("walrus aggregator GET")?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            return Err(anyhow!(
                "walrus aggregator {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        Ok(Some(resp.bytes().await.context("read walrus blob bytes")?.to_vec()))
    }
}
