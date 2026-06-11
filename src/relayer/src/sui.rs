//! Sui RPC helpers — looks up the owner address for a given delegate
//! public key by walking the on-chain `MemWalAccount.delegate_keys`
//! mapping (same mapping MemWal uses; we read it via a public RPC call).

use anyhow::Result;

#[derive(Clone)]
pub struct SuiClient {
    rpc_url: String,
    http: reqwest::Client,
}

impl SuiClient {
    pub fn new(rpc_url: String) -> Self {
        Self {
            rpc_url,
            http: reqwest::Client::new(),
        }
    }

    /// Look up the owner Sui address associated with a delegate Ed25519
    /// public key. Implementation filled in once we wire the actual
    /// MemWalAccount contract reads.
    pub async fn owner_for_delegate(&self, _delegate_pubkey_hex: &str) -> Result<Option<String>> {
        // Stub for Step 1. Real implementation queries the account
        // registry the same way MemWal's relayer does.
        Ok(None)
    }
}
