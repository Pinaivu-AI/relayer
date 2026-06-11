//! HTTP client for the TS sidecar's Walrus endpoints.

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct WalrusClient {
    base: String,
    secret: String,
    http: reqwest::Client,
}

#[derive(Serialize)]
struct UploadReq {
    bytes_b64: String,
    epochs: u32,
}

#[derive(Deserialize)]
struct UploadResp {
    blob_id: String,
}

#[derive(Deserialize)]
struct DownloadResp {
    bytes_b64: String,
}

impl WalrusClient {
    pub fn new(base: String, secret: String) -> Self {
        Self {
            base,
            secret,
            http: reqwest::Client::new(),
        }
    }

    pub async fn upload(&self, bytes: &[u8], epochs: u32) -> Result<String> {
        let url = format!("{}/walrus/upload", self.base.trim_end_matches('/'));
        let resp = self
            .http
            .post(&url)
            .header("X-Sidecar-Secret", &self.secret)
            .json(&UploadReq {
                bytes_b64: B64.encode(bytes),
                epochs,
            })
            .send()
            .await
            .context("sidecar walrus upload send")?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "sidecar /walrus/upload {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let body: UploadResp = resp.json().await.context("walrus upload decode")?;
        Ok(body.blob_id)
    }

    pub async fn download(&self, blob_id: &str) -> Result<Option<Vec<u8>>> {
        let url = format!(
            "{}/walrus/download/{}",
            self.base.trim_end_matches('/'),
            blob_id
        );
        let resp = self
            .http
            .get(&url)
            .header("X-Sidecar-Secret", &self.secret)
            .send()
            .await
            .context("sidecar walrus download send")?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            return Err(anyhow!(
                "sidecar /walrus/download {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let body: DownloadResp = resp.json().await.context("walrus download decode")?;
        Ok(Some(B64.decode(body.bytes_b64)?))
    }
}
