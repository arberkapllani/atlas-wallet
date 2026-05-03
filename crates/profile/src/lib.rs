//! Profile (multi-wallet) registry for Atlas.
//!
//! A *profile* is a single named wallet identity. Two kinds:
//! - [`ProfileKind::Hot`] — encrypted seed vault on disk (signing capable).
//! - [`ProfileKind::WatchOnly`] — list of `(chain_id, address)` pairs only,
//!   no private keys, no signing capability.
//!
//! The registry lives at `data_dir/profiles.json`. Vault blobs live at
//! `data_dir/vaults/<uuid>.bin` and watch-only descriptors at
//! `data_dir/watch/<uuid>.json`. The registry tracks the currently active
//! profile id.
//!
//! Backwards compatibility: if a legacy `data_dir/vault.bin` exists when the
//! registry is first loaded, [`ProfileRegistry::load_or_init`] migrates it
//! into a profile named "Default".

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

mod error;
mod registry;
mod types;

pub use error::{ProfileError, ProfileResult};
pub use registry::ProfileRegistry;
pub use types::{Profile, ProfileKind, ProfileSummary, WatchAccount};

