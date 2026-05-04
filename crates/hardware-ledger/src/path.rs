//! BIP-32 derivation path encoding for Ledger commands.
//!
//! Ledger expects the path as: 1-byte component count, then each
//! component as 4 big-endian bytes. Hardened components have the
//! high bit (0x80000000) set.

use thiserror::Error;

/// Hardening offset for BIP-32 components.
pub const HARDENED: u32 = 0x8000_0000;

/// A parsed BIP-32 path. Bounded to 10 components (Ledger's
/// maximum).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bip32Path(Vec<u32>);

impl Bip32Path {
    /// Parse a path string like `"m/44'/60'/0'/0/0"`. Apostrophe
    /// or `h` mark hardened components; an explicit `m/` prefix is
    /// optional.
    pub fn parse(input: &str) -> Result<Self, PathError> {
        let trimmed = input.trim();
        let body = trimmed.strip_prefix("m/").unwrap_or(trimmed);
        if body.is_empty() {
            return Ok(Self(Vec::new()));
        }
        let mut parts = Vec::new();
        for part in body.split('/') {
            let (num_str, hardened) =
                if let Some(rest) = part.strip_suffix('\'').or_else(|| part.strip_suffix('h')) {
                    (rest, true)
                } else {
                    (part, false)
                };
            let n: u32 = num_str
                .parse()
                .map_err(|_| PathError::InvalidComponent(part.to_string()))?;
            if n >= HARDENED {
                return Err(PathError::ComponentOutOfRange(part.to_string()));
            }
            parts.push(if hardened { n | HARDENED } else { n });
        }
        if parts.len() > 10 {
            return Err(PathError::TooManyComponents(parts.len()));
        }
        Ok(Self(parts))
    }

    /// Borrow components.
    pub fn as_slice(&self) -> &[u32] {
        &self.0
    }

    /// Encode in Ledger's wire format: count byte + N × big-endian
    /// u32.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(1 + 4 * self.0.len());
        buf.push(self.0.len() as u8);
        for c in &self.0 {
            buf.extend_from_slice(&c.to_be_bytes());
        }
        buf
    }
}

#[derive(Debug, Error)]
pub enum PathError {
    #[error("invalid path component: {0}")]
    InvalidComponent(String),
    #[error("path component {0} is out of u31 range")]
    ComponentOutOfRange(String),
    #[error("too many path components ({0}); Ledger allows max 10")]
    TooManyComponents(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_eth_default_path() {
        let p = Bip32Path::parse("m/44'/60'/0'/0/0").unwrap();
        assert_eq!(
            p.as_slice(),
            &[44 | HARDENED, 60 | HARDENED, HARDENED, 0, 0,]
        );
    }

    #[test]
    fn accepts_h_suffix_for_hardened() {
        let p = Bip32Path::parse("44h/60h/0h/0/0").unwrap();
        let q = Bip32Path::parse("44'/60'/0'/0/0").unwrap();
        assert_eq!(p, q);
    }

    #[test]
    fn encodes_count_byte_and_big_endian() {
        let p = Bip32Path::parse("m/44'/60'/0'/0/0").unwrap();
        let bytes = p.encode();
        assert_eq!(bytes[0], 5);
        // 0x8000002C = 44'
        assert_eq!(&bytes[1..5], &[0x80, 0x00, 0x00, 0x2C]);
        // last component is plain 0
        assert_eq!(&bytes[17..21], &[0x00, 0x00, 0x00, 0x00]);
        assert_eq!(bytes.len(), 1 + 5 * 4);
    }

    #[test]
    fn rejects_garbage() {
        assert!(Bip32Path::parse("m/44'/abc").is_err());
    }

    #[test]
    fn empty_path_is_legal() {
        let p = Bip32Path::parse("m/").unwrap();
        assert_eq!(p.as_slice(), &[] as &[u32]);
        assert_eq!(p.encode(), vec![0u8]);
    }
}
