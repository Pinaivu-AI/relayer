//! Memory layer (recall + analyze + store).
//!
//! Each public memory blob is:
//!   embedding (pgvector) + ciphertext-on-walrus (Seal-encrypted),
//!   keyed by (owner_address, namespace).
//!
//! `recall(query)` embeds the query, pgvector-searches the top-K, then
//! Walrus-fetches + Seal-decrypts each hit.
//!
//! `analyze(turn)` is the inverse: the relayer extracts fact-shaped
//! summaries from a chat turn (using a small inference call against
//! the same Pinaivu network — or an external embedder for the MVP),
//! Seal-encrypts each, Walrus-uploads, and inserts the row.

pub mod analyze;
pub mod embed;
pub mod recall;
