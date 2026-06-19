use anyhow::{Context, Result};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::crypto::MemoryCrypto;
use crate::walrus::WalrusClient;

use super::embed::EmbeddingClient;

pub struct RecalledMemory {
    pub id: Uuid,
    pub plaintext: String,
    pub score: f32,
}

pub async fn recall(
    pg: &PgPool,
    embed: &EmbeddingClient,
    crypto: &MemoryCrypto,
    walrus: &WalrusClient,
    owner_address: &str,
    namespace: &str,
    query: &str,
    top_k: i64,
) -> Result<Vec<RecalledMemory>> {
    let qvec = embed.embed_query(query).await.context("embed recall query")?;
    // Encode as Postgres vector literal: '[1.0,2.0,...]'
    let vec_literal = format!(
        "[{}]",
        qvec.iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );

    let rows = sqlx::query(
        r#"
        SELECT id, blob_id,
               (embedding <=> $1::vector)::float4 AS score
        FROM memory_blobs
        WHERE owner_address = $2 AND namespace = $3
        ORDER BY embedding <=> $1::vector
        LIMIT $4
        "#,
    )
    .bind(&vec_literal)
    .bind(owner_address)
    .bind(namespace)
    .bind(top_k)
    .fetch_all(pg)
    .await
    .context("pgvector recall query")?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let blob_id: String = row.try_get("blob_id").context("blob_id")?;
        let score: Option<f32> = row.try_get("score").ok().flatten();
        let id: Uuid = row.try_get("id").context("id")?;

        let ciphertext = walrus
            .download(&blob_id)
            .await
            .with_context(|| format!("walrus download blob {blob_id}"))?
            .unwrap_or_default();
        let plaintext_bytes = crypto
            .decrypt(owner_address, &ciphertext)
            .with_context(|| format!("decrypt blob {blob_id}"))?;
        let plaintext = String::from_utf8(plaintext_bytes).context("utf8 plaintext")?;
        out.push(RecalledMemory {
            id,
            plaintext,
            score: score.unwrap_or(1.0),
        });
    }

    Ok(out)
}
