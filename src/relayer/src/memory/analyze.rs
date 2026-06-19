use anyhow::{Context, Result};
use sqlx::PgPool;
use uuid::Uuid;

use crate::crypto::MemoryCrypto;
use crate::walrus::WalrusClient;

use super::embed::EmbeddingClient;

/// Extract facts from a completed chat turn and store them.
///
/// `turn_text` is typically `"user: {msg}\nassistant: {reply}"`.
pub async fn analyze(
    pg: &PgPool,
    embed: &EmbeddingClient,
    crypto: &MemoryCrypto,
    walrus: &WalrusClient,
    owner_address: &str,
    namespace: &str,
    session_id: Uuid,
    turn_text: &str,
) -> Result<()> {
    let embedding = embed
        .embed_passage(turn_text)
        .await
        .context("embed turn for analyze")?;

    let ciphertext = crypto.encrypt(owner_address, turn_text.as_bytes());

    let blob_id = walrus
        .upload(&ciphertext)
        .await
        .context("walrus upload turn")?;

    let vec_literal = format!(
        "[{}]",
        embedding
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    sqlx::query(
        r#"
        INSERT INTO memory_blobs
            (id, owner_address, namespace, session_id, blob_id, embedding, created_at)
        VALUES ($1, $2, $3, $4, $5, $6::vector, NOW())
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(owner_address)
    .bind(namespace)
    .bind(session_id)
    .bind(&blob_id)
    .bind(&vec_literal)
    .execute(pg)
    .await
    .context("insert memory_blob row")?;

    Ok(())
}
