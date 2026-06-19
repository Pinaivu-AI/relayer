//! chat-relayer — Nitro-enclave Rust service for chat.pinaivu.ai.
//!
//! Wraps every chat turn with cross-session memory: recall before the
//! model, analyze after. Carries its own encryption + Walrus + pgvector
//! stack (architecturally borrowed from MemWal, configurable for the
//! embedding model + vector dimension that the chat product needs).
//! Forwards the actual inference to pinaivu-api → coordinator → node.

mod auth;
mod config;
mod crypto;
mod db;
mod enclave;
mod error;
mod http;
mod memory;
mod rate_limit;
mod state;
mod sui;
mod telemetry;
mod upstream;
mod walrus;

use anyhow::{Context, Result};

#[tokio::main]
async fn main() -> Result<()> {
    telemetry::init();

    let cfg = config::Config::from_env().context("load chat-relayer config")?;

    let state = state::AppState::new(&cfg)
        .await
        .context("build chat-relayer state")?;

    let app = http::build_router(state);

    let bind = cfg.bind_addr.clone();
    tracing::info!(%bind, "chat-relayer http ready");

    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .with_context(|| format!("bind {bind}"))?;
    axum::serve(listener, app).await.context("axum serve")?;
    Ok(())
}
