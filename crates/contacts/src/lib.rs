//! Address book / contacts store.
//!
//! Pure data: a contact has a stable id (UUID), a display name, an
//! optional note, and one or more chain-tagged addresses. The
//! same person can hold both a Bitcoin and an Ethereum address
//! under one entry.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "message")]
pub enum ContactError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("not found")]
    NotFound,
}

const MAX_NAME_LEN: usize = 64;
const MAX_NOTE_LEN: usize = 256;
const MAX_ADDRESSES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
pub enum Chain {
    Bitcoin,
    Ethereum,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ContactAddress {
    pub chain: Chain,
    /// Lower-cased, trimmed.
    pub address: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Contact {
    pub id: String,
    pub name: String,
    pub note: String,
    pub addresses: Vec<ContactAddress>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, Type)]
pub struct ContactBook {
    contacts: BTreeMap<String, Contact>,
}

impl ContactBook {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_json(json: &str) -> Result<Self, ContactError> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(json).map_err(|e| ContactError::InvalidInput(e.to_string()))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn len(&self) -> usize {
        self.contacts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.contacts.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&Contact> {
        self.contacts.get(id)
    }

    /// All contacts, ordered by name (case-insensitive).
    pub fn list(&self) -> Vec<Contact> {
        let mut v: Vec<Contact> = self.contacts.values().cloned().collect();
        v.sort_by_key(|c| c.name.to_lowercase());
        v
    }

    /// Add a new contact and return its id.
    pub fn add(
        &mut self,
        name: &str,
        note: &str,
        addresses: &[ContactAddress],
    ) -> Result<String, ContactError> {
        let (name, note, addresses) = validate(name, note, addresses)?;
        let id = Uuid::new_v4().to_string();
        self.contacts.insert(
            id.clone(),
            Contact {
                id: id.clone(),
                name,
                note,
                addresses,
            },
        );
        Ok(id)
    }

    /// Update an existing contact.
    pub fn update(
        &mut self,
        id: &str,
        name: &str,
        note: &str,
        addresses: &[ContactAddress],
    ) -> Result<(), ContactError> {
        if !self.contacts.contains_key(id) {
            return Err(ContactError::NotFound);
        }
        let (name, note, addresses) = validate(name, note, addresses)?;
        self.contacts.insert(
            id.to_string(),
            Contact {
                id: id.to_string(),
                name,
                note,
                addresses,
            },
        );
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> bool {
        self.contacts.remove(id).is_some()
    }

    /// Look up the contact owning a given address (chain-aware).
    pub fn find_by_address(&self, chain: Chain, address: &str) -> Option<&Contact> {
        let needle = address.trim().to_lowercase();
        self.contacts.values().find(|c| {
            c.addresses
                .iter()
                .any(|a| a.chain == chain && a.address == needle)
        })
    }

    /// Case-insensitive substring search across name + note.
    pub fn search(&self, query: &str) -> Vec<Contact> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return self.list();
        }
        let mut hits: Vec<Contact> = self
            .contacts
            .values()
            .filter(|c| {
                c.name.to_lowercase().contains(&needle) || c.note.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect();
        hits.sort_by_key(|c| c.name.to_lowercase());
        hits
    }
}

fn validate(
    name: &str,
    note: &str,
    addresses: &[ContactAddress],
) -> Result<(String, String, Vec<ContactAddress>), ContactError> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(ContactError::InvalidInput("empty name".into()));
    }
    if name.len() > MAX_NAME_LEN {
        return Err(ContactError::InvalidInput(format!(
            "name {} > {MAX_NAME_LEN}",
            name.len()
        )));
    }
    if note.len() > MAX_NOTE_LEN {
        return Err(ContactError::InvalidInput(format!(
            "note {} > {MAX_NOTE_LEN}",
            note.len()
        )));
    }
    if addresses.len() > MAX_ADDRESSES {
        return Err(ContactError::InvalidInput(format!(
            "addresses {} > {MAX_ADDRESSES}",
            addresses.len()
        )));
    }
    let mut normalised: Vec<ContactAddress> = Vec::new();
    for a in addresses {
        let addr = a.address.trim().to_lowercase();
        if addr.is_empty() {
            return Err(ContactError::InvalidInput("empty address".into()));
        }
        let entry = ContactAddress {
            chain: a.chain,
            address: addr,
        };
        if !normalised.contains(&entry) {
            normalised.push(entry);
        }
    }
    Ok((name, note.to_string(), normalised))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(chain: Chain, a: &str) -> ContactAddress {
        ContactAddress {
            chain,
            address: a.to_string(),
        }
    }

    #[test]
    fn add_then_get() {
        let mut book = ContactBook::new();
        let id = book
            .add("Alice", "friend", &[addr(Chain::Bitcoin, "bc1qabc")])
            .unwrap();
        let c = book.get(&id).unwrap();
        assert_eq!(c.name, "Alice");
        assert_eq!(c.addresses.len(), 1);
    }

