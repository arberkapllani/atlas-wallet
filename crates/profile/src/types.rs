//! Profile data types.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// A `(chain_id, address)` pair tracked by a watch-only profile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
pub struct WatchAccount {
    /// Chain identifier (e.g. `"btc"`, `"eth"`, `"polygon"`).
    pub chain_id: String,
    /// Public address (no private key material).
    pub address: String,
    /// Optional human label.
    #[serde(default)]
    pub label: Option<String>,
}

/// What kind of profile this is.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileKind {
    /// Hot wallet — encrypted seed lives at `vault_path`.
    Hot {
        /// Path to the encrypted vault blob (relative to `data_dir`).
        #[specta(type = String)]
        vault_file: PathBuf,
    },
    /// Watch-only wallet — list of public addresses only.
    WatchOnly {
        /// Public addresses being watched.
        accounts: Vec<WatchAccount>,
    },
}

impl ProfileKind {
    /// `true` if this profile can sign transactions.
    pub fn is_signing_capable(&self) -> bool {
        matches!(self, ProfileKind::Hot { .. })
    }
}

/// A single named profile in the registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    /// Stable opaque identifier.
    pub id: Uuid,
    /// Human-visible label. Unique within a registry.
    pub name: String,
    /// Whether this is hot or watch-only.
    pub kind: ProfileKind,
    /// Creation timestamp (RFC3339 string).
    pub created_at: String,
}

/// Frontend-friendly summary of a profile.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct ProfileSummary {
    /// Stable id (as string for JS).
    pub id: String,
    /// Human-visible label.
    pub name: String,
    /// `"hot"` or `"watch_only"`.
    pub kind: &'static str,
    /// Whether this profile can sign transactions.
    pub signing: bool,
    /// Creation timestamp.
    pub created_at: String,
    /// Number of watched accounts (0 for hot profiles).
    pub watch_account_count: usize,
}

impl From<&Profile> for ProfileSummary {
    fn from(p: &Profile) -> Self {
        let (kind, watch_account_count) = match &p.kind {
            ProfileKind::Hot { .. } => ("hot", 0),
            ProfileKind::WatchOnly { accounts } => ("watch_only", accounts.len()),
        };
        Self {
            id: p.id.to_string(),
            name: p.name.clone(),
            kind,
            signing: p.kind.is_signing_capable(),
            created_at: p.created_at.clone(),
            watch_account_count,
        }
    }
}
