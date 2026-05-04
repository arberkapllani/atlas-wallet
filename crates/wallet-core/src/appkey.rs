//! Application-scoped symmetric keys derived from the unlocked seed.
//!
//! Atlas keeps the BIP-39 seed in memory only while the wallet is
//! unlocked. Subsystems that need their own at-rest encryption key
//! (address book, NFT cache, sync metadata, …) derive a 32-byte
//! AES-256 key from the seed using HMAC-SHA256 with a fixed
//! domain-separation context string.
//!
//! The seed never leaves this crate, and the derived keys are
//! independent: leaking one (e.g. via a memory dump while a sub-tab
//! holds it open) does not leak any other key, and crucially does
//! not leak the seed itself — HMAC is one-way.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::error::{Error, Result};

type HmacSha256 = Hmac<Sha256>;

/// Derive a 32-byte symmetric key from the BIP-39 seed for `context`.
///
/// `context` should be a stable ASCII string identifying the consumer
/// (e.g. `"atlas/address-book/v1"`). Different contexts produce
/// independent keys; the same context with the same seed always
/// produces the same key (so encrypted blobs decrypt across sessions).
pub fn derive_app_key(seed: &[u8; 64], context: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    let mut mac = HmacSha256::new_from_slice(seed).map_err(|e| Error::Kdf(e.to_string()))?;
    mac.update(context);
    let tag = mac.finalize().into_bytes();
    let mut out = Zeroizing::new([0u8; 32]);
    out.copy_from_slice(&tag);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_per_context() {
        let seed = [7u8; 64];
        let a = derive_app_key(&seed, b"atlas/address-book/v1").unwrap();
        let b = derive_app_key(&seed, b"atlas/address-book/v1").unwrap();
        assert_eq!(*a, *b, "same seed + context must produce same key");
    }

    #[test]
    fn different_contexts_diverge() {
        let seed = [7u8; 64];
        let a = derive_app_key(&seed, b"atlas/address-book/v1").unwrap();
        let b = derive_app_key(&seed, b"atlas/nft-cache/v1").unwrap();
        assert_ne!(*a, *b, "different contexts must produce different keys");
    }

    #[test]
    fn different_seeds_diverge() {
        let a = derive_app_key(&[1u8; 64], b"atlas/x").unwrap();
        let b = derive_app_key(&[2u8; 64], b"atlas/x").unwrap();
        assert_ne!(*a, *b);
    }
}
