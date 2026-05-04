//! In-app dApp browser registry & origin trust gating.
//!
//! The embedded webview cannot be the sole gatekeeper for what the
//! wallet ultimately signs, so this crate provides a small,
//! deterministic policy layer:
//!
//! - `DappEntry` — curated metadata for a known dApp (name, category,
//!   homepage, optional risk flags).
//! - `DappRegistry::lookup_origin` — strict origin (scheme + host +
//!   port) match against the registry. HTTPS-only by default; plain
//!   HTTP is only ever allowed for `localhost` / `127.0.0.1` so dev
//!   tooling still works.
//! - `DappRegistry::risk_assessment` — returns one of `Verified`,
//!   `Unknown`, or `Blocked`, plus a list of warnings (mixed-content,
//!   non-HTTPS, blocklisted, etc.).
//!
//! The registry itself is a pure value (no I/O, no network), making it
//! trivial to seed from a remote tokenlist-style JSON later or to swap
//! in a fixture for tests. The URL normaliser lives here so the same
//! origin shape is enforced from every callsite (Tauri command, WC
//! peer metadata, deep-link handler).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum DappError {
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("unsupported scheme: {0}")]
    UnsupportedScheme(String),
    #[error("missing host")]
    MissingHost,
}

/// Risk verdict returned to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum DappRisk {
    /// Origin matches a curated, audited entry.
    Verified,
    /// Origin is unknown to the registry — proceed with caution.
    Unknown,
    /// Origin is on the blocklist (phishing / known scam).
    Blocked,
}

/// Category tag used purely for UI grouping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum DappCategory {
    Defi,
    Nft,
    Bridge,
    Game,
    Social,
    Other,
}

/// One curated dApp.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DappEntry {
    pub name: String,
    /// Canonical homepage origin, e.g. `https://app.uniswap.org`.
    /// Stored already-normalised (no trailing slash, lowercased host).
    pub origin: String,
    pub category: DappCategory,
    /// Optional comma-separated tags ("amm", "lending", ...).
    pub tags: Vec<String>,
}

/// Outcome of evaluating a candidate URL.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DappAssessment {
    /// The normalised origin (`scheme://host[:port]`) that was checked.
    pub origin: String,
    pub risk: DappRisk,
    /// Matched curated entry, if any.
    pub entry: Option<DappEntry>,
    /// Human-readable warnings shown in the consent UI.
    pub warnings: Vec<String>,
    /// Stable per-origin fingerprint useful as a cache key in the UI
    /// (first 16 hex chars of `sha256(origin)`).
    pub fingerprint: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct DappRegistry {
    pub entries: Vec<DappEntry>,
    /// Origins that should always be flagged as `Blocked`. Stored
    /// already-normalised.
    pub blocklist: Vec<String>,
}

impl DappRegistry {
    /// Build with the small built-in seed list.
    pub fn with_defaults() -> Self {
        Self {
            entries: default_entries(),
            blocklist: Vec::new(),
        }
    }

    /// Evaluate a URL string. Always returns an assessment — invalid
    /// inputs surface as `Err` rather than as a fake `Unknown`.
    pub fn assess(&self, candidate: &str) -> Result<DappAssessment, DappError> {
        let origin = normalise_origin(candidate)?;
        let mut warnings: Vec<String> = Vec::new();

        if origin.starts_with("http://") && !is_local_origin(&origin) {
            warnings.push(
                "Connection is not encrypted (plain HTTP). Do not approve transactions.".into(),
            );
        }

        let blocked = self.blocklist.iter().any(|o| eq_origin(o, &origin));
        let entry = self.entries.iter().find(|e| eq_origin(&e.origin, &origin));

        let risk = if blocked {
            warnings.insert(0, "This origin is on the Atlas blocklist.".into());
            DappRisk::Blocked
        } else if entry.is_some() {
            DappRisk::Verified
        } else {
            warnings.push(
                "Origin is not in Atlas's curated registry. Verify the URL before signing.".into(),
            );
            DappRisk::Unknown
        };

        Ok(DappAssessment {
            origin: origin.clone(),
            risk,
            entry: entry.cloned(),
            warnings,
            fingerprint: fingerprint(&origin),
        })
    }
}

