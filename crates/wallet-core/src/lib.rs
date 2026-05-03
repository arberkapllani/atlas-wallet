//! `atlas-wallet-core` — sovereign key management for Atlas.
//!
//! Responsibilities:
//! * Mnemonic generation/import (BIP-39, 12 or 24 words)
//! * Hierarchical deterministic key derivation (BIP-32 / BIP-44 / BIP-84)
//! * Vault: encrypt/decrypt the seed at rest with Argon2id + AES-256-GCM
//! * Strict zeroization of all secret material on drop
//!
//! Nothing in this crate touches the network or the filesystem directly —
//! callers handle persistence so the crate stays auditable in isolation.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod derive;
pub mod error;
pub mod mnemonic;
pub mod vault;

pub use derive::{derive_account, AccountKey, ChainKind};
pub use error::{Error, Result};
pub use mnemonic::{Mnemonic, MnemonicLength};
pub use vault::{EncryptedVault, KdfParams, VaultHeader};

