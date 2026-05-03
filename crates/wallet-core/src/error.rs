//! Strongly-typed errors for the wallet core.

use thiserror::Error;

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// All failure modes exposed by `exodus2-wallet-core`.
#[derive(Debug, Error)]
pub enum Error {
    /// The mnemonic phrase failed BIP-39 checksum validation.
    #[error("invalid mnemonic: {0}")]
    InvalidMnemonic(String),

    /// The requested word count is not 12 or 24.
    #[error("invalid mnemonic length: must be 12 or 24 words")]
    InvalidMnemonicLength,

    /// BIP-32 derivation failed.
    #[error("derivation error: {0}")]
    Derivation(String),

    /// Vault decryption failed (wrong password or tampered ciphertext).
    #[error("vault decryption failed (wrong password or corrupted data)")]
    VaultDecryption,

    /// AEAD/AES-GCM internal failure.
    #[error("aead error")]
    Aead,

    /// Argon2id KDF failure.
    #[error("kdf error: {0}")]
    Kdf(String),

    /// Vault binary format is not understood.
    #[error("vault format: {0}")]
    VaultFormat(&'static str),

    /// (De)serialization error.
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),

    /// Hex decoding error.
    #[error("hex: {0}")]
    Hex(#[from] hex::FromHexError),

    /// I/O error from a low-level primitive (e.g. RNG).
    #[error("rng: {0}")]
    Rng(#[from] getrandom::Error),
}
