//! Wrapper around `nautilus-enclave`'s ephemeral key + NSM attestation.
//! Mirrors the coordinator's enclave wiring so we can register this
//! relayer on Sui as its own `Enclave<ChatRelayer>` once Phase 12-style
//! registration is wired in.

use anyhow::Result;
use std::sync::Arc;

pub use nautilus_enclave::EnclaveKeyPair;

#[derive(Clone)]
pub struct Enclave {
    inner: Arc<EnclaveKeyPair>,
}

impl Enclave {
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: Arc::new(EnclaveKeyPair::generate()),
        })
    }

    pub fn key(&self) -> &EnclaveKeyPair {
        &self.inner
    }

    pub fn public_key_hex(&self) -> String {
        hex::encode(self.inner.public_key_bytes())
    }
}
