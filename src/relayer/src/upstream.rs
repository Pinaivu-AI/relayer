//! Outbound calls to pinaivu-api (the gateway) → coordinator → node.
//!
//! The coordinator's `/v1/chat/completions` now does the full round
//! trip itself — auction, then dispatch the job to the winning node
//! over its existing outbound libp2p connection, then wait for the
//! reply — and returns the final `content` directly. chat-relayer
//! calls this exact same endpoint a Path B developer would, just with
//! a couple of extra fields (`session_key`, `memwal_context`). No
//! separate relay endpoint needed: the coordinator never expects the
//! caller to dial the node's HTTP server itself anymore.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone)]
pub struct UpstreamClient {
    pinaivu_api_base: String,
    pinaivu_api_key: String,
    http: reqwest::Client,
}

#[derive(Serialize)]
pub struct ChatMessageOut<'a> {
    pub role: &'a str,
    pub content: &'a str,
}

/// Body for POST {pinaivu_api_base}/v1/chat/completions.
#[derive(Serialize)]
pub struct ChatCompletionBody<'a> {
    pub model: &'a str,
    pub messages: &'a [ChatMessageOut<'a>],
    pub client_pubkey_hex: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Uuid>,
    /// AES-256 key (base64) so the node can decrypt the Walrus session
    /// blob. The relayer mints + caches per-user/session.
    pub session_key: &'a str,
    /// Cross-session memory facts recalled from chat-relayer's own
    /// pgvector + Walrus stack. Prepended into the system prompt by the
    /// node's context::assemble.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memwal_context: Option<&'a str>,
}

#[derive(Deserialize)]
pub struct NodeReply {
    pub request_id: Uuid,
    pub session_id: Uuid,
    pub content: String,
    #[allow(dead_code)]
    pub session_key: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub latency_ms: u32,
}

impl UpstreamClient {
    pub fn new(pinaivu_api_base: String, pinaivu_api_key: String) -> Self {
        // Accept invalid TLS in dev (matches the INSECURE_COORDINATOR
        // gate used by the node binary).
        let insecure = std::env::var("INSECURE_COORDINATOR")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let http = reqwest::Client::builder()
            .danger_accept_invalid_certs(insecure)
            .build()
            .expect("build reqwest client");
        Self {
            pinaivu_api_base,
            pinaivu_api_key,
            http,
        }
    }

    pub async fn chat_completions(&self, body: &ChatCompletionBody<'_>) -> Result<NodeReply> {
        let url = format!(
            "{}/v1/chat/completions",
            self.pinaivu_api_base.trim_end_matches('/')
        );
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.pinaivu_api_key)
            .json(body)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        if !resp.status().is_success() {
            anyhow::bail!(
                "pinaivu-api {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            );
        }
        Ok(resp.json().await.context("decode chat completion reply")?)
    }
}
