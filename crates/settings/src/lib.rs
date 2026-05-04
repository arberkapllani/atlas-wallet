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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsData {
    /// `chain_id -> user-provided RPC/REST URL`. Absence means "use default".
    pub rpc_overrides: BTreeMap<String, String>,
    /// Display currency for fiat values across the UI.
    pub fiat_currency: FiatCurrency,
    /// Optional 1inch developer API key (Bearer token). Absent = unauthenticated.
    pub oneinch_api_key: Option<String>,
    /// User-overridable 1inch base URL. Absent = built-in default.
    pub oneinch_base_url: Option<String>,
    /// Auto-lock the wallet after this many minutes of UI inactivity.
    /// `0` disables auto-lock. Absent (legacy file) → default 5 minutes.
    #[serde(default = "default_auto_lock_minutes")]
    pub auto_lock_minutes: u32,
    /// User-chosen anti-phishing phrase. Surfaced on every Atlas
    /// unlock screen so a phishing UI without this phrase is
    /// immediately recognisable. Absent = feature disabled.
    #[serde(default)]
    pub anti_phishing_phrase: Option<String>,
    /// `true` if the user has opted into biometric (Windows Hello
    /// / Touch ID) confirmation when re-unlocking within the
    /// auto-lock window. Defaults to `false`. Atlas never replaces
    /// the master password — biometrics only gate access to a
    /// *previously-unlocked* seed cached in memory.
    #[serde(default)]
    pub biometric_unlock_enabled: bool,
    /// Unix timestamp (seconds) of the last successful recovery
    /// drill (user verified they still have their seed phrase).
    /// Absent = never performed.
    #[serde(default)]
    pub last_recovery_drill_at: Option<i64>,
    /// How often Atlas should remind the user to verify their
    /// seed. `0` disables the reminder. Default 90 days.
    #[serde(default = "default_recovery_drill_interval_days")]
    pub recovery_drill_interval_days: u32,
}

fn default_recovery_drill_interval_days() -> u32 {
    90
}

fn default_auto_lock_minutes() -> u32 {
    5
}

impl Default for SettingsData {
    fn default() -> Self {
        Self {
            rpc_overrides: BTreeMap::new(),
            fiat_currency: FiatCurrency::default(),
            oneinch_api_key: None,
            oneinch_base_url: None,
            auto_lock_minutes: default_auto_lock_minutes(),
            anti_phishing_phrase: None,
            biometric_unlock_enabled: false,
            last_recovery_drill_at: None,
            recovery_drill_interval_days: default_recovery_drill_interval_days(),
        }
    }
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

    /// Currently configured 1inch API key, if any.
    pub fn oneinch_api_key(&self) -> Option<String> {
        self.inner
            .read()
            .expect("settings poisoned")
            .oneinch_api_key
            .clone()
    }

    /// Currently configured 1inch base URL (or the built-in default).
    pub fn oneinch_base_url(&self) -> String {
        self.inner
            .read()
            .expect("settings poisoned")
            .oneinch_base_url
            .clone()
            .unwrap_or_else(|| "https://api.1inch.dev".to_string())
    }

    /// Persist (or clear) the 1inch API key. Empty/whitespace clears it.
    pub fn set_oneinch_api_key(&self, key: Option<&str>) -> Result<(), SettingsError> {
        {
            let mut g = self.inner.write().expect("settings poisoned");
            g.oneinch_api_key = match key {
                Some(k) if !k.trim().is_empty() => Some(k.trim().to_string()),
                _ => None,
            };
        }
        self.persist()
    }

    /// Auto-lock timeout in minutes. `0` means disabled.
    pub fn auto_lock_minutes(&self) -> u32 {
        self.inner
            .read()
            .expect("settings poisoned")
            .auto_lock_minutes
    }

    /// Persist a new auto-lock timeout (minutes). `0` disables.
    pub fn set_auto_lock_minutes(&self, minutes: u32) -> Result<(), SettingsError> {
        {
            let mut g = self.inner.write().expect("settings poisoned");
            g.auto_lock_minutes = minutes;
        }
        self.persist()
    }

    /// Currently configured anti-phishing phrase, if any. The phrase
    /// is plaintext on purpose: Atlas surfaces it on every unlock
    /// screen so a phishing UI that *doesn't* know it is instantly
    /// distinguishable. Treat the value as low-sensitivity (no key
    /// material derives from it).
    pub fn anti_phishing_phrase(&self) -> Option<String> {
        self.inner
            .read()
            .expect("settings poisoned")
            .anti_phishing_phrase
            .clone()
    }

