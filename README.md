# chat-relayer

Pinaivu's chat-relayer — a Nitro Enclave service backing `chat.pinaivu.com`
with cross-session memory. It is the off-chain-but-verifiable backend
for the chat product, the same trust shape as the coordinator: a single
instance today, but its code runs inside an attested enclave and is
checkable against an on-chain record. See the [decentralization &
verifiability model](https://docs.pinaivu.com/architecture/decentralization)
for how it differs from the genuinely decentralized GPU mesh, and the
[memory layers doc](https://docs.pinaivu.com/architecture/memory-layers)
for how this cross-session layer composes with the node's intra-session
Walrus session blob.

A chat turn does two things the developer-facing gateway doesn't:

1. **Recalls** relevant long-term facts about the user from past
   sessions (embedding similarity search over encrypted memory blobs).
2. **Analyzes** the new turn in the background and stores any new
   facts worth remembering.

The relayer then forwards the turn (plus recalled context) to the
coordinator's auction, same as any other client, and the response
streams back from whichever node wins.

## Architecture

```
client ── POST /v1/chat ──▶ chat-relayer (Nitro Enclave)
                              │ 1. recall: embed query, pgvector search,
                              │    decrypt matching memory blobs
                              │ 2. forward to coordinator's
                              │    /v1/chat/completions (with memwal_context)
                              ▼
                            coordinator ── auction ──▶ node
                              │ 3. fire-and-forget: analyze the turn,
                              │    embed + encrypt + store new facts
                              ▼
client ◀── reply + recalled_facts ──┘
```

Memory blobs are encrypted with a key derived per-owner via HKDF from a
relayer-held secret (`crypto.rs`) and stored on Walrus (`walrus.rs`,
plain HTTP — no Node-only SDK dependency). This is a placeholder for
real Seal-based, wallet-gated access control, not a production
access-control story yet.

## Endpoints

| Endpoint | Purpose |
|---|---|
| `POST /v1/chat` | Recall, dispatch to the coordinator, analyze |
| `GET /health` | Liveness |
| `GET /enclave_health` | Pubkey, uptime, registered Sui enclave object id |
| `GET /get_attestation` | NSM attestation document |

## Crate layout

| Path | Role |
|---|---|
| `src/relayer/` | Main binary + library — axum HTTP, memory (embed/recall/analyze), upstream client, auth, rate limiting |
| `src/pinaivu-protocol/` | Shared wire types, reused from the coordinator workspace |
| `src/nautilus-enclave/`, `src/aws/`, `src/init/`, `src/system/` | Enclave attestation + Nitro init, same pattern as the coordinator |

## Running

Requires Postgres (with `pgvector`), Redis, and an embeddings API key:

```bash
cp .env.example .env   # DATABASE_URL, REDIS_URL, MEMORY_ENCRYPTION_KEY,
                        # WALRUS_PUBLISHER_URL, WALRUS_AGGREGATOR_URL,
                        # EMBEDDING_API_KEY, EMBEDDING_DIM, PINAIVU_API_BASE
cargo run -p relayer
```

`MEMORY_ENCRYPTION_KEY` is a 32-byte hex secret (`openssl rand -hex 32`).
`EMBEDDING_DIM` must match the embedding model's actual output
dimension — the pgvector column size is fixed at migration time.

## Tests

```bash
cargo test --workspace
```

## Building the enclave image

```bash
make eif
```

Produces `chat-relayer.eif` + `chat-relayer.pcrs` via the same
stagex/Nitro pipeline as the coordinator.
