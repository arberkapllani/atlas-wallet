//! ENS name validation + namehash (EIP-137).
//!
//! No I/O. Computes the deterministic 32-byte node hash that an
//! on-chain resolver call needs, plus light validation (allowed
//! characters, label length, total length, lowercasing). Actual
//! `resolver.addr(node)` RPC lives in the host.
//!
//! Reference: https://docs.ens.domains/ens-improvement-proposals/ensip-1-ens
//!
//! Normalisation here is intentionally simple — it lower-cases and
//! enforces ASCII letters, digits, hyphen and dot. Full UTS-46 is
//! out of scope for this crate; an upstream layer can run UTS-46
//! and pass the normalised name in.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use specta::Type;
use thiserror::Error;

#[derive(Debug, Error, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "message")]
pub enum EnsError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

const MAX_NAME_LEN: usize = 255;
const MAX_LABEL_LEN: usize = 63;

/// Looks like an ENS name (case-insensitive `.eth`, `.xyz`, ...).
/// Bare hex addresses or strings without a dot are rejected.
pub fn looks_like_ens(input: &str) -> bool {
    let s = input.trim();
    if s.is_empty() || s.len() > MAX_NAME_LEN {
        return false;
    }
    if !s.contains('.') {
        return false;
    }
    let lower = s.to_lowercase();
    lower
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
}

/// Normalise: trim, lowercase, validate. Returns the canonical
/// form ready to feed into `namehash`.
pub fn normalise(input: &str) -> Result<String, EnsError> {
    let s = input.trim();
    if s.is_empty() {
        return Err(EnsError::InvalidInput("empty name".into()));
    }
    if s.len() > MAX_NAME_LEN {
        return Err(EnsError::InvalidInput(format!(
            "name {} > {MAX_NAME_LEN}",
            s.len()
        )));
    }
    if s.starts_with('.') || s.ends_with('.') {
        return Err(EnsError::InvalidInput("leading or trailing dot".into()));
    }
    let lower = s.to_lowercase();
    for label in lower.split('.') {
        if label.is_empty() {
            return Err(EnsError::InvalidInput("empty label".into()));
        }
        if label.len() > MAX_LABEL_LEN {
            return Err(EnsError::InvalidInput(format!(
                "label too long: {}",
                label.len()
            )));
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err(EnsError::InvalidInput(
                "label may not start or end with '-'".into(),
            ));
        }
        if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(EnsError::InvalidInput(format!(
                "label has invalid character: {label}"
            )));
        }
    }
    Ok(lower)
}

/// EIP-137 namehash. Empty string → 32 zero bytes; otherwise
/// `keccak256(namehash(parent) || keccak256(label))` recursively.
pub fn namehash(name: &str) -> Result<[u8; 32], EnsError> {
    let normalised = if name.is_empty() {
        String::new()
    } else {
        normalise(name)?
    };
    let mut node = [0u8; 32];
    if normalised.is_empty() {
        return Ok(node);
    }
    let labels: Vec<&str> = normalised.split('.').collect();
    for label in labels.iter().rev() {
        let label_hash = keccak(label.as_bytes());
        let mut buf = [0u8; 64];
        buf[..32].copy_from_slice(&node);
        buf[32..].copy_from_slice(&label_hash);
        node = keccak(&buf);
    }
    Ok(node)
}

/// Hex-encode the 32-byte namehash with a leading `0x`.
pub fn namehash_hex(name: &str) -> Result<String, EnsError> {
    let bytes = namehash(name)?;
    let mut out = String::with_capacity(2 + 64);
    out.push_str("0x");
    for b in bytes {
        out.push(hex_nibble(b >> 4));
        out.push(hex_nibble(b & 0x0f));
    }
    Ok(out)
}

fn hex_nibble(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        _ => (b'a' + n - 10) as char,
    }
}

