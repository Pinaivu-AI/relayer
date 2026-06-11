//! HTTP client for the TS sidecar's Seal endpoints.
//!
//! The Rust binary never touches `@mysten/seal` directly — every
//! encrypt/decrypt round-trips through `localhost:9000` to the
//! co-located Node process. Same pattern as nautilus-memwal-relayer.

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct SealClient {
    base: String,
    secret: String,
    http: reqwest::Client,
}

#[derive(Serialize)]
struct EncryptReq<'a> {
    plaintext_b64: String,
    owner_address: &'a str,
}

#[derive(Deserialize)]
struct EncryptResp {
    ciphertext_b64: String,
}

#[derive(Serialize)]
struct DecryptReq<'a> {
    ciphertext_b64: String,
    owner_address: &'a str,
}

#[derive(Deserialize)]
struct DecryptResp {
    plaintext_b64: String,
}

impl SealClient {
    pub fn new(base: String, secret: String) -> Self {
        Self {
            base,
            secret,
            http: reqwest::Client::new(),
        }
    }

    pub async fn encrypt(&self, owner_address: &str, plaintext: &[u8]) -> Result<Vec<u8>> {
        let url = format!("{}/seal/encrypt", self.base.trim_end_matches('/'));
        let resp = self
            .http
            .post(&url)
            .header("X-Sidecar-Secret", &self.secret)
            .json(&EncryptReq {
                plaintext_b64: B64.encode(plaintext),
                owner_address,
            })
            .send()
            .await
            .context("sidecar encrypt send")?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "sidecar /seal/encrypt {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let body: EncryptResp = resp.json().await.context("sidecar encrypt decode")?;
        B64.decode(body.ciphertext_b64).map_err(Into::into)
    }

    pub async fn decrypt(&self, owner_address: &str, ciphertext: &[u8]) -> Result<Vec<u8>> {
        let url = format!("{}/seal/decrypt", self.base.trim_end_matches('/'));
        let resp = self
            .http
            .post(&url)
            .header("X-Sidecar-Secret", &self.secret)
            .json(&DecryptReq {
                ciphertext_b64: B64.encode(ciphertext),
                owner_address,
            })
            .send()
            .await
            .context("sidecar decrypt send")?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "sidecar /seal/decrypt {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let body: DecryptResp = resp.json().await.context("sidecar decrypt decode")?;
        B64.decode(body.plaintext_b64).map_err(Into::into)
    }
}
