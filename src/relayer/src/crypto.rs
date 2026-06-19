//! Per-owner symmetric encryption for memory blobs.
//!
//! Placeholder for real Seal access control (which requires a
//! wallet-signed `SessionKey` per request — not yet implemented here).
//! The relayer holds a single master secret; every owner's key is
//! derived from it via HKDF-SHA256, so only someone holding the master
//! secret can decrypt any owner's blobs. Wire format matches
//! `node/src/cipher.rs`: `nonce_12_bytes || ciphertext_with_tag`.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{anyhow, Result};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;

const NONCE_LEN: usize = 12;

#[derive(Clone)]
pub struct MemoryCrypto {
    master_key: [u8; 32],
}

impl MemoryCrypto {
    pub fn new(master_key: [u8; 32]) -> Self {
        Self { master_key }
    }

    fn derive_key(&self, owner_address: &str) -> [u8; 32] {
        let hk = Hkdf::<Sha256>::new(Some(owner_address.as_bytes()), &self.master_key);
        let mut out = [0u8; 32];
        hk.expand(b"pinaivu-chat-relayer-memory", &mut out)
            .expect("32 bytes is a valid HKDF output length");
        out
    }

    pub fn encrypt(&self, owner_address: &str, plaintext: &[u8]) -> Vec<u8> {
        let key = self.derive_key(owner_address);
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = cipher
            .encrypt(nonce, plaintext)
            .expect("AES-GCM encrypt is infallible for valid key + nonce");
        let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ct);
        out
    }

    pub fn decrypt(&self, owner_address: &str, blob: &[u8]) -> Result<Vec<u8>> {
        if blob.len() < NONCE_LEN {
            return Err(anyhow!("ciphertext too short to contain a nonce"));
        }
        let key = self.derive_key(owner_address);
        let (nonce_bytes, ct) = blob.split_at(NONCE_LEN);
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
        cipher
            .decrypt(Nonce::from_slice(nonce_bytes), ct)
            .map_err(|_| anyhow!("AEAD authentication failed (wrong key or tampered ciphertext)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let crypto = MemoryCrypto::new([7u8; 32]);
        let ct = crypto.encrypt("0xabc", b"hello pinaivu");
        let pt = crypto.decrypt("0xabc", &ct).unwrap();
        assert_eq!(pt, b"hello pinaivu");
    }

    #[test]
    fn different_owners_get_different_keys() {
        let crypto = MemoryCrypto::new([7u8; 32]);
        let ct = crypto.encrypt("0xabc", b"secret");
        assert!(crypto.decrypt("0xdef", &ct).is_err());
    }

    #[test]
    fn fresh_nonce_per_call() {
        let crypto = MemoryCrypto::new([9u8; 32]);
        let a = crypto.encrypt("0xabc", b"same plaintext");
        let b = crypto.encrypt("0xabc", b"same plaintext");
        assert_ne!(a, b, "nonce reuse would produce identical ciphertexts");
    }
}
