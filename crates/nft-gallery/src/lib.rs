//! Gallery-side aggregation on top of `atlas-nft-registry`.
//!
//! `atlas-nft-registry` returns a flat list of every NFT a user owns
//! across supported EVM chains. The UI doesn't want a flat list — it
//! wants:
//!
//! - Collections grouped together with cover art and item counts.
//! - Floor-price sums per collection (best-effort USD valuation).
//! - Hidden / spam tokens filtered out (or surfaced separately so
//!   the user can review them).
//! - Stable, deterministic ordering so the UI doesn't reshuffle on
//!   every refresh.
//!
//! This crate is pure data — it takes `Vec<OwnedNft>` and produces
//! `GalleryView` without any network I/O.

use atlas_nft_registry::OwnedNft;
use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GalleryError {
    #[error("invalid filter: {0}")]
    InvalidFilter(String),
}

/// One collection grouping in the gallery.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CollectionGroup {
    /// `chain:contract` — stable cross-chain key.
    pub key: String,
    pub chain_id: String,
    pub contract: String,
    /// Display name (falls back to the contract address when absent).
    pub name: String,
    pub item_count: u32,
    /// Sum of per-item `floor_usd` values that were known. Items with
    /// unknown floor are excluded from the sum and recorded in
    /// `unpriced_count`.
    pub estimated_value_usd: f64,
    pub unpriced_count: u32,
    /// Cover image: the first item's image, if any.
    pub cover_image: Option<String>,
    /// Items in stable order (by token-id string).
    pub items: Vec<OwnedNft>,
}

/// Aggregated view of a user's NFT holdings.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct GalleryView {
    /// Visible collections, sorted by estimated USD value desc, then
    /// by item count desc, then by name.
    pub collections: Vec<CollectionGroup>,
    /// Items the spam filter set aside. Surfaced so the user can
    /// un-hide them from settings.
    pub hidden_items: Vec<OwnedNft>,
    pub total_collections: u32,
    pub total_items: u32,
    pub total_value_usd: f64,
}

/// Filter knobs passed in from the UI.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct GalleryFilter {
    /// If true, items matching the spam heuristic are routed to
    /// `hidden_items` instead of being grouped.
    pub hide_spam: bool,
    /// Hide collections worth less than this in USD (estimated). 0
    /// disables the filter.
    pub min_collection_value_usd: f64,
    /// If non-empty, only items on these chains are included.
    pub chains: Vec<String>,
}

/// Build a `GalleryView` from a flat list of owned NFTs.
pub fn build_gallery(items: &[OwnedNft], filter: &GalleryFilter) -> GalleryView {
    let mut visible: Vec<&OwnedNft> = Vec::new();
    let mut hidden: Vec<OwnedNft> = Vec::new();

    let chain_filter_active = !filter.chains.is_empty();
    for it in items {
        if chain_filter_active && !filter.chains.iter().any(|c| c == &it.chain_id) {
            continue;
        }
        if filter.hide_spam && is_spam(it) {
            hidden.push(it.clone());
            continue;
        }
        visible.push(it);
    }

    // Group by (chain, contract).
    let mut groups: Vec<CollectionGroup> = Vec::new();
    for it in &visible {
        let key = collection_key(&it.chain_id, &it.contract);
        match groups.iter_mut().find(|g| g.key == key) {
            Some(g) => {
                g.item_count += 1;
                match it.floor_usd {
                    Some(p) => g.estimated_value_usd += p,
                    None => g.unpriced_count += 1,
                }
                if g.cover_image.is_none() {
                    g.cover_image = it.image.clone();
                }
                g.items.push((*it).clone());
            }
            None => {
                let (val, unpriced) = match it.floor_usd {
                    Some(p) => (p, 0),
                    None => (0.0, 1),
                };
                groups.push(CollectionGroup {
                    key,
                    chain_id: it.chain_id.clone(),
                    contract: it.contract.clone(),
                    name: it
                        .collection
                        .clone()
                        .unwrap_or_else(|| short_address(&it.contract)),
                    item_count: 1,
                    estimated_value_usd: val,
                    unpriced_count: unpriced,
                    cover_image: it.image.clone(),
                    items: vec![(*it).clone()],
                });
            }
        }
    }

    // Per-group: stable item order.
    for g in &mut groups {
        g.items.sort_by(|a, b| a.token_id.cmp(&b.token_id));
    }

    // Min-value filter.
    if filter.min_collection_value_usd > 0.0 {
        groups.retain(|g| g.estimated_value_usd >= filter.min_collection_value_usd);
    }

    // Sort: value desc, count desc, name asc.
    groups.sort_by(|a, b| {
        b.estimated_value_usd
            .partial_cmp(&a.estimated_value_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.item_count.cmp(&a.item_count))
            .then_with(|| a.name.cmp(&b.name))
    });

    let total_collections = groups.len() as u32;
    let total_items: u32 = groups.iter().map(|g| g.item_count).sum();
    let total_value_usd: f64 = groups.iter().map(|g| g.estimated_value_usd).sum();

    GalleryView {
        collections: groups,
        hidden_items: hidden,
        total_collections,
        total_items,
        total_value_usd,
    }
}

