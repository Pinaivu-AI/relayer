//! Outbound calls to pinaivu-api → coordinator → node.
//!
//! The two halves of a chat turn:
//!   1. `open_chat` posts to `pinaivu-api/v1/chat/completions` so the
//!      coordinator runs the auction and returns `{ node_url,
//!      dispatch_token, session_id }`.
//!   2. `run_inference` posts the actual prompt to the node directly,
//!      receives the assistant reply.

use anyhow::{Context, Result};
use pinaivu_protocol::DispatchToken;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone)]
pub struct UpstreamClient {
    pinaivu_api_base: String,
    pinaivu_api_key: String,
    http: reqwest::Client,
}

#[derive(Serialize)]
pub struct OpenChatBody<'a> {
    pub model: &'a str,
    pub messages: &'a [ChatMessageOut<'a>],
    pub client_pubkey_hex: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct ChatMessageOut<'a> {
    pub role: &'a str,
    pub content: &'a str,
}

#[derive(Deserialize)]
pub struct DispatchResp {
    pub request_id: Uuid,
    pub session_id: Uuid,
    pub node_url: String,
    pub dispatch_token: DispatchToken,
}

#[derive(Serialize)]
pub struct InferenceBody<'a> {
    pub dispatch_token: &'a DispatchToken,
    pub session_id: Uuid,
    /// AES-256 key (base64) so the node can decrypt the Walrus session
    /// blob. The relayer mints + caches per-user/session.
    pub session_key: String,
    pub new_user_message: &'a str,
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

    pub async fn open_chat(&self, body: &OpenChatBody<'_>) -> Result<DispatchResp> {
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
        Ok(resp.json().await.context("decode dispatch resp")?)
    }

    pub async fn run_inference(&self, node_url: &str, body: &InferenceBody<'_>) -> Result<NodeReply> {
        let url = format!("{}/v1/inference", node_url.trim_end_matches('/'));
        let resp = self
            .http
            .post(&url)
            .json(body)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        if !resp.status().is_success() {
            anyhow::bail!(
                "node {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            );
        }
        Ok(resp.json().await.context("decode node reply")?)
    }
}
