//! Configurable embedding client. The chat-relayer is meant to swap
//! the embedding model + vector dimension per deployment — the
//! reason chat-relayer carries its own memory stack instead of
//! delegating to MemWal's relayer.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct EmbeddingClient {
    api_base: String,
    api_key: Option<String>,
    model: String,
    dim: usize,
    http: reqwest::Client,
}

#[derive(Serialize)]
struct EmbedReq<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Deserialize)]
struct EmbedResp {
    data: Vec<EmbedItem>,
}

#[derive(Deserialize)]
struct EmbedItem {
    embedding: Vec<f32>,
}

impl EmbeddingClient {
    pub fn new(api_base: String, api_key: Option<String>, model: String, dim: usize) -> Self {
        Self {
            api_base,
            api_key,
            model,
            dim,
            http: reqwest::Client::new(),
        }
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/embeddings", self.api_base.trim_end_matches('/'));
        let mut req = self.http.post(&url).json(&EmbedReq {
            model: &self.model,
            input: text,
        });
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req.send().await.with_context(|| format!("POST {url}"))?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "embedding {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let body: EmbedResp = resp.json().await.context("decode embeddings")?;
        let v = body
            .data
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("embedding response had no data"))?
            .embedding;
        if v.len() != self.dim {
            return Err(anyhow!(
                "embedding returned {} dims, configured for {}",
                v.len(),
                self.dim
            ));
        }
        Ok(v)
    }
}
