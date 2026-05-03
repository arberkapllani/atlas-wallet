//! Persistent user settings for Atlas.
//!
//! Atlas is sovereignty-first: every chain provider must let the user point at
//! their own RPC node (or any endpoint they trust). This crate persists those
//! overrides in `app_data/settings.json` and exposes a small synchronous API
//! consumed by the desktop shell.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// File name (within the app data dir) used to persist settings.
pub const FILE_NAME: &str = "settings.json";

/// User-selectable display currency for fiat values across the UI.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FiatCurrency {
    /// US dollar (default).
    #[default]
    Usd,
    /// Euro.
    Eur,
    /// British pound.
    Gbp,
}

impl FiatCurrency {
    /// Lowercase ticker matching CoinGecko's `vs_currencies` parameter.
    pub fn as_str(self) -> &'static str {
        match self {
            FiatCurrency::Usd => "usd",
            FiatCurrency::Eur => "eur",
            FiatCurrency::Gbp => "gbp",
        }
    }

    /// Parse a case-insensitive ticker (`"usd"` / `"eur"` / `"gbp"`).
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "usd" => Some(FiatCurrency::Usd),
            "eur" => Some(FiatCurrency::Eur),
            "gbp" => Some(FiatCurrency::Gbp),
            _ => None,
        }
    }
}

/// Errors emitted by the settings layer.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    /// Filesystem I/O failed.
    #[error("settings io: {0}")]
    Io(#[from] std::io::Error),
    /// JSON could not be (de)serialised.
    #[error("settings json: {0}")]
    Json(#[from] serde_json::Error),
}

/// On-disk shape. Stable: every field is `#[serde(default)]` so older files
/// keep loading after we add new keys.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsData {
    /// `chain_id -> user-provided RPC/REST URL`. Absence means "use default".
    pub rpc_overrides: BTreeMap<String, String>,
    /// Display currency for fiat values across the UI.
    pub fiat_currency: FiatCurrency,
}

/// Thread-safe handle to the persisted settings.
pub struct Settings {
    path: PathBuf,
    inner: RwLock<SettingsData>,
}

impl Settings {
    /// Load `settings.json` from `data_dir`, or create an empty record on
    /// first run.
    pub fn load_or_init(data_dir: impl AsRef<Path>) -> Result<Self, SettingsError> {
        let dir = data_dir.as_ref();
        std::fs::create_dir_all(dir)?;
        let path = dir.join(FILE_NAME);
        let data: SettingsData = if path.exists() {
            let bytes = std::fs::read(&path)?;
            // Treat a corrupt/empty file as a fresh state rather than aborting:
            // the user can re-enter overrides, but we never lose their wallet.
            serde_json::from_slice(&bytes).unwrap_or_default()
        } else {
            SettingsData::default()
        };
        Ok(Self {
            path,
            inner: RwLock::new(data),
        })
    }

    /// Snapshot of the current settings.
    pub fn snapshot(&self) -> SettingsData {
        self.inner.read().expect("settings poisoned").clone()
    }

    /// Effective override URL for `chain_id`, if any.
    pub fn rpc_override(&self, chain_id: &str) -> Option<String> {
        self.inner
            .read()
            .expect("settings poisoned")
            .rpc_overrides
            .get(chain_id)
            .cloned()
    }

    /// Set or replace the RPC override for `chain_id`. Empty/whitespace URLs
    /// are rejected.
    pub fn set_rpc_override(&self, chain_id: &str, url: &str) -> Result<(), SettingsError> {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return Err(SettingsError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "rpc url must not be empty",
            )));
        }
        {
            let mut g = self.inner.write().expect("settings poisoned");
            g.rpc_overrides
                .insert(chain_id.to_string(), trimmed.to_string());
        }
        self.persist()
    }

    /// Remove any user override for `chain_id`, falling back to the built-in
    /// default the next time providers are constructed.
    pub fn clear_rpc_override(&self, chain_id: &str) -> Result<(), SettingsError> {
        {
            let mut g = self.inner.write().expect("settings poisoned");
            g.rpc_overrides.remove(chain_id);
        }
        self.persist()
    }

    /// Currently selected fiat display currency.
    pub fn fiat_currency(&self) -> FiatCurrency {
        self.inner.read().expect("settings poisoned").fiat_currency
    }

    /// Persist a new fiat display currency.
    pub fn set_fiat_currency(&self, currency: FiatCurrency) -> Result<(), SettingsError> {
        {
            let mut g = self.inner.write().expect("settings poisoned");
            g.fiat_currency = currency;
        }
        self.persist()
    }

    fn persist(&self) -> Result<(), SettingsError> {
        let snapshot = self.snapshot();
        let json = serde_json::to_vec_pretty(&snapshot)?;
        // Atomic write: same pattern profile registry uses.
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_persists_overrides() {
        let dir = tempfile::tempdir().unwrap();
        {
            let s = Settings::load_or_init(dir.path()).unwrap();
            s.set_rpc_override("eth", "http://127.0.0.1:8545").unwrap();
            s.set_rpc_override("btc", "http://my-node.local/api")
                .unwrap();
        }
        let s = Settings::load_or_init(dir.path()).unwrap();
        assert_eq!(
            s.rpc_override("eth").as_deref(),
            Some("http://127.0.0.1:8545")
        );
        assert_eq!(
            s.rpc_override("btc").as_deref(),
            Some("http://my-node.local/api")
        );
        assert!(s.rpc_override("sol").is_none());
    }

    #[test]
    fn clear_removes_override() {
        let dir = tempfile::tempdir().unwrap();
        let s = Settings::load_or_init(dir.path()).unwrap();
        s.set_rpc_override("eth", "http://x").unwrap();
        s.clear_rpc_override("eth").unwrap();
        assert!(s.rpc_override("eth").is_none());
    }

    #[test]
    fn empty_url_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let s = Settings::load_or_init(dir.path()).unwrap();
        assert!(s.set_rpc_override("eth", "   ").is_err());
    }

    #[test]
    fn corrupt_file_resets_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), b"{not json").unwrap();
        let s = Settings::load_or_init(dir.path()).unwrap();
        assert!(s.snapshot().rpc_overrides.is_empty());
    }

    #[test]
    fn fiat_currency_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        {
            let s = Settings::load_or_init(dir.path()).unwrap();
            assert_eq!(s.fiat_currency(), FiatCurrency::Usd);
            s.set_fiat_currency(FiatCurrency::Eur).unwrap();
        }
        let s = Settings::load_or_init(dir.path()).unwrap();
        assert_eq!(s.fiat_currency(), FiatCurrency::Eur);
    }

    #[test]
    fn fiat_currency_parses_case_insensitive() {
        assert_eq!(FiatCurrency::parse("USD"), Some(FiatCurrency::Usd));
        assert_eq!(FiatCurrency::parse("eur"), Some(FiatCurrency::Eur));
        assert_eq!(FiatCurrency::parse("Gbp"), Some(FiatCurrency::Gbp));
        assert_eq!(FiatCurrency::parse("jpy"), None);
    }
}
