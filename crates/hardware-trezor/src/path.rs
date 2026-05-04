//! BIP-32 path parser. Trezor encodes paths as a `repeated uint32`
//! protobuf field; this module produces the unwrapped `Vec<u32>`
//! and the encoder lives in [`crate::messages`].
//!
//! The parser logic mirrors the Ledger path module; we keep them
//! independent so the two hardware crates can evolve separately.

use thiserror::Error;

pub const HARDENED: u32 = 0x8000_0000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bip32Path(Vec<u32>);

impl Bip32Path {
    /// Parse `m/44'/60'/0'/0/0` etc.
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

    pub fn as_slice(&self) -> &[u32] {
        &self.0
    }
}

#[derive(Debug, Error)]
pub enum PathError {
    #[error("invalid path component: {0}")]
    InvalidComponent(String),
    #[error("path component {0} is out of u31 range")]
    ComponentOutOfRange(String),
    #[error("too many path components ({0}); max 10")]
    TooManyComponents(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_eth_path() {
        let p = Bip32Path::parse("m/44'/60'/0'/0/0").unwrap();
        assert_eq!(
            p.as_slice(),
            &[44 | HARDENED, 60 | HARDENED, HARDENED, 0, 0]
        );
    }

    #[test]
    fn parses_btc_native_segwit_path() {
        let p = Bip32Path::parse("m/84h/0h/0h/0/0").unwrap();
        assert_eq!(p.as_slice(), &[84 | HARDENED, HARDENED, HARDENED, 0, 0]);
    }

    #[test]
    fn rejects_garbage() {
        assert!(Bip32Path::parse("m/oops").is_err());
    }
}
