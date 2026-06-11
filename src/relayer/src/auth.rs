//! Delegate-key authentication. The chat client signs each request
//! body (canonical JSON) with its Ed25519 delegate key; the relayer
//! verifies the signature and resolves the owner address via Sui.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};

use crate::error::AppError;

/// Stateless helper — clone-cheap (zero heap).
#[derive(Clone)]
pub struct AuthVerifier;

impl AuthVerifier {
    pub fn new() -> Self {
        Self
    }
}

pub struct Authed {
    pub delegate_pubkey_hex: String,
    /// Owner Sui address (looked up via Sui RPC). When the lookup
    /// fails or is skipped (dev mode), this falls back to the
    /// delegate pubkey hex string.
    pub owner_address: String,
}

/// Verify an Ed25519 signature over `canonical` produced by
/// `delegate_pubkey_hex`. Returns the parsed pubkey for downstream use.
pub fn verify_signature(
    delegate_pubkey_hex: &str,
    signature_hex: &str,
    canonical: &[u8],
) -> Result<(), AppError> {
    let pk_bytes = hex::decode(delegate_pubkey_hex)
        .map_err(|_| AppError::Unauthorized("delegate pubkey not hex".into()))?;
    let pk_arr: [u8; 32] = pk_bytes
        .as_slice()
        .try_into()
        .map_err(|_| AppError::Unauthorized("delegate pubkey must be 32 bytes".into()))?;
    let vk = VerifyingKey::from_bytes(&pk_arr)
        .map_err(|_| AppError::Unauthorized("delegate pubkey not on curve".into()))?;

    let sig_bytes = hex::decode(signature_hex)
        .map_err(|_| AppError::Unauthorized("signature not hex".into()))?;
    let sig = Signature::from_slice(&sig_bytes)
        .map_err(|_| AppError::Unauthorized("signature bytes malformed".into()))?;

    vk.verify(canonical, &sig)
        .map_err(|_| AppError::Unauthorized("signature does not verify".into()))
}
