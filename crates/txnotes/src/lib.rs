//! Per-transaction notes & tags store.
//!
//! Pure data layer — owns no I/O. The host (Tauri) is expected to
//! load/save the JSON blob from disk and pass it in. A note is
//! keyed by `(chain, txid_lowercase)` so the same hex string on
//! Bitcoin and Ethereum cannot collide.
//!
//! Tags are normalised: lowercase, trimmed, deduplicated, and
//! capped to keep the UI sane.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "message")]
pub enum NoteError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("note too long: {0} > {1} bytes")]
    NoteTooLong(usize, usize),
    #[error("too many tags: {0} > {1}")]
    TooManyTags(usize, usize),
}

const MAX_NOTE_BYTES: usize = 512;
const MAX_TAGS: usize = 16;
const MAX_TAG_LEN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
pub enum Chain {
    Bitcoin,
    Ethereum,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TxNote {
    pub chain: Chain,
    pub txid: String,
    pub note: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, Type)]
pub struct TxNoteStore {
    /// Composite key: `"<chain>:<txid_lowercase>"`.
    entries: BTreeMap<String, TxNote>,
}

impl TxNoteStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_json(json: &str) -> Result<Self, NoteError> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(json).map_err(|e| NoteError::InvalidInput(e.to_string()))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, chain: Chain, txid: &str) -> Option<&TxNote> {
        self.entries.get(&key(chain, txid))
    }

    pub fn list(&self) -> Vec<TxNote> {
        self.entries.values().cloned().collect()
    }

    pub fn list_by_tag(&self, tag: &str) -> Vec<TxNote> {
        let needle = tag.trim().to_lowercase();
        self.entries
            .values()
            .filter(|n| n.tags.iter().any(|t| t == &needle))
            .cloned()
            .collect()
    }

    /// Insert or replace. Empty note + empty tags removes the entry.
    pub fn upsert(
        &mut self,
        chain: Chain,
        txid: &str,
        note: &str,
        tags: &[String],
    ) -> Result<(), NoteError> {
        let txid_norm = txid.trim().to_lowercase();
        if txid_norm.is_empty() {
            return Err(NoteError::InvalidInput("empty txid".into()));
        }
        if note.len() > MAX_NOTE_BYTES {
            return Err(NoteError::NoteTooLong(note.len(), MAX_NOTE_BYTES));
        }
        let normalised_tags = normalise_tags(tags)?;
        let k = key(chain, &txid_norm);

        if note.trim().is_empty() && normalised_tags.is_empty() {
            self.entries.remove(&k);
            return Ok(());
        }

        self.entries.insert(
            k,
            TxNote {
                chain,
                txid: txid_norm,
                note: note.to_string(),
                tags: normalised_tags,
            },
        );
        Ok(())
    }

    pub fn remove(&mut self, chain: Chain, txid: &str) -> bool {
        self.entries.remove(&key(chain, txid)).is_some()
    }

    /// All distinct tags currently in use, sorted alphabetically.
    pub fn all_tags(&self) -> Vec<String> {
        let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for n in self.entries.values() {
            for t in &n.tags {
                set.insert(t.clone());
            }
        }
        set.into_iter().collect()
    }
}

fn key(chain: Chain, txid: &str) -> String {
    let prefix = match chain {
        Chain::Bitcoin => "btc",
        Chain::Ethereum => "eth",
    };
    format!("{prefix}:{}", txid.trim().to_lowercase())
}