/// Normalise a URL down to `scheme://host[:port]`, lowercased host,
/// no trailing slash, no path/query/fragment.
pub fn normalise_origin(input: &str) -> Result<String, DappError> {
    let parsed = Url::parse(input).map_err(|e| DappError::InvalidUrl(e.to_string()))?;
    let scheme = parsed.scheme();
    if scheme != "https" && scheme != "http" {
        return Err(DappError::UnsupportedScheme(scheme.to_string()));
    }
    let host = parsed.host_str().ok_or(DappError::MissingHost)?;
    let host_l = host.to_ascii_lowercase();
    let mut origin = format!("{scheme}://{host_l}");
    // Only include the port if it's non-default for the scheme.
    if let Some(port) = parsed.port() {
        let default =
            matches!(scheme, "https" if port == 443) || matches!(scheme, "http" if port == 80);
        if !default {
            origin.push_str(&format!(":{port}"));
        }
    }
    Ok(origin)
}

fn eq_origin(a: &str, b: &str) -> bool {
    // Both sides should be normalised, but be defensive.
    a.eq_ignore_ascii_case(b)
}

fn is_local_origin(origin: &str) -> bool {
    origin.contains("//localhost") || origin.contains("//127.0.0.1") || origin.contains("//[::1]")
}

fn fingerprint(origin: &str) -> String {
    let mut h = Sha256::new();
    h.update(origin.as_bytes());
    let digest = h.finalize();
    hex::encode(&digest[..8])
}

