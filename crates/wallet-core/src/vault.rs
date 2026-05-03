//! Encrypted vault format.
//!
//! On disk: a single binary blob with the following layout:
//!
//! ```text
//!   magic    : 8  bytes  ("ATLASV01")
//!   version  : 1  byte   (0x01)
//!   kdf      : 1  byte   (0x01 = Argon2id)
//!   salt     : 16 bytes
//!   nonce    : 12 bytes
//!   m_cost   : 4  bytes  (KiB,    big-endian u32)
//!   t_cost   : 4  bytes  (passes, big-endian u32)
//!   p_cost   : 4  bytes  (lanes,  big-endian u32)
//!   ct_len   : 4  bytes  (big-endian u32)
//!   ct       : ct_len bytes  (AES-256-GCM ciphertext)
//! ```
//!
//! The plaintext is the BIP-39 phrase (UTF-8). The phrase is enough to
//! recreate every derived key — we never store private keys directly.

use crate::error::{Error, Result};
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

const MAGIC: &[u8; 8] = b"ATLASV01";
const VERSION: u8 = 0x01;
const KDF_ARGON2ID: u8 = 0x01;

/// Argon2id parameters used to derive the AES-256 key.
///
/// Defaults are calibrated for ~0.5–1s on a desktop CPU as of 2026.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// Memory cost in KiB.
    pub m_cost: u32,
    /// Number of passes.
    pub t_cost: u32,
    /// Parallelism / lanes.
    pub p_cost: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self {
            m_cost: 64 * 1024, // 64 MiB
            t_cost: 3,
            p_cost: 4,
        }
    }
}

/// Public metadata about a vault (no secret content).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultHeader {
    /// Format version.
    pub version: u8,
    /// KDF parameters that were used to encrypt this vault.
    pub kdf: KdfParams,
    /// Salt used by Argon2id.
    pub salt: [u8; 16],
    /// AES-GCM nonce.
    pub nonce: [u8; 12],
}

/// An encrypted vault, ready to be persisted to disk or transmitted.
#[derive(Debug, Clone)]
pub struct EncryptedVault {
    /// Vault header (public).
    pub header: VaultHeader,
    /// AES-256-GCM ciphertext (includes the 16-byte authentication tag).
    pub ciphertext: Vec<u8>,
}

impl EncryptedVault {
    /// Encrypt a UTF-8 plaintext (the BIP-39 mnemonic) under the given password.
    pub fn encrypt(plaintext: &str, password: &str, kdf: KdfParams) -> Result<Self> {
        let mut salt = [0u8; 16];
        let mut nonce = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut salt);
        rand::thread_rng().fill_bytes(&mut nonce);

        let key = derive_key(password.as_bytes(), &salt, &kdf)?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_ref()));
        let ct = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: MAGIC,
                },
            )
            .map_err(|_| Error::Aead)?;

        Ok(Self {
            header: VaultHeader {
                version: VERSION,
                kdf,
                salt,
                nonce,
            },
            ciphertext: ct,
        })
    }

    /// Decrypt under the supplied password. Returns the recovered mnemonic
    /// in a [`Zeroizing`] buffer so it is wiped on drop.
    pub fn decrypt(&self, password: &str) -> Result<Zeroizing<String>> {
        let key = derive_key(password.as_bytes(), &self.header.salt, &self.header.kdf)?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_ref()));
        let pt = cipher
            .decrypt(
                Nonce::from_slice(&self.header.nonce),
                Payload {
                    msg: &self.ciphertext,
                    aad: MAGIC,
                },
            )
            .map_err(|_| Error::VaultDecryption)?;
        let s = String::from_utf8(pt).map_err(|_| Error::VaultDecryption)?;
        Ok(Zeroizing::new(s))
    }

    /// Serialize to the on-disk binary format.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + 1 + 1 + 16 + 12 + 12 + 4 + self.ciphertext.len());
        out.extend_from_slice(MAGIC);
        out.push(self.header.version);
        out.push(KDF_ARGON2ID);
        out.extend_from_slice(&self.header.salt);
        out.extend_from_slice(&self.header.nonce);
        out.extend_from_slice(&self.header.kdf.m_cost.to_be_bytes());
        out.extend_from_slice(&self.header.kdf.t_cost.to_be_bytes());
        out.extend_from_slice(&self.header.kdf.p_cost.to_be_bytes());
        out.extend_from_slice(&(self.ciphertext.len() as u32).to_be_bytes());
        out.extend_from_slice(&self.ciphertext);
        out
    }

    /// Deserialize from the on-disk binary format.
    pub fn from_bytes(buf: &[u8]) -> Result<Self> {
        if buf.len() < 8 + 1 + 1 + 16 + 12 + 12 + 4 {
            return Err(Error::VaultFormat("buffer too short"));
        }
        if &buf[..8] != MAGIC {
            return Err(Error::VaultFormat("bad magic"));
        }
        let version = buf[8];
        if version != VERSION {
            return Err(Error::VaultFormat("unsupported version"));
        }
        let kdf_id = buf[9];
        if kdf_id != KDF_ARGON2ID {
            return Err(Error::VaultFormat("unsupported KDF"));
        }
        let mut salt = [0u8; 16];
        salt.copy_from_slice(&buf[10..26]);
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&buf[26..38]);
        let m_cost = u32::from_be_bytes(buf[38..42].try_into().unwrap());
        let t_cost = u32::from_be_bytes(buf[42..46].try_into().unwrap());
        let p_cost = u32::from_be_bytes(buf[46..50].try_into().unwrap());
        let ct_len = u32::from_be_bytes(buf[50..54].try_into().unwrap()) as usize;
        if buf.len() < 54 + ct_len {
            return Err(Error::VaultFormat("truncated ciphertext"));
        }
        Ok(Self {
            header: VaultHeader {
                version,
                kdf: KdfParams {
                    m_cost,
                    t_cost,
                    p_cost,
                },
                salt,
                nonce,
            },
            ciphertext: buf[54..54 + ct_len].to_vec(),
        })
    }
}

fn derive_key(password: &[u8], salt: &[u8], kdf: &KdfParams) -> Result<Zeroizing<[u8; 32]>> {
    let params = Params::new(kdf.m_cost, kdf.t_cost, kdf.p_cost, Some(32))
        .map_err(|e| Error::Kdf(e.to_string()))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(password, salt, out.as_mut())
        .map_err(|e| Error::Kdf(e.to_string()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fast_kdf() -> KdfParams {
        // Tiny params for unit tests so they finish quickly.
        KdfParams {
            m_cost: 1024, // 1 MiB
            t_cost: 1,
            p_cost: 1,
        }
    }

    #[test]
    fn roundtrip() {
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let v = EncryptedVault::encrypt(phrase, "password123", fast_kdf()).unwrap();
        let decrypted = v.decrypt("password123").unwrap();
        assert_eq!(&*decrypted as &str, phrase);
    }

    #[test]
    fn wrong_password_fails() {
        let v = EncryptedVault::encrypt("hello", "right", fast_kdf()).unwrap();
        assert!(v.decrypt("wrong").is_err());
    }

    #[test]
    fn binary_format_roundtrip() {
        let v = EncryptedVault::encrypt("hello world", "pw", fast_kdf()).unwrap();
        let bytes = v.to_bytes();
        let parsed = EncryptedVault::from_bytes(&bytes).unwrap();
        let pt = parsed.decrypt("pw").unwrap();
        assert_eq!(&*pt as &str, "hello world");
    }
}