    #[test]
    fn address_is_lowercased_and_trimmed() {
        let mut book = ContactBook::new();
        let id = book
            .add("Bob", "", &[addr(Chain::Ethereum, "  0xDeadBeef ")])
            .unwrap();
        assert_eq!(book.get(&id).unwrap().addresses[0].address, "0xdeadbeef");
    }

    #[test]
    fn find_by_address_is_case_insensitive() {
        let mut book = ContactBook::new();
        book.add("Bob", "", &[addr(Chain::Ethereum, "0xdeadbeef")])
            .unwrap();
        let c = book.find_by_address(Chain::Ethereum, "0xDEADBEEF").unwrap();
        assert_eq!(c.name, "Bob");
    }

    #[test]
    fn find_by_address_chain_isolation() {
        let mut book = ContactBook::new();
        book.add("Bob", "", &[addr(Chain::Bitcoin, "abc")]).unwrap();
        assert!(book.find_by_address(Chain::Ethereum, "abc").is_none());
    }

    #[test]
    fn duplicate_addresses_are_deduplicated() {
        let mut book = ContactBook::new();
        let id = book
            .add(
                "Bob",
                "",
                &[
                    addr(Chain::Bitcoin, "abc"),
                    addr(Chain::Bitcoin, "ABC"),
                    addr(Chain::Bitcoin, "abc"),
                ],
            )
            .unwrap();
        assert_eq!(book.get(&id).unwrap().addresses.len(), 1);
    }

    #[test]
    fn empty_name_is_rejected() {
        let mut book = ContactBook::new();
        let err = book.add("   ", "", &[]).unwrap_err();
        assert!(matches!(err, ContactError::InvalidInput(_)));
    }

    #[test]
    fn note_length_is_enforced() {
        let mut book = ContactBook::new();
        let big = "x".repeat(MAX_NOTE_LEN + 1);
        let err = book.add("Bob", &big, &[]).unwrap_err();
        assert!(matches!(err, ContactError::InvalidInput(_)));
    }

    #[test]
    fn empty_address_in_list_is_rejected() {
        let mut book = ContactBook::new();
        let err = book
            .add("Bob", "", &[addr(Chain::Bitcoin, "  ")])
            .unwrap_err();
        assert!(matches!(err, ContactError::InvalidInput(_)));
    }

    #[test]
    fn update_changes_fields() {
        let mut book = ContactBook::new();
        let id = book.add("Bob", "x", &[]).unwrap();
        book.update(&id, "Robert", "y", &[]).unwrap();
        let c = book.get(&id).unwrap();
        assert_eq!(c.name, "Robert");
        assert_eq!(c.note, "y");
    }

    #[test]
    fn update_unknown_id_returns_not_found() {
        let mut book = ContactBook::new();
        let err = book.update("nope", "x", "", &[]).unwrap_err();
        assert!(matches!(err, ContactError::NotFound));
    }

    #[test]
    fn remove_returns_whether_existed() {
        let mut book = ContactBook::new();
        let id = book.add("Bob", "", &[]).unwrap();
        assert!(book.remove(&id));
        assert!(!book.remove(&id));
    }

    #[test]
    fn list_is_sorted_by_name_case_insensitive() {
        let mut book = ContactBook::new();
        book.add("charlie", "", &[]).unwrap();
        book.add("Alice", "", &[]).unwrap();
        book.add("bob", "", &[]).unwrap();
        let names: Vec<String> = book.list().into_iter().map(|c| c.name).collect();
        assert_eq!(names, vec!["Alice", "bob", "charlie"]);
    }

    #[test]
    fn search_matches_name_or_note() {
        let mut book = ContactBook::new();
        book.add("Alice", "favourite", &[]).unwrap();
        book.add("Bob", "rent", &[]).unwrap();
        assert_eq!(book.search("alice").len(), 1);
        assert_eq!(book.search("RENT").len(), 1);
        assert_eq!(book.search("missing").len(), 0);
    }

    #[test]
    fn search_empty_returns_all() {
        let mut book = ContactBook::new();
        book.add("Alice", "", &[]).unwrap();
        book.add("Bob", "", &[]).unwrap();
        assert_eq!(book.search("").len(), 2);
    }

    #[test]
    fn json_round_trip() {
        let mut book = ContactBook::new();
        book.add("Alice", "x", &[addr(Chain::Bitcoin, "abc")])
            .unwrap();
        let restored = ContactBook::from_json(&book.to_json()).unwrap();
        assert_eq!(restored.len(), 1);
    }

    #[test]
    fn from_json_empty_is_empty_book() {
        let book = ContactBook::from_json("").unwrap();
        assert!(book.is_empty());
    }
}
