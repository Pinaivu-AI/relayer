//! Postgres pool + chat-relayer migrations.
//!
//! Tables:
//!   * `memory_blobs` — pgvector embedding + Walrus blob pointer, scoped
//!     by `owner_address + namespace`.
//!   * `memory_jobs`  — apalis-shaped queue for background Walrus uploads.
//!   * `chat_sessions` — minimal audit row per chat turn (request_id,
//!     owner_address, model, latency_ms, created_at). Useful for the
//!     dashboard later.

use anyhow::Result;
use sqlx::PgPool;

pub async fn connect(database_url: &str, embedding_dim: usize) -> Result<PgPool> {
    let pool = PgPool::connect(database_url).await?;
    run_migrations(&pool, embedding_dim).await?;
    Ok(pool)
}

async fn run_migrations(pool: &PgPool, embedding_dim: usize) -> Result<()> {
    // pgvector extension. Idempotent.
    sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
        .execute(pool)
        .await?;

    // The vector dimension is part of the type, so we interpolate it
    // into the table DDL. Migrating to a different dimension is a
    // deployment-level decision (drop + recreate, or shadow table).
    let memory_blobs = format!(
        r#"CREATE TABLE IF NOT EXISTS memory_blobs (
            id              UUID         PRIMARY KEY,
            owner_address   TEXT         NOT NULL,
            namespace       TEXT         NOT NULL,
            session_id      UUID,
            blob_id         TEXT         NOT NULL,
            embedding       vector({embedding_dim}) NOT NULL,
            created_at      TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
            UNIQUE (owner_address, blob_id)
        )"#
    );

    let statements: &[&str] = &[
        &memory_blobs,
        "CREATE INDEX IF NOT EXISTS memory_blobs_owner_ns_idx
            ON memory_blobs (owner_address, namespace)",
        // ivfflat needs the table to have rows before it's useful, but
        // creating it idempotently is fine and the planner picks it up
        // when there's enough data.
        "CREATE INDEX IF NOT EXISTS memory_blobs_embedding_idx
            ON memory_blobs USING ivfflat (embedding vector_cosine_ops)
            WITH (lists = 100)",
        r#"CREATE TABLE IF NOT EXISTS chat_sessions (
            request_id      UUID         PRIMARY KEY,
            owner_address   TEXT         NOT NULL,
            model           TEXT         NOT NULL,
            input_tokens    INT,
            output_tokens   INT,
            latency_ms      INT,
            created_at      TIMESTAMPTZ  NOT NULL DEFAULT NOW()
        )"#,
        "CREATE INDEX IF NOT EXISTS chat_sessions_owner_idx
            ON chat_sessions (owner_address, created_at DESC)",
    ];

    for stmt in statements {
        sqlx::query(stmt).execute(pool).await?;
    }
    Ok(())
}