fn collection_key(chain_id: &str, contract: &str) -> String {
    format!("{}:{}", chain_id, contract.to_ascii_lowercase())
}

fn short_address(addr: &str) -> String {
    if addr.len() <= 10 {
        return addr.to_string();
    }
    format!("{}…{}", &addr[..6], &addr[addr.len() - 4..])
}

/// Heuristic spam detector. Conservative: only flags clearly-suspect
/// items so we don't hide legitimate art unprompted.
pub fn is_spam(it: &OwnedNft) -> bool {
    let name = it.name.as_deref().unwrap_or("").to_ascii_lowercase();
    let coll = it.collection.as_deref().unwrap_or("").to_ascii_lowercase();
    let combined = format!("{name} {coll}");

    // Classic airdrop-spam signals.
    let bad_phrases = [
        "claim",
        "visit ",
        "airdrop",
        "free mint",
        "reward",
        "$ ",
        "http://",
        "https://",
        ".io ",
        ".xyz ",
        ".com ",
        "voucher",
    ];
    if bad_phrases.iter().any(|p| combined.contains(p)) {
        return true;
    }

    // Unicode look-alike or emoji-only names are also a strong signal.
    if !name.is_empty() && name.chars().all(|c| !c.is_ascii_alphanumeric()) {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nft(
        chain: &str,
        contract: &str,
        token: &str,
        coll: Option<&str>,
        floor: Option<f64>,
        name: Option<&str>,
    ) -> OwnedNft {
        OwnedNft {
            chain_id: chain.into(),
            contract: contract.into(),
            token_id: token.into(),
            name: name.map(String::from),
            collection: coll.map(String::from),
            image: Some(format!("https://img/{token}.png")),
            floor_usd: floor,
        }
    }

    #[test]
    fn groups_by_chain_and_contract() {
        let items = vec![
            nft("eth", "0xAAA", "1", Some("BAYC"), Some(50_000.0), None),
            nft("eth", "0xAAA", "2", Some("BAYC"), Some(48_000.0), None),
            nft("eth", "0xBBB", "10", Some("Doodles"), Some(1_500.0), None),
        ];
        let view = build_gallery(&items, &GalleryFilter::default());
        assert_eq!(view.total_collections, 2);
        assert_eq!(view.total_items, 3);
        assert_eq!(view.collections[0].name, "BAYC");
        assert_eq!(view.collections[0].item_count, 2);
        assert_eq!(view.collections[0].estimated_value_usd, 98_000.0);
    }

    #[test]
    fn sorts_collections_by_value_desc_then_count() {
        let items = vec![
            nft("eth", "0xCheap", "1", Some("Cheap"), Some(10.0), None),
            nft("eth", "0xPricy", "1", Some("Pricy"), Some(5_000.0), None),
        ];
        let view = build_gallery(&items, &GalleryFilter::default());
        assert_eq!(view.collections[0].name, "Pricy");
        assert_eq!(view.collections[1].name, "Cheap");
    }

    #[test]
    fn unpriced_items_count_separately() {
        let items = vec![
            nft("eth", "0xA", "1", Some("Art"), None, None),
            nft("eth", "0xA", "2", Some("Art"), Some(123.0), None),
        ];
        let view = build_gallery(&items, &GalleryFilter::default());
        let g = &view.collections[0];
        assert_eq!(g.item_count, 2);
        assert_eq!(g.unpriced_count, 1);
        assert_eq!(g.estimated_value_usd, 123.0);
    }

    #[test]
    fn collection_falls_back_to_short_address() {
        let items = vec![nft(
            "eth",
            "0x1234567890abcdef1234567890abcdef12345678",
            "1",
            None,
            None,
            None,
        )];
        let view = build_gallery(&items, &GalleryFilter::default());
        assert!(view.collections[0].name.contains("…"));
    }

    #[test]
    fn items_sorted_by_token_id_within_collection() {
        let items = vec![
            nft("eth", "0xA", "10", Some("X"), None, None),
            nft("eth", "0xA", "1", Some("X"), None, None),
            nft("eth", "0xA", "5", Some("X"), None, None),
        ];
        let view = build_gallery(&items, &GalleryFilter::default());
        let tokens: Vec<&str> = view.collections[0]
            .items
            .iter()
            .map(|i| i.token_id.as_str())
            .collect();
        assert_eq!(tokens, vec!["1", "10", "5"]);
    }

    #[test]
    fn hide_spam_diverts_to_hidden_items() {
        let items = vec![
            nft("eth", "0xA", "1", Some("BAYC"), Some(50.0), None),
            nft(
                "eth",
                "0xB",
                "1",
                Some("Visit https://claim-now.xyz"),
                None,
                None,
            ),
        ];
        let view = build_gallery(
            &items,
            &GalleryFilter {
                hide_spam: true,
                ..Default::default()
            },
        );
        assert_eq!(view.total_collections, 1);
        assert_eq!(view.hidden_items.len(), 1);
    }

    #[test]
    fn min_value_filter_drops_low_value_collections() {
        let items = vec![
            nft("eth", "0xA", "1", Some("Big"), Some(1000.0), None),
            nft("eth", "0xB", "1", Some("Small"), Some(5.0), None),
        ];
        let view = build_gallery(
            &items,
            &GalleryFilter {
                min_collection_value_usd: 100.0,
                ..Default::default()
            },
        );
        assert_eq!(view.total_collections, 1);
        assert_eq!(view.collections[0].name, "Big");
    }

    #[test]
    fn chain_filter_includes_only_listed_chains() {
        let items = vec![
            nft("eth", "0xA", "1", Some("E"), Some(10.0), None),
            nft("polygon", "0xB", "1", Some("P"), Some(20.0), None),
        ];
        let view = build_gallery(
            &items,
            &GalleryFilter {
                chains: vec!["polygon".into()],
                ..Default::default()
            },
        );
        assert_eq!(view.total_collections, 1);
        assert_eq!(view.collections[0].chain_id, "polygon");
    }

    #[test]
    fn spam_heuristic_flags_known_patterns() {
        assert!(is_spam(&nft(
            "eth",
            "0xA",
            "1",
            Some("Free $REWARD voucher"),
            None,
            None
        )));
        assert!(is_spam(&nft(
            "eth",
            "0xA",
            "1",
            Some("Coll"),
            None,
            Some("Visit https://x.io now")
        )));
        assert!(!is_spam(&nft(
            "eth",
            "0xA",
            "1",
            Some("Bored Ape Yacht Club"),
            Some(40_000.0),
            Some("BAYC #1234")
        )));
    }

    #[test]
    fn totals_match_visible_collections() {
        let items = vec![
            nft("eth", "0xA", "1", Some("X"), Some(100.0), None),
            nft("eth", "0xA", "2", Some("X"), Some(200.0), None),
            nft("eth", "0xB", "1", Some("Y"), Some(50.0), None),
        ];
        let view = build_gallery(&items, &GalleryFilter::default());
        assert_eq!(view.total_value_usd, 350.0);
        assert_eq!(view.total_items, 3);
        assert_eq!(view.total_collections, 2);
    }

    #[test]
    fn empty_input_yields_empty_view() {
        let view = build_gallery(&[], &GalleryFilter::default());
        assert_eq!(view.total_collections, 0);
        assert_eq!(view.total_items, 0);
        assert_eq!(view.total_value_usd, 0.0);
        assert!(view.collections.is_empty());
        assert!(view.hidden_items.is_empty());
    }

    #[test]
    fn case_insensitive_contract_grouping() {
        let items = vec![
            nft("eth", "0xABCDEF", "1", Some("X"), Some(1.0), None),
            nft("eth", "0xabcdef", "2", Some("X"), Some(1.0), None),
        ];
        let view = build_gallery(&items, &GalleryFilter::default());
        assert_eq!(view.total_collections, 1);
        assert_eq!(view.collections[0].item_count, 2);
    }
}