fn keccak(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(bytes);
    let out = hasher.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string_namehash_is_zero() {
        let h = namehash("").unwrap();
        assert_eq!(h, [0u8; 32]);
    }

    /// Reference vector from EIP-137:
    /// namehash("eth") =
    /// 0x93cdeb708b7545dc668eb9280176169d1c33cfd8ed6f04690a0bcc88a93fc4ae
    #[test]
    fn namehash_eth_matches_eip137() {
        let h = namehash_hex("eth").unwrap();
        assert_eq!(
            h,
            "0x93cdeb708b7545dc668eb9280176169d1c33cfd8ed6f04690a0bcc88a93fc4ae"
        );
    }

    /// Reference vector from EIP-137:
    /// namehash("foo.eth") =
    /// 0xde9b09fd7c5f901e23a3f19fecc54828e9c848539801e86591bd9801b019f84f
    #[test]
    fn namehash_foo_eth_matches_eip137() {
        let h = namehash_hex("foo.eth").unwrap();
        assert_eq!(
            h,
            "0xde9b09fd7c5f901e23a3f19fecc54828e9c848539801e86591bd9801b019f84f"
        );
    }

    #[test]
    fn namehash_lower_cases_input() {
        let lo = namehash_hex("FOO.ETH").unwrap();
        let canonical = namehash_hex("foo.eth").unwrap();
        assert_eq!(lo, canonical);
    }

    #[test]
    fn namehash_trims_whitespace() {
        let a = namehash_hex("  foo.eth  ").unwrap();
        let b = namehash_hex("foo.eth").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn looks_like_ens_basic() {
        assert!(looks_like_ens("foo.eth"));
        assert!(looks_like_ens("a.b.c"));
        assert!(looks_like_ens("alice-bob.eth"));
        assert!(!looks_like_ens("alice"));
        assert!(!looks_like_ens(""));
        assert!(!looks_like_ens("0xdeadbeef"));
    }

    #[test]
    fn normalise_rejects_empty() {
        let err = normalise("   ").unwrap_err();
        assert!(matches!(err, EnsError::InvalidInput(_)));
    }

    #[test]
    fn normalise_rejects_leading_dot() {
        let err = normalise(".eth").unwrap_err();
        assert!(matches!(err, EnsError::InvalidInput(_)));
    }

    #[test]
    fn normalise_rejects_trailing_dot() {
        let err = normalise("foo.").unwrap_err();
        assert!(matches!(err, EnsError::InvalidInput(_)));
    }

    #[test]
    fn normalise_rejects_double_dot() {
        let err = normalise("foo..eth").unwrap_err();
        assert!(matches!(err, EnsError::InvalidInput(_)));
    }

    #[test]
    fn normalise_rejects_leading_hyphen_label() {
        let err = normalise("-foo.eth").unwrap_err();
        assert!(matches!(err, EnsError::InvalidInput(_)));
    }

    #[test]
    fn normalise_rejects_trailing_hyphen_label() {
        let err = normalise("foo-.eth").unwrap_err();
        assert!(matches!(err, EnsError::InvalidInput(_)));
    }

    #[test]
    fn normalise_rejects_non_ascii() {
        let err = normalise("föö.eth").unwrap_err();
        assert!(matches!(err, EnsError::InvalidInput(_)));
    }

    #[test]
    fn normalise_rejects_label_too_long() {
        let label = "a".repeat(MAX_LABEL_LEN + 1);
        let err = normalise(&format!("{label}.eth")).unwrap_err();
        assert!(matches!(err, EnsError::InvalidInput(_)));
    }

    #[test]
    fn normalise_rejects_name_too_long() {
        let label = "a".repeat(60);
        let mut name = String::new();
        // 5 labels of 60 chars + 4 dots = 304 chars > 255.
        for i in 0..5 {
            if i > 0 {
                name.push('.');
            }
            name.push_str(&label);
        }
        let err = normalise(&name).unwrap_err();
        assert!(matches!(err, EnsError::InvalidInput(_)));
    }

    #[test]
    fn namehash_hex_is_lowercase_64chars() {
        let h = namehash_hex("foo.eth").unwrap();
        assert!(h.starts_with("0x"));
        assert_eq!(h.len(), 66);
        assert!(h
            .chars()
            .skip(2)
            .all(|c| c.is_ascii_hexdigit() && (!c.is_ascii_uppercase())));
    }
}
