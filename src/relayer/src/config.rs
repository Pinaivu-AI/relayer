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

    /// Master secret memory blobs are encrypted under (per-owner keys are
    /// HKDF-derived from this). Placeholder for real Seal access control —
    /// see crypto.rs.
    pub memory_encryption_key: [u8; 32],

    /// Walrus HTTP endpoints — same publisher/aggregator pair the node uses.
    pub walrus_publisher_url: String,
    pub walrus_aggregator_url: String,
    pub walrus_epochs: u32,

    /// Pinaivu-API base URL (target of upstream chat completions). For
    /// local testing this points directly at the coordinator's HTTPS
    /// endpoint, skipping pinaivu-api/gateway.
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
            memory_encryption_key: parse_hex_key(&req("MEMORY_ENCRYPTION_KEY")?)?,
            walrus_publisher_url: req("WALRUS_PUBLISHER_URL")?,
            walrus_aggregator_url: req("WALRUS_AGGREGATOR_URL")?,
            walrus_epochs: std::env::var("WALRUS_EPOCHS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5),
            pinaivu_api_base: req("PINAIVU_API_BASE")?,
            embedding_api_base: env_or("EMBEDDING_API_BASE", "https://api.jina.ai/v1"),
            embedding_api_key: std::env::var("EMBEDDING_API_KEY").ok(),
            embedding_model: env_or("EMBEDDING_MODEL", "jina-embeddings-v5-text-small"),
            embedding_dim: req("EMBEDDING_DIM")?
                .parse()
                .context("EMBEDDING_DIM must be an integer")?,
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

fn parse_hex_key(s: &str) -> anyhow::Result<[u8; 32]> {
    let bytes = hex::decode(s).context("MEMORY_ENCRYPTION_KEY must be hex")?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("MEMORY_ENCRYPTION_KEY must decode to 32 bytes"))
}
