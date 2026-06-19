use std::sync::Arc;

use anyhow::{Context, Result};
use sqlx::PgPool;

use crate::auth::AuthVerifier;
use crate::config::Config;
use crate::crypto::MemoryCrypto;
use crate::db;
use crate::enclave::Enclave;
use crate::memory::embed::EmbeddingClient;
use crate::rate_limit::RateLimiter;
use crate::sui::SuiClient;
use crate::upstream::UpstreamClient;
use crate::walrus::WalrusClient;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub enclave: Enclave,
    pub pg: PgPool,
    pub embed: EmbeddingClient,
    pub crypto: MemoryCrypto,
    pub walrus: WalrusClient,
    pub sui: SuiClient,
    pub upstream: UpstreamClient,
    pub rate_limiter: RateLimiter,
    pub auth: AuthVerifier,
}

impl AppState {
    pub async fn new(cfg: &Config) -> Result<Self> {
        let pg = db::connect(&cfg.database_url, cfg.embedding_dim)
            .await
            .context("postgres connect")?;

        let redis = redis::Client::open(cfg.redis_url.as_str())
            .context("redis client")?;
        let redis_mgr = redis::aio::ConnectionManager::new(redis)
            .await
            .context("redis connection manager")?;

        let enclave = Enclave::new().context("enclave key")?;

        let embed = EmbeddingClient::new(
            cfg.embedding_api_base.clone(),
            cfg.embedding_api_key.clone(),
            cfg.embedding_model.clone(),
            cfg.embedding_dim,
        );

        let crypto = MemoryCrypto::new(cfg.memory_encryption_key);
        let walrus = WalrusClient::new(
            cfg.walrus_publisher_url.clone(),
            cfg.walrus_aggregator_url.clone(),
            cfg.walrus_epochs,
        );
        let sui = SuiClient::new(cfg.sui_rpc_url.clone());
        let upstream = UpstreamClient::new(cfg.pinaivu_api_base.clone());
        let rate_limiter = RateLimiter::new(redis_mgr, 60);
        let auth = AuthVerifier::new();

        Ok(Self {
            cfg: Arc::new(cfg.clone()),
            enclave,
            pg,
            embed,
            crypto,
            walrus,
            sui,
            upstream,
            rate_limiter,
            auth,
        })
    }
}
