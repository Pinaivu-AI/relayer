use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{verify_signature, Authed};
use crate::error::AppError;
use crate::memory::{analyze, recall};
use crate::state::AppState;
use crate::upstream::{ChatMessageOut, InferenceBody, OpenChatBody};

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/enclave_health", get(enclave_health))
        .route("/get_attestation", get(get_attestation))
        .route("/v1/chat", post(handle_chat))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

#[derive(Serialize)]
struct EnclaveHealthResp {
    pubkey_hex: String,
    uptime_s: u64,
}

async fn enclave_health(State(s): State<AppState>) -> Json<EnclaveHealthResp> {
    Json(EnclaveHealthResp {
        pubkey_hex: s.enclave.public_key_hex(),
        uptime_s: 0,
    })
}

async fn get_attestation(State(_s): State<AppState>) -> &'static str {
    // In enclave mode, return the raw NSM CBOR doc via nautilus_enclave.
    // Binary mode returns this placeholder.
    "not-in-enclave"
}

// ── /v1/chat ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ChatReq {
    /// OpenAI-style message history (last entry is the new user message).
    pub messages: Vec<ChatMsg>,
    pub model: String,

    /// If present the relayer continues an existing session; otherwise mints one.
    #[serde(default)]
    pub session_id: Option<Uuid>,
    /// AES-256 key (base64) for the Walrus session blob. Client generates on first
    /// turn and persists locally; relayer forwards to the node.
    #[serde(default)]
    pub session_key: Option<String>,

    /// MemWal account identifier — owner's Sui address or MemWal handle.
    pub owner_address: String,
    /// Memory namespace — separates different agents/applications per owner.
    #[serde(default = "default_namespace")]
    pub namespace: String,

    /// Ed25519 delegate pubkey (hex) — used for auth + request signing.
    pub delegate_pubkey_hex: String,
    /// Signature (hex) over the canonical form of this request body.
    pub signature_hex: String,
}

fn default_namespace() -> String {
    "default".into()
}

#[derive(Deserialize, Serialize, Clone)]
pub struct ChatMsg {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
pub struct ChatResp {
    pub content: String,
    pub session_id: Uuid,
    /// AES-256 key (base64) for this session's Walrus blob. The caller
    /// must persist this and resend it as `session_key` on every later
    /// turn of the same session — the relayer never stores it.
    pub session_key: String,
    pub request_id: Uuid,
    /// Facts recalled from long-term memory that were injected into this turn.
    pub recalled_facts: Vec<String>,
    pub latency_ms: u32,
}

async fn handle_chat(
    State(s): State<AppState>,
    Json(req): Json<ChatReq>,
) -> Result<Json<ChatResp>, AppError> {
    // Auth: verify delegate-key signature over canonical request bytes.
    let canonical = canonical_chat_req(&req);
    verify_signature(&req.delegate_pubkey_hex, &req.signature_hex, &canonical)?;

    // Resolve owner address (Sui lookup). Falls back to the caller-supplied address in dev.
    let owner_address = s
        .sui
        .owner_for_delegate(&req.delegate_pubkey_hex)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| req.owner_address.clone());

    // Rate limit by owner.
    s.rate_limiter.check(&owner_address).await?;

    // Extract the newest user message.
    let last_msg = req
        .messages
        .last()
        .filter(|m| m.role == "user")
        .ok_or_else(|| AppError::BadRequest("messages must end with a user turn".into()))?;

    // Recall relevant long-term memories.
    let recalled = recall::recall(
        &s.pg,
        &s.embed,
        &s.crypto,
        &s.walrus,
        &owner_address,
        &req.namespace,
        &last_msg.content,
        5,
    )
    .await
    .inspect_err(|e| tracing::warn!(error = %e, "recall failed"))
    .unwrap_or_default();

    let recalled_facts: Vec<String> = recalled.iter().map(|r| r.plaintext.clone()).collect();
    let memwal_context = if recalled_facts.is_empty() {
        None
    } else {
        Some(recalled_facts.join("\n"))
    };

    // Open a chat slot on the coordinator (auction → dispatch token).
    let session_id = req.session_id.unwrap_or_else(Uuid::new_v4);
    let messages_out: Vec<ChatMessageOut<'_>> = req
        .messages
        .iter()
        .map(|m| ChatMessageOut { role: &m.role, content: &m.content })
        .collect();
    let dispatch = s
        .upstream
        .open_chat(&OpenChatBody {
            model: &req.model,
            messages: &messages_out,
            client_pubkey_hex: &req.delegate_pubkey_hex,
            session_id: Some(session_id),
        })
        .await
        .map_err(|e| AppError::Upstream(e.to_string()))?;

    // Determine session key (caller-supplied or generate a fresh one).
    let session_key = req.session_key.unwrap_or_else(|| {
        use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
        use rand::RngCore;
        let mut k = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut k);
        B64.encode(k)
    });

    // Forward inference to the winning node.
    let reply = s
        .upstream
        .run_inference(
            &dispatch.node_url,
            &InferenceBody {
                dispatch_token: &dispatch.dispatch_token,
                session_id: dispatch.session_id,
                session_key: session_key.clone(),
                new_user_message: &last_msg.content,
                memwal_context: memwal_context.as_deref(),
            },
        )
        .await
        .map_err(|e| AppError::Upstream(e.to_string()))?;

    // Background: analyze the completed turn and store new memory.
    let pg2 = s.pg.clone();
    let embed2 = s.embed.clone();
    let crypto2 = s.crypto.clone();
    let walrus2 = s.walrus.clone();
    let owner2 = owner_address.clone();
    let ns2 = req.namespace.clone();
    let reply_content = reply.content.clone();
    let user_content = last_msg.content.clone();
    tokio::spawn(async move {
        let turn_text = format!("user: {user_content}\nassistant: {reply_content}");
        if let Err(e) = analyze::analyze(
            &pg2,
            &embed2,
            &crypto2,
            &walrus2,
            &owner2,
            &ns2,
            dispatch.session_id,
            &turn_text,
        )
        .await
        {
            tracing::warn!(error = %e, "analyze failed");
        }
    });

    Ok(Json(ChatResp {
        content: reply.content,
        session_id: dispatch.session_id,
        session_key,
        request_id: reply.request_id,
        recalled_facts,
        latency_ms: reply.latency_ms,
    }))
}

/// Canonical bytes for signature verification.
/// Covers the fields that represent the caller's intent.
fn canonical_chat_req(req: &ChatReq) -> Vec<u8> {
    use std::fmt::Write;
    let mut s = String::new();
    for m in &req.messages {
        write!(s, "{}:{}\n", m.role, m.content).ok();
    }
    write!(s, "model:{}\nowner:{}\nns:{}", req.model, req.owner_address, req.namespace).ok();
    s.into_bytes()
}