fn normalise_tags(tags: &[String]) -> Result<Vec<String>, NoteError> {
    let mut out: Vec<String> = Vec::new();
    for raw in tags {
        let t = raw.trim().to_lowercase();
        if t.is_empty() {
            continue;
        }
        if t.len() > MAX_TAG_LEN {
            return Err(NoteError::InvalidInput(format!(
                "tag too long ({} > {MAX_TAG_LEN})",
                t.len()
            )));
        }
        if !out.contains(&t) {
            out.push(t);
        }
    }
    if out.len() > MAX_TAGS {
        return Err(NoteError::TooManyTags(out.len(), MAX_TAGS));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn upsert_then_get() {
        let mut store = TxNoteStore::new();
        store
            .upsert(Chain::Bitcoin, "ABC123", "rent", &s(&["bills"]))
            .unwrap();
        let got = store.get(Chain::Bitcoin, "abc123").unwrap();
        assert_eq!(got.note, "rent");
        assert_eq!(got.tags, vec!["bills"]);
    }

    #[test]
    fn txid_is_case_insensitive() {
        let mut store = TxNoteStore::new();
        store
            .upsert(Chain::Ethereum, "0xDeadBeef", "swap", &[])
            .unwrap();
        assert!(store.get(Chain::Ethereum, "0xdeadbeef").is_some());
        assert!(store.get(Chain::Ethereum, "0xDEADBEEF").is_some());
    }

    #[test]
    fn chain_separates_keys() {
        let mut store = TxNoteStore::new();
        store
            .upsert(Chain::Bitcoin, "abc", "btc-side", &[])
            .unwrap();
        store
            .upsert(Chain::Ethereum, "abc", "eth-side", &[])
            .unwrap();
        assert_eq!(store.get(Chain::Bitcoin, "abc").unwrap().note, "btc-side");
        assert_eq!(store.get(Chain::Ethereum, "abc").unwrap().note, "eth-side");
    }

    #[test]
    fn tags_are_normalised_and_deduplicated() {
        let mut store = TxNoteStore::new();
        store
            .upsert(
                Chain::Bitcoin,
                "abc",
                "x",
                &s(&[" Food ", "food", "TRAVEL"]),
            )
            .unwrap();
        let got = store.get(Chain::Bitcoin, "abc").unwrap();
        assert_eq!(got.tags, vec!["food", "travel"]);
    }

    #[test]
    fn empty_note_and_no_tags_removes() {
        let mut store = TxNoteStore::new();
        store
            .upsert(Chain::Bitcoin, "abc", "x", &s(&["bills"]))
            .unwrap();
        store.upsert(Chain::Bitcoin, "abc", "", &[]).unwrap();
        assert!(store.get(Chain::Bitcoin, "abc").is_none());
    }

    #[test]
    fn note_length_is_enforced() {
        let mut store = TxNoteStore::new();
        let big = "x".repeat(MAX_NOTE_BYTES + 1);
        let err = store.upsert(Chain::Bitcoin, "abc", &big, &[]).unwrap_err();
        assert!(matches!(err, NoteError::NoteTooLong(_, _)));
    }

    #[test]
    fn too_many_tags_is_rejected() {
        let mut store = TxNoteStore::new();
        let many: Vec<String> = (0..(MAX_TAGS + 1)).map(|n| format!("t{n}")).collect();
        let err = store.upsert(Chain::Bitcoin, "abc", "x", &many).unwrap_err();
        assert!(matches!(err, NoteError::TooManyTags(_, _)));
    }

    #[test]
    fn empty_txid_is_rejected() {
        let mut store = TxNoteStore::new();
        let err = store.upsert(Chain::Bitcoin, "  ", "x", &[]).unwrap_err();
        assert!(matches!(err, NoteError::InvalidInput(_)));
    }

    #[test]
    fn list_by_tag_filters() {
        let mut store = TxNoteStore::new();
        store
            .upsert(Chain::Bitcoin, "a", "x", &s(&["food"]))
            .unwrap();
        store
            .upsert(Chain::Bitcoin, "b", "y", &s(&["bills"]))
            .unwrap();
        store
            .upsert(Chain::Bitcoin, "c", "z", &s(&["food", "bills"]))
            .unwrap();
        assert_eq!(store.list_by_tag("food").len(), 2);
        assert_eq!(store.list_by_tag("bills").len(), 2);
        assert_eq!(store.list_by_tag("travel").len(), 0);
    }

    #[test]
    fn all_tags_is_sorted_and_unique() {
        let mut store = TxNoteStore::new();
        store
            .upsert(Chain::Bitcoin, "a", "x", &s(&["zeta", "alpha"]))
            .unwrap();
        store
            .upsert(Chain::Bitcoin, "b", "y", &s(&["alpha", "mu"]))
            .unwrap();
        assert_eq!(store.all_tags(), vec!["alpha", "mu", "zeta"]);
    }

    #[test]
    fn json_round_trip_preserves_entries() {
        let mut store = TxNoteStore::new();
        store
            .upsert(Chain::Bitcoin, "abc", "n1", &s(&["bills"]))
            .unwrap();
        store
            .upsert(Chain::Ethereum, "0xdead", "n2", &s(&["swap"]))
            .unwrap();
        let json = store.to_json();
        let restored = TxNoteStore::from_json(&json).unwrap();
        assert_eq!(restored.len(), 2);
        assert_eq!(restored.get(Chain::Bitcoin, "abc").unwrap().note, "n1");
    }

    #[test]
    fn from_json_empty_string_is_empty_store() {
        let store = TxNoteStore::from_json("").unwrap();
        assert!(store.is_empty());
    }

    #[test]
    fn from_json_malformed_errors() {
        let err = TxNoteStore::from_json("{not-json").unwrap_err();
        assert!(matches!(err, NoteError::InvalidInput(_)));
    }

    #[test]
    fn remove_returns_whether_existed() {
        let mut store = TxNoteStore::new();
        store.upsert(Chain::Bitcoin, "abc", "x", &[]).unwrap();
        assert!(store.remove(Chain::Bitcoin, "abc"));
        assert!(!store.remove(Chain::Bitcoin, "abc"));
    }
}