    /// Set or clear the anti-phishing phrase. Empty / whitespace
    /// clears it. The phrase is trimmed and capped at 200 chars to
    /// keep the unlock screen readable.
    pub fn set_anti_phishing_phrase(&self, phrase: Option<&str>) -> Result<(), SettingsError> {
        let normalised = match phrase {
            Some(p) => {
                let trimmed = p.trim();
                if trimmed.is_empty() {
                    None
                } else if trimmed.chars().count() > 200 {
                    return Err(SettingsError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "anti-phishing phrase must be 200 characters or fewer",
                    )));
                } else {
                    Some(trimmed.to_string())
                }
            }
            None => None,
        };
        {
            let mut g = self.inner.write().expect("settings poisoned");
            g.anti_phishing_phrase = normalised;
        }
        self.persist()
    }

    /// `true` if biometric unlock has been opted in. The actual
    /// availability of the platform sensor is checked separately
    /// at unlock time — this flag only stores the user preference.
    pub fn biometric_unlock_enabled(&self) -> bool {
        self.inner
            .read()
            .expect("settings poisoned")
            .biometric_unlock_enabled
    }

    /// Persist the user's biometric-unlock preference.
    pub fn set_biometric_unlock_enabled(&self, enabled: bool) -> Result<(), SettingsError> {
        {
            let mut g = self.inner.write().expect("settings poisoned");
            g.biometric_unlock_enabled = enabled;
        }
        self.persist()
    }

    /// Unix timestamp (seconds) of the last successful recovery
    /// drill, or `None` if the user has never performed one.
    pub fn last_recovery_drill_at(&self) -> Option<i64> {
        self.inner
            .read()
            .expect("settings poisoned")
            .last_recovery_drill_at
    }

    /// Recovery-drill reminder interval (days). `0` disables.
    pub fn recovery_drill_interval_days(&self) -> u32 {
        self.inner
            .read()
            .expect("settings poisoned")
            .recovery_drill_interval_days
    }

    /// Set the recovery-drill reminder interval. `0` disables.
    /// Values above 365 days are clamped to keep the prompt
    /// meaningful (a once-a-year drill is the floor).
    pub fn set_recovery_drill_interval_days(&self, days: u32) -> Result<(), SettingsError> {
        let clamped = days.min(365);
        {
            let mut g = self.inner.write().expect("settings poisoned");
            g.recovery_drill_interval_days = clamped;
        }
        self.persist()
    }

    /// Record that the user just verified their seed. Caller is
    /// responsible for actually checking the user typed the
    /// phrase correctly — this just stamps the success.
    pub fn mark_recovery_drill_completed(&self, at_unix_seconds: i64) -> Result<(), SettingsError> {
        {
            let mut g = self.inner.write().expect("settings poisoned");
            g.last_recovery_drill_at = Some(at_unix_seconds);
        }
        self.persist()
    }

    /// `true` if a drill is currently due. A drill is due when the
    /// interval is non-zero AND
    /// (`last_recovery_drill_at` is None OR
    ///  `now - last_recovery_drill_at >= interval_days * 86400`).
    pub fn recovery_drill_is_due(&self, now_unix_seconds: i64) -> bool {
        let g = self.inner.read().expect("settings poisoned");
        if g.recovery_drill_interval_days == 0 {
            return false;
        }
        let interval_secs = i64::from(g.recovery_drill_interval_days) * 86_400;
        match g.last_recovery_drill_at {
            None => true,
            Some(last) => now_unix_seconds.saturating_sub(last) >= interval_secs,
        }
    }

    /// Persist (or clear) the 1inch base URL. Empty/whitespace clears it.
    pub fn set_oneinch_base_url(&self, url: Option<&str>) -> Result<(), SettingsError> {
        {
            let mut g = self.inner.write().expect("settings poisoned");
            g.oneinch_base_url = match url {
                Some(u) if !u.trim().is_empty() => Some(u.trim().to_string()),
                _ => None,
            };
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

    #[test]
    fn anti_phishing_phrase_round_trips_and_trims() {
        let dir = tempfile::tempdir().unwrap();
        {
            let s = Settings::load_or_init(dir.path()).unwrap();
            assert_eq!(s.anti_phishing_phrase(), None);
            s.set_anti_phishing_phrase(Some("  blue heron at dusk  "))
                .unwrap();
        }
        let s = Settings::load_or_init(dir.path()).unwrap();
        assert_eq!(
            s.anti_phishing_phrase().as_deref(),
            Some("blue heron at dusk")
        );
        // Empty / whitespace clears.
        s.set_anti_phishing_phrase(Some("   ")).unwrap();
        assert_eq!(s.anti_phishing_phrase(), None);
        // Length limit enforced.
        let too_long = "x".repeat(201);
        assert!(s.set_anti_phishing_phrase(Some(&too_long)).is_err());
    }

    #[test]
    fn recovery_drill_due_initially_then_marked_completed() {
        let dir = tempfile::tempdir().unwrap();
        let s = Settings::load_or_init(dir.path()).unwrap();
        // Default 90-day interval, never performed → due.
        let now = 1_700_000_000_i64;
        assert!(s.recovery_drill_is_due(now));
        s.mark_recovery_drill_completed(now).unwrap();
        // Just completed → not due.
        assert!(!s.recovery_drill_is_due(now + 60));
        // 91 days later → due again.
        assert!(s.recovery_drill_is_due(now + 91 * 86_400));
        // Disabling the interval suppresses the reminder.
        s.set_recovery_drill_interval_days(0).unwrap();
        assert!(!s.recovery_drill_is_due(now + 365 * 86_400));
    }
}
