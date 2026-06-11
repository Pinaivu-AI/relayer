//! Environment-driven configuration. In prod the values come from
//! the VSOCK:7000 config push at enclave boot (same pattern as the
//! coordinator + nautilus-memwal-relayer); locally a `.env` works.

use anyhow::Context;

#[derive(Debug, Clone)]
pub struct Config {
    /// TCP socket the axum router binds. Default `127.0.0.1:4002`.
    pub bind_addr: String,

    /// Postgres URL — pgvector embeddings, audit log, apalis jobs.
    pub database_url: String,
    /// Redis URL — rate limit + replay nonces.
    pub redis_url: String,

    /// TS sidecar address (the Seal + Walrus bridge running as a child
    /// process on `localhost:9000` by default).
    pub sidecar_url: String,
    /// Shared secret the Rust binary sends in `X-Sidecar-Secret` so the
    /// sidecar refuses any other caller.
    pub sidecar_secret: String,

    /// Pinaivu-API gateway base URL (target of upstream chat completions).
    pub pinaivu_api_base: String,

    /// Embedding service — OpenAI-compatible HTTP endpoint. Both the
    /// model id and the resulting vector dimension are env-driven so a
    /// deployment can pick its own embedder.
    pub embedding_api_base: String,
    pub embedding_api_key: Option<String>,
    pub embedding_model: String,
    pub embedding_dim: usize,

    /// Sui RPC URL — used to look up delegate-key → owner address.
    pub sui_rpc_url: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let _ = dotenvy::dotenv();
        Ok(Self {
            bind_addr: env_or("CHAT_RELAYER_BIND", "127.0.0.1:4002"),
            database_url: req("DATABASE_URL")?,
            redis_url: req("REDIS_URL")?,
            sidecar_url: env_or("SIDECAR_URL", "http://127.0.0.1:9000"),
            sidecar_secret: req("SIDECAR_SECRET")?,
            pinaivu_api_base: req("PINAIVU_API_BASE")?,
            embedding_api_base: env_or("EMBEDDING_API_BASE", "https://api.openai.com/v1"),
            embedding_api_key: std::env::var("EMBEDDING_API_KEY").ok(),
            embedding_model: env_or("EMBEDDING_MODEL", "text-embedding-3-small"),
            embedding_dim: std::env::var("EMBEDDING_DIM")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1536),
            sui_rpc_url: env_or("SUI_RPC_URL", "https://fullnode.testnet.sui.io"),
        })
    }
}

fn req(key: &str) -> anyhow::Result<String> {
    std::env::var(key).with_context(|| format!("env {key} not set"))
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}
