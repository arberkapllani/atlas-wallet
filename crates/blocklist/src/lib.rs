//! Malicious-address blocklist registry.
//!
//! A small in-memory store of addresses (EVM-style or otherwise)
//! that are known to be drainers, phishing wallets, scam contracts,
//! or OFAC-sanctioned. The wallet checks every outbound destination
//! against this list and surfaces a warning before signing.
//!
//! The blocklist is not pretending to be exhaustive — it's a layer
//! the user can extend at runtime, and a place we can ship curated
//! entries from upstream feeds. Pure data, no I/O.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

#[derive(Debug, Error, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "message")]
pub enum BlocklistError {
    #[error("invalid json: {0}")]
    InvalidJson(String),
    #[error("invalid address: {0}")]
    InvalidAddress(String),
}

/// What kind of bad-actor an entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum BlocklistCategory {
    /// Wallet drainer (auto-sweeps approved tokens).
    Drainer,
    /// Phishing / impersonation wallet.
    Phishing,
    /// Generic scam / pig-butchering / fake support.
    Scam,
    /// OFAC or other government sanctions list.
    Sanctioned,
    /// Reported by the user themselves.
    UserReported,
}

/// One row in the blocklist.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BlocklistEntry {
    /// Normalised address (lower-case, stripped `0x`).
    pub address: String,
    pub category: BlocklistCategory,
    /// Free-text source attribution (e.g. "scamsniffer", "user").
    pub source: String,
    /// Optional reason / context.
    pub note: Option<String>,
    /// Unix seconds when added.
    pub added_at: i64,
}

/// Result of looking up an address.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "data")]
pub enum BlocklistVerdict {
    Clean,
    Listed(BlocklistEntry),
}

/// In-memory, hash-keyed blocklist.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct Blocklist {
    entries: HashMap<String, BlocklistEntry>,
}

impl Blocklist {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Insert (or overwrite) an entry. Returns the normalised key.
    pub fn add(&mut self, entry: BlocklistEntry) -> Result<String, BlocklistError> {
        let key = normalise(&entry.address)?;
        let mut e = entry;
        e.address = key.clone();
        self.entries.insert(key.clone(), e);
        Ok(key)
    }

    /// Remove an address. Returns `true` if it was present.
    pub fn remove(&mut self, address: &str) -> bool {
        match normalise(address) {
            Ok(k) => self.entries.remove(&k).is_some(),
            Err(_) => false,
        }
    }

    /// Look up an address.
    pub fn check(&self, address: &str) -> BlocklistVerdict {
        let key = match normalise(address) {
            Ok(k) => k,
            Err(_) => return BlocklistVerdict::Clean,
        };
        match self.entries.get(&key) {
            Some(e) => BlocklistVerdict::Listed(e.clone()),
            None => BlocklistVerdict::Clean,
        }
    }

    /// Bulk-load from a JSON array of `BlocklistEntry`.
    pub fn merge_json(&mut self, json: &str) -> Result<usize, BlocklistError> {
        let parsed: Vec<BlocklistEntry> =
            serde_json::from_str(json).map_err(|e| BlocklistError::InvalidJson(e.to_string()))?;
        let mut added = 0usize;
        for e in parsed {
            self.add(e)?;
            added += 1;
        }
        Ok(added)
    }

    /// Sorted iter (by address) — handy for stable UI rendering.
    pub fn sorted(&self) -> Vec<BlocklistEntry> {
        let mut v: Vec<BlocklistEntry> = self.entries.values().cloned().collect();
        v.sort_by(|a, b| a.address.cmp(&b.address));
        v
    }
}

/// Normalise an address: strip whitespace, lower-case, strip `0x`.
/// Rejects empty after normalisation.
pub fn normalise(addr: &str) -> Result<String, BlocklistError> {
    let trimmed = addr.trim().to_ascii_lowercase();
    let stripped = trimmed.trim_start_matches("0x").to_string();
    if stripped.is_empty() {
        return Err(BlocklistError::InvalidAddress(addr.to_string()));
    }
    Ok(stripped)
}