fn default_entries() -> Vec<DappEntry> {
    fn e(name: &str, origin: &str, category: DappCategory, tags: &[&str]) -> DappEntry {
        DappEntry {
            name: name.to_string(),
            origin: origin.to_string(),
            category,
            tags: tags.iter().map(|s| s.to_string()).collect(),
        }
    }
    vec![
        e(
            "Uniswap",
            "https://app.uniswap.org",
            DappCategory::Defi,
            &["amm", "ethereum"],
        ),
        e(
            "Aave",
            "https://app.aave.com",
            DappCategory::Defi,
            &["lending"],
        ),
        e(
            "Curve",
            "https://curve.fi",
            DappCategory::Defi,
            &["stableswap"],
        ),
        e(
            "1inch",
            "https://app.1inch.io",
            DappCategory::Defi,
            &["aggregator"],
        ),
        e(
            "Lido",
            "https://stake.lido.fi",
            DappCategory::Defi,
            &["staking", "ethereum"],
        ),
        e(
            "OpenSea",
            "https://opensea.io",
            DappCategory::Nft,
            &["marketplace"],
        ),
        e(
            "Blur",
            "https://blur.io",
            DappCategory::Nft,
            &["marketplace"],
        ),
        e(
            "Across",
            "https://app.across.to",
            DappCategory::Bridge,
            &["rollup-bridge"],
        ),
        e(
            "Stargate",
            "https://stargate.finance",
            DappCategory::Bridge,
            &["layerzero"],
        ),
        e(
            "Jupiter",
            "https://jup.ag",
            DappCategory::Defi,
            &["solana", "aggregator"],
        ),
        e(
            "Raydium",
            "https://raydium.io",
            DappCategory::Defi,
            &["solana", "amm"],
        ),
        e(
            "Magic Eden",
            "https://magiceden.io",
            DappCategory::Nft,
            &["solana", "marketplace"],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_origin_strips_path_and_lowercases_host() {
        assert_eq!(
            normalise_origin("https://App.Uniswap.org/swap?x=1#frag").unwrap(),
            "https://app.uniswap.org"
        );
    }

    #[test]
    fn normalises_keeps_non_default_port() {
        assert_eq!(
            normalise_origin("https://example.com:8443/").unwrap(),
            "https://example.com:8443"
        );
    }

    #[test]
    fn normalises_drops_default_https_port() {
        assert_eq!(
            normalise_origin("https://example.com:443/foo").unwrap(),
            "https://example.com"
        );
    }

    #[test]
    fn normalise_rejects_non_http_scheme() {
        let err = normalise_origin("file:///etc/passwd").unwrap_err();
        assert!(matches!(err, DappError::UnsupportedScheme(_)));
    }

    #[test]
    fn normalise_rejects_garbage() {
        assert!(matches!(
            normalise_origin("not a url").unwrap_err(),
            DappError::InvalidUrl(_)
        ));
    }

    #[test]
    fn assess_known_origin_is_verified_with_no_warnings() {
        let reg = DappRegistry::with_defaults();
        let a = reg.assess("https://app.uniswap.org/swap").unwrap();
        assert_eq!(a.risk, DappRisk::Verified);
        assert_eq!(a.entry.as_ref().unwrap().name, "Uniswap");
        assert!(a.warnings.is_empty());
    }

    #[test]
    fn assess_unknown_origin_warns_and_is_unknown() {
        let reg = DappRegistry::with_defaults();
        let a = reg.assess("https://random-defi.example/").unwrap();
        assert_eq!(a.risk, DappRisk::Unknown);
        assert!(a.entry.is_none());
        assert_eq!(a.warnings.len(), 1);
    }

    #[test]
    fn assess_blocklisted_origin_is_blocked() {
        let mut reg = DappRegistry::with_defaults();
        reg.blocklist.push("https://uniswap.evil-clone.io".into());
        let a = reg.assess("https://uniswap.evil-clone.io/").unwrap();
        assert_eq!(a.risk, DappRisk::Blocked);
        // First warning is the blocklist notice.
        assert!(a.warnings[0].contains("blocklist"));
    }

    #[test]
    fn assess_plain_http_warns_about_encryption() {
        let reg = DappRegistry::with_defaults();
        let a = reg.assess("http://insecure-dapp.example/").unwrap();
        assert!(a.warnings.iter().any(|w| w.contains("not encrypted")));
    }

    #[test]
    fn assess_localhost_http_does_not_warn_about_encryption() {
        let reg = DappRegistry::with_defaults();
        let a = reg.assess("http://localhost:5173/").unwrap();
        assert!(!a.warnings.iter().any(|w| w.contains("not encrypted")));
    }

    #[test]
    fn assess_returns_stable_fingerprint() {
        let reg = DappRegistry::with_defaults();
        let a = reg.assess("https://app.uniswap.org/").unwrap();
        let b = reg.assess("https://APP.uniswap.org/swap?x=1").unwrap();
        assert_eq!(a.fingerprint, b.fingerprint);
        assert_eq!(a.fingerprint.len(), 16);
    }

    #[test]
    fn defaults_contain_curated_dapps() {
        let reg = DappRegistry::with_defaults();
        assert!(reg.entries.iter().any(|e| e.name == "Uniswap"));
        assert!(reg.entries.iter().any(|e| e.name == "OpenSea"));
        assert!(reg.entries.iter().any(|e| e.name == "Jupiter"));
        // No two entries share an origin.
        let mut seen: Vec<&str> = Vec::new();
        for e in &reg.entries {
            assert!(
                !seen.contains(&e.origin.as_str()),
                "dup origin {}",
                e.origin
            );
            seen.push(&e.origin);
        }
    }

    #[test]
    fn assess_rejects_invalid_urls() {
        let reg = DappRegistry::with_defaults();
        assert!(reg.assess("not a url").is_err());
        assert!(reg.assess("ftp://example.com").is_err());
    }
}
