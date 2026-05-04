//! Local-only event recorder.
//!
//! Used by the wallet to surface a "Recent activity" panel and to
//! ship a redacted bundle when the user clicks "Report a problem".
//! Nothing here is sent off-device automatically.
//!
//! The store is a bounded ring buffer (most recent N events) so a
//! long-running session can't grow unbounded. Pure data; the host
//! is responsible for persistence.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

#[derive(Debug, Error, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "message")]
pub enum EventLogError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

const MAX_MESSAGE_BYTES: usize = 1_024;
const DEFAULT_CAPACITY: usize = 500;
const MAX_CAPACITY: usize = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
pub enum EventLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
pub enum EventCategory {
    Wallet,
    Transaction,
    Network,
    Security,
    Ui,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EventRecord {
    pub timestamp_unix_ms: u64,
    pub level: EventLevel,
    pub category: EventCategory,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct EventLog {
    capacity: usize,
    events: Vec<EventRecord>,
}

impl Default for EventLog {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY).expect("default capacity is valid")
    }
}

impl EventLog {
    pub fn with_capacity(capacity: usize) -> Result<Self, EventLogError> {
        if capacity == 0 {
            return Err(EventLogError::InvalidInput("capacity = 0".into()));
        }
        if capacity > MAX_CAPACITY {
            return Err(EventLogError::InvalidInput(format!(
                "capacity {capacity} > {MAX_CAPACITY}"
            )));
        }
        Ok(Self {
            capacity,
            events: Vec::new(),
        })
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn record(
        &mut self,
        timestamp_unix_ms: u64,
        level: EventLevel,
        category: EventCategory,
        message: &str,
    ) -> Result<(), EventLogError> {
        if message.len() > MAX_MESSAGE_BYTES {
            return Err(EventLogError::InvalidInput(format!(
                "message {} > {MAX_MESSAGE_BYTES}",
                message.len()
            )));
        }
        if self.events.len() == self.capacity {
            self.events.remove(0);
        }
        self.events.push(EventRecord {
            timestamp_unix_ms,
            level,
            category,
            message: message.to_string(),
        });
        Ok(())
    }

    /// Most recent first (newest at index 0).
    pub fn recent(&self, limit: usize) -> Vec<EventRecord> {
        self.events.iter().rev().take(limit).cloned().collect()
    }

    /// Filter by min-level + optional category. Most recent first.
    pub fn filter(
        &self,
        min_level: EventLevel,
        category: Option<EventCategory>,
        limit: usize,
    ) -> Vec<EventRecord> {
        self.events
            .iter()
            .rev()
            .filter(|e| e.level >= min_level)
            .filter(|e| category.is_none_or(|c| e.category == c))
            .take(limit)
            .cloned()
            .collect()
    }

    /// Drop everything.
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Redacted bundle for "Report a problem". Strips Debug events
    /// and replaces 0x... hex blobs and bech32-ish strings with
    /// `<redacted>` so a copy-pasted log can't leak full addresses.
    pub fn export_redacted(&self) -> Vec<EventRecord> {
        self.events
            .iter()
            .filter(|e| e.level != EventLevel::Debug)
            .map(|e| EventRecord {
                timestamp_unix_ms: e.timestamp_unix_ms,
                level: e.level,
                category: e.category,
                message: redact(&e.message),
            })
            .collect()
    }

    pub fn from_json(json: &str) -> Result<Self, EventLogError> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(json).map_err(|e| EventLogError::InvalidInput(e.to_string()))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Replace 0x-hex sequences (>=8 hex chars) and bech32-ish words
/// (bc1 / tb1 prefix, >=14 chars) with `<redacted>`.
fn redact(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for word in input.split_inclusive(char::is_whitespace) {
        let trimmed = word.trim_end_matches(char::is_whitespace);
        let suffix = &word[trimmed.len()..];
        if is_hex_blob(trimmed) || is_bech32ish(trimmed) {
            out.push_str("<redacted>");
            out.push_str(suffix);
        } else {
            out.push_str(word);
        }
    }
    out
}

fn is_hex_blob(s: &str) -> bool {
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"));
    match s {
        Some(rest) => rest.len() >= 8 && rest.chars().all(|c| c.is_ascii_hexdigit()),
        None => false,
    }
}

fn is_bech32ish(s: &str) -> bool {
    if s.len() < 14 {
        return false;
    }
    let lower = s.to_lowercase();
    (lower.starts_with("bc1") || lower.starts_with("tb1") || lower.starts_with("bcrt1"))
        && lower.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(log: &mut EventLog, ts: u64, level: EventLevel, msg: &str) {
        log.record(ts, level, EventCategory::Wallet, msg).unwrap();
    }

    #[test]
    fn default_capacity_is_positive() {
        let log = EventLog::default();
        assert!(log.capacity() > 0);
        assert!(log.is_empty());
    }

    #[test]
    fn record_and_recent_returns_newest_first() {
        let mut log = EventLog::with_capacity(10).unwrap();
        rec(&mut log, 1, EventLevel::Info, "first");
        rec(&mut log, 2, EventLevel::Info, "second");
        rec(&mut log, 3, EventLevel::Info, "third");
        let recent = log.recent(10);
        assert_eq!(recent[0].message, "third");
        assert_eq!(recent[2].message, "first");
    }

    #[test]
    fn ring_buffer_drops_oldest() {
        let mut log = EventLog::with_capacity(3).unwrap();
        rec(&mut log, 1, EventLevel::Info, "a");
        rec(&mut log, 2, EventLevel::Info, "b");
        rec(&mut log, 3, EventLevel::Info, "c");
        rec(&mut log, 4, EventLevel::Info, "d");
        let all = log.recent(10);
        assert_eq!(all.len(), 3);
        assert_eq!(all.last().unwrap().message, "b");
    }

    #[test]
    fn capacity_zero_is_rejected() {
        let err = EventLog::with_capacity(0).unwrap_err();
        assert!(matches!(err, EventLogError::InvalidInput(_)));
    }

    #[test]
    fn capacity_above_cap_is_rejected() {
        let err = EventLog::with_capacity(MAX_CAPACITY + 1).unwrap_err();
        assert!(matches!(err, EventLogError::InvalidInput(_)));
    }

    #[test]
    fn message_length_is_enforced() {
        let mut log = EventLog::with_capacity(10).unwrap();
        let big = "x".repeat(MAX_MESSAGE_BYTES + 1);
        let err = log
            .record(1, EventLevel::Info, EventCategory::Wallet, &big)
            .unwrap_err();
        assert!(matches!(err, EventLogError::InvalidInput(_)));
    }

    #[test]
    fn filter_by_min_level_drops_below() {
        let mut log = EventLog::with_capacity(10).unwrap();
        rec(&mut log, 1, EventLevel::Debug, "d");
        rec(&mut log, 2, EventLevel::Info, "i");
        rec(&mut log, 3, EventLevel::Warn, "w");
        rec(&mut log, 4, EventLevel::Error, "e");
        let warn = log.filter(EventLevel::Warn, None, 100);
        let levels: Vec<EventLevel> = warn.iter().map(|r| r.level).collect();
        assert_eq!(levels, vec![EventLevel::Error, EventLevel::Warn]);
    }

    #[test]
    fn filter_by_category() {
        let mut log = EventLog::with_capacity(10).unwrap();
        log.record(1, EventLevel::Info, EventCategory::Wallet, "w")
            .unwrap();
        log.record(2, EventLevel::Info, EventCategory::Network, "n")
            .unwrap();
        log.record(3, EventLevel::Info, EventCategory::Wallet, "w2")
            .unwrap();
        let only_wallet = log.filter(EventLevel::Debug, Some(EventCategory::Wallet), 100);
        assert_eq!(only_wallet.len(), 2);
    }

    #[test]
    fn clear_empties_log() {
        let mut log = EventLog::with_capacity(10).unwrap();
        rec(&mut log, 1, EventLevel::Info, "x");
        log.clear();
        assert!(log.is_empty());
    }

    #[test]
    fn export_redacted_drops_debug_events() {
        let mut log = EventLog::with_capacity(10).unwrap();
        rec(&mut log, 1, EventLevel::Debug, "secret");
        rec(&mut log, 2, EventLevel::Info, "public");
        let bundle = log.export_redacted();
        assert_eq!(bundle.len(), 1);
        assert_eq!(bundle[0].message, "public");
    }

    #[test]
    fn export_redacted_masks_hex_blobs() {
        let mut log = EventLog::with_capacity(10).unwrap();
        rec(
            &mut log,
            1,
            EventLevel::Info,
            "sent to 0xDeadBeefCafe1234567890abcdef done",
        );
        let bundle = log.export_redacted();
        assert!(bundle[0].message.contains("<redacted>"));
        assert!(!bundle[0].message.contains("0xDeadBeef"));
    }

    #[test]
    fn export_redacted_masks_bech32() {
        let mut log = EventLog::with_capacity(10).unwrap();
        rec(
            &mut log,
            1,
            EventLevel::Info,
            "to bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh ok",
        );
        let bundle = log.export_redacted();
        assert!(bundle[0].message.contains("<redacted>"));
    }

    #[test]
    fn export_redacted_keeps_short_strings() {
        let mut log = EventLog::with_capacity(10).unwrap();
        rec(&mut log, 1, EventLevel::Info, "0xab short hex");
        let bundle = log.export_redacted();
        // Only 2 hex chars after 0x → not redacted.
        assert!(bundle[0].message.contains("0xab"));
    }

    #[test]
    fn json_round_trip() {
        let mut log = EventLog::with_capacity(10).unwrap();
        rec(&mut log, 1, EventLevel::Warn, "x");
        let restored = EventLog::from_json(&log.to_json()).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored.capacity(), 10);
    }

    #[test]
    fn from_json_empty_is_default() {
        let log = EventLog::from_json("").unwrap();
        assert!(log.is_empty());
        assert!(log.capacity() > 0);
    }

    #[test]
    fn recent_limit_is_respected() {
        let mut log = EventLog::with_capacity(10).unwrap();
        for i in 0..5 {
            rec(&mut log, i, EventLevel::Info, "x");
        }
        assert_eq!(log.recent(2).len(), 2);
    }
}