/// Convenience: assess one address against a blocklist.
pub fn assess(address: &str, list: &Blocklist) -> BlocklistVerdict {
    list.check(address)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(addr: &str, cat: BlocklistCategory) -> BlocklistEntry {
        BlocklistEntry {
            address: addr.to_string(),
            category: cat,
            source: "test".into(),
            note: None,
            added_at: 1_700_000_000,
        }
    }

    #[test]
    fn empty_blocklist_is_clean() {
        let bl = Blocklist::new();
        assert!(matches!(bl.check("0xabc"), BlocklistVerdict::Clean));
    }

    #[test]
    fn add_and_check_roundtrip() {
        let mut bl = Blocklist::new();
        bl.add(entry(
            "0xDeAdBeEfDeAdBeEfDeAdBeEfDeAdBeEfDeAdBeEf",
            BlocklistCategory::Drainer,
        ))
        .unwrap();
        match bl.check("0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef") {
            BlocklistVerdict::Listed(e) => assert_eq!(e.category, BlocklistCategory::Drainer),
            _ => panic!("expected Listed"),
        }
    }

    #[test]
    fn case_insensitive_and_strips_0x() {
        let mut bl = Blocklist::new();
        bl.add(entry("AABBCC", BlocklistCategory::Phishing))
            .unwrap();
        assert!(matches!(bl.check("0xaabbcc"), BlocklistVerdict::Listed(_)));
        assert!(matches!(bl.check("0xAABBCC"), BlocklistVerdict::Listed(_)));
    }

    #[test]
    fn unknown_address_is_clean() {
        let mut bl = Blocklist::new();
        bl.add(entry("0xabc", BlocklistCategory::Drainer)).unwrap();
        assert!(matches!(bl.check("0xdef"), BlocklistVerdict::Clean));
    }

    #[test]
    fn remove_works() {
        let mut bl = Blocklist::new();
        bl.add(entry("0xabc", BlocklistCategory::Scam)).unwrap();
        assert!(bl.remove("0xABC"));
        assert!(matches!(bl.check("0xabc"), BlocklistVerdict::Clean));
        assert_eq!(bl.len(), 0);
    }

    #[test]
    fn remove_missing_returns_false() {
        let mut bl = Blocklist::new();
        assert!(!bl.remove("0xnope"));
    }

    #[test]
    fn empty_address_is_invalid() {
        let mut bl = Blocklist::new();
        let err = bl.add(entry("   ", BlocklistCategory::Scam)).unwrap_err();
        assert!(matches!(err, BlocklistError::InvalidAddress(_)));
    }

    #[test]
    fn check_invalid_address_is_clean() {
        let bl = Blocklist::new();
        assert!(matches!(bl.check("   "), BlocklistVerdict::Clean));
    }

    #[test]
    fn merge_json_loads_entries() {
        let mut bl = Blocklist::new();
        let json = r#"[
            {"address":"0xaaa","category":"Drainer","source":"feed","note":null,"added_at":1},
            {"address":"0xbbb","category":"Sanctioned","source":"ofac","note":"sdn","added_at":2}
        ]"#;
        let n = bl.merge_json(json).unwrap();
        assert_eq!(n, 2);
        assert_eq!(bl.len(), 2);
    }

    #[test]
    fn merge_json_rejects_garbage() {
        let mut bl = Blocklist::new();
        let err = bl.merge_json("garbage").unwrap_err();
        assert!(matches!(err, BlocklistError::InvalidJson(_)));
    }

    #[test]
    fn add_overwrites_existing() {
        let mut bl = Blocklist::new();
        bl.add(entry("0xabc", BlocklistCategory::Scam)).unwrap();
        bl.add(entry("0xABC", BlocklistCategory::Drainer)).unwrap();
        assert_eq!(bl.len(), 1);
        match bl.check("0xabc") {
            BlocklistVerdict::Listed(e) => assert_eq!(e.category, BlocklistCategory::Drainer),
            _ => panic!(),
        }
    }

    #[test]
    fn sorted_returns_stable_order() {
        let mut bl = Blocklist::new();
        bl.add(entry("0x222", BlocklistCategory::Scam)).unwrap();
        bl.add(entry("0x111", BlocklistCategory::Scam)).unwrap();
        bl.add(entry("0x333", BlocklistCategory::Scam)).unwrap();
        let v = bl.sorted();
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].address, "111");
        assert_eq!(v[1].address, "222");
        assert_eq!(v[2].address, "333");
    }

    #[test]
    fn assess_helper() {
        let mut bl = Blocklist::new();
        bl.add(entry("0xfeedface", BlocklistCategory::UserReported))
            .unwrap();
        assert!(matches!(
            assess("0xFEEDFACE", &bl),
            BlocklistVerdict::Listed(_)
        ));
        assert!(matches!(assess("0xnope", &bl), BlocklistVerdict::Clean));
    }
}
