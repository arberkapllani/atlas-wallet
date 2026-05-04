//! UTXO labelling, mix detection and privacy-aware coin selection.
//!
//! Even with Tor active and seed-only sourcing, on-chain analytics
//! (Chainalysis, Elliptic, etc.) can still link addresses by
//! looking at how UTXOs are combined inside a transaction. The
//! single most damaging mistake is to spend a "KYC-tainted" UTXO
//! together with a "private" UTXO in the same transaction — the
//! common-input-ownership heuristic immediately marries them, and
//! the privacy of the second one is destroyed.
//!
//! This crate is **offline-pure**:
//!
//! - [`Origin`] tags every UTXO with where the coins came from.
//! - [`detect_mix`] inspects a selected set and returns a
//!   [`MixWarning`] when buckets that should never share a
//!   transaction are about to be combined.
//! - [`suggest_selection`] chooses UTXOs from a candidate pool
//!   honouring privacy buckets first, then a configurable
//!   strategy.
//! - [`UtxoLabelStore`] is a tiny JSON-friendly map from
//!   [`UtxoRef`] to [`UtxoLabel`] that the host persists at
//!   `data_dir/utxo_labels.json`.
//!
//! Selection is intentionally simple — Atlas treats this as a
//! "best advisor" rather than a wallet-grade UTXO planner. The
//! actual transaction builder still does fee / change accounting.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Where a UTXO came from. The bucket determines what it can
/// safely be combined with.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Origin not yet labelled. Treated as `Mixed`-equivalent for
    /// safety — the user should label it before spending.
    #[default]
    Unknown,
    /// Came from a KYC source (centralised exchange withdrawal,
    /// regulated on-ramp, custodial wallet). The user's real-world
    /// identity is tied to this UTXO forever.
    KycTainted,
    /// Came from a non-KYC P2P trade (Bisq, RoboSats, Hodl Hodl,
    /// LocalMonero etc.). No off-chain identity link, but a
    /// counterparty knows one of the addresses.
    P2p,
    /// Came from mining or a similar self-generated source. No
    /// counterparty involved.
    Mining,
    /// Output of a CoinJoin / WabiSabi / payjoin round. The
    /// privacy bucket — must never be re-merged with a KYC input.
    Private,
    /// User accepted the coin as direct payment from a non-KYC
    /// peer who knows them by name (donation button, friend).
    Donation,
}

impl Origin {
    /// `true` if this origin links the holder to a real-world
    /// identity (KYC) and therefore taints any other input it is
    /// combined with.
    pub fn is_identifying(self) -> bool {
        matches!(self, Origin::KycTainted | Origin::Donation)
    }

    /// `true` if this origin is the "high privacy" bucket whose
    /// anonymity is destroyed when merged with an identifying
    /// origin.
    pub fn is_private(self) -> bool {
        matches!(self, Origin::Private)
    }
}

/// Reference to a single UTXO. `txid` is hex (lowercase, 64 chars
/// for Bitcoin); `vout` is the 0-based output index.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
pub struct UtxoRef {
    /// Transaction id (hex, lowercase).
    pub txid: String,
    /// 0-based output index inside that transaction.
    pub vout: u32,
}

/// User-attached metadata for a UTXO. Persisted in
/// `data_dir/utxo_labels.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct UtxoLabel {
    /// Provenance bucket.
    pub origin: Origin,
    /// Optional free-form note shown in the UI (e.g. "RoboSats
    /// trade 2026-01", "Whirlpool round 137").
    #[serde(default)]
    pub note: String,
    /// Optional user-defined tags. Useful for grouping UTXOs by
    /// purpose (`["savings", "cold"]`).
    #[serde(default)]
    pub tags: Vec<String>,
}

/// A candidate for coin selection: UTXO ref + value + label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct LabeledUtxo {
    /// Outpoint reference.
    pub utxo: UtxoRef,
    /// Value in base units (sats for Bitcoin, gwei is N/A —
    /// account-based chains don't use this crate).
    pub value: u64,
    /// Optional confirmations. `None` means unknown / unconfirmed.
    #[serde(default)]
    pub confirmations: Option<u32>,
    /// Provenance + notes.
    pub label: UtxoLabel,
}

/// Reasons [`detect_mix`] flags a selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MixWarning {
    /// At least one identifying input (KYC) and one private input
    /// would share the transaction. Spending both together breaks
    /// the privacy of the private one.
    KycMeetsPrivate {
        /// Outpoints flagged as KYC.
        kyc: Vec<UtxoRef>,
        /// Outpoints flagged as Private.
        private: Vec<UtxoRef>,
    },
    /// At least one identifying input is being combined with an
    /// otherwise-anonymous P2P input. Less severe than the above —
    /// only the P2P counterparty's view is widened.
    KycMeetsP2p {
        /// Outpoints flagged as KYC.
        kyc: Vec<UtxoRef>,
        /// Outpoints flagged as P2P.
        p2p: Vec<UtxoRef>,
    },
    /// Selection contains UTXOs whose origin has not yet been
    /// labelled. The user should triage them before spending.
    UnknownOrigin {
        /// Unlabelled outpoints.
        unknown: Vec<UtxoRef>,
    },
}

/// Inspect a selected set of inputs and return every privacy
/// concern that applies. An empty vec means the selection is
/// internally consistent.
pub fn detect_mix(selected: &[LabeledUtxo]) -> Vec<MixWarning> {
    let mut kyc: Vec<UtxoRef> = Vec::new();
    let mut p2p: Vec<UtxoRef> = Vec::new();
    let mut private: Vec<UtxoRef> = Vec::new();
    let mut unknown: Vec<UtxoRef> = Vec::new();

    for u in selected {
        match u.label.origin {
            Origin::KycTainted | Origin::Donation => kyc.push(u.utxo.clone()),
            Origin::P2p => p2p.push(u.utxo.clone()),
            Origin::Private => private.push(u.utxo.clone()),
            Origin::Unknown => unknown.push(u.utxo.clone()),
            Origin::Mining => {}
        }
    }

    let mut out = Vec::new();
    if !kyc.is_empty() && !private.is_empty() {
        out.push(MixWarning::KycMeetsPrivate {
            kyc: kyc.clone(),
            private,
        });
    }
    if !kyc.is_empty() && !p2p.is_empty() {
        out.push(MixWarning::KycMeetsP2p { kyc, p2p });
    }
    if !unknown.is_empty() {
        out.push(MixWarning::UnknownOrigin { unknown });
    }
    out
}

/// Strategy for [`suggest_selection`] when more than one
/// privacy-safe combination satisfies the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum SelectionStrategy {
    /// Pick smallest UTXOs first. Consolidates dust over time but
    /// produces larger transactions and higher fees.
    SmallestFirst,
    /// Pick largest UTXOs first. Cheapest in fees but creates
    /// larger leftover change.
    #[default]
    LargestFirst,
    /// Greedily pick UTXOs whose individual value is closest to
    /// the remaining target. Tends to minimise change leftover.
    BranchAndBound,
}

/// Result of [`suggest_selection`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Selection {
    /// Outpoints chosen by the algorithm, in the order they were
    /// added.
    pub inputs: Vec<LabeledUtxo>,
    /// Sum of `inputs`. Always `>= target` on success.
    pub total_value: u64,
    /// Privacy bucket the algorithm restricted itself to.
    pub bucket: PrivacyBucket,
}

/// Coarse partition of UTXOs into privacy buckets. Selection runs
/// independently in each bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyBucket {
    /// `Origin::Private` only.
    Private,
    /// `Origin::P2p` and `Origin::Mining` — high anonymity, no
    /// real-world identity link.
    Anonymous,
    /// `Origin::KycTainted` and `Origin::Donation` — already
    /// linked to identity.
    Identifying,
    /// `Origin::Unknown` — not yet triaged.
    Unlabelled,
}

impl PrivacyBucket {
    /// The bucket that owns this origin tag.
    pub fn of(origin: Origin) -> Self {
        match origin {
            Origin::Private => Self::Private,
            Origin::P2p | Origin::Mining => Self::Anonymous,
            Origin::KycTainted | Origin::Donation => Self::Identifying,
            Origin::Unknown => Self::Unlabelled,
        }
    }
}

/// Errors [`suggest_selection`] can return.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SelectionError {
    /// `target` is zero — caller bug.
    #[error("target value must be greater than zero")]
    ZeroTarget,
    /// No bucket has enough total value to fund `target` on its
    /// own. The user must label more UTXOs or accept a mix
    /// (handled by the host with explicit confirmation).
    #[error(
        "no privacy-safe bucket has enough funds (largest single-bucket total = {largest_bucket_total} sats)"
    )]
    Insufficient {
        /// Largest sum any single bucket could cover.
        largest_bucket_total: u64,
    },
}

/// Pick a privacy-safe combination of UTXOs that covers `target`.
///
/// Buckets are tried in this order: Private → Anonymous →
/// Unlabelled → Identifying. The first bucket whose total >=
/// `target` is used; selection then runs `strategy` inside that
/// bucket. The returned [`Selection`] is guaranteed to be
/// **single-bucket**, so [`detect_mix`] on the result will be
/// empty for the KYC/Private and KYC/P2P checks (it may still
/// flag `UnknownOrigin` if the only viable bucket was
/// `Unlabelled`).
pub fn suggest_selection(
    target: u64,
    available: &[LabeledUtxo],
    strategy: SelectionStrategy,
) -> Result<Selection, SelectionError> {
    if target == 0 {
        return Err(SelectionError::ZeroTarget);
    }

    let order = [
        PrivacyBucket::Private,
        PrivacyBucket::Anonymous,
        PrivacyBucket::Unlabelled,
        PrivacyBucket::Identifying,
    ];

    let mut largest_bucket_total: u64 = 0;
    for bucket in order {
        let pool: Vec<&LabeledUtxo> = available
            .iter()
            .filter(|u| PrivacyBucket::of(u.label.origin) == bucket)
            .collect();
        let total: u64 = pool.iter().map(|u| u.value).sum();
        if total > largest_bucket_total {
            largest_bucket_total = total;
        }
        if total < target {
            continue;
        }
        let inputs = pick(&pool, target, strategy);
        let total_value: u64 = inputs.iter().map(|u| u.value).sum();
        return Ok(Selection {
            inputs,
            total_value,
            bucket,
        });
    }
    Err(SelectionError::Insufficient {
        largest_bucket_total,
    })
}

/// Inner selection algorithm. Operates on an already-bucketed
/// pool. Returns owned `LabeledUtxo`s.
fn pick(pool: &[&LabeledUtxo], target: u64, strategy: SelectionStrategy) -> Vec<LabeledUtxo> {
    let mut sorted: Vec<&LabeledUtxo> = pool.to_vec();
    match strategy {
        SelectionStrategy::SmallestFirst => sorted.sort_by_key(|u| u.value),
        SelectionStrategy::LargestFirst => sorted.sort_by_key(|u| std::cmp::Reverse(u.value)),
        SelectionStrategy::BranchAndBound => {
            // Cheap heuristic: sort by absolute distance to remaining
            // target each step. Recomputed via repeated min-search;
            // pool sizes here are tiny (< 10k) so this is fine.
            return branch_and_bound(&sorted, target);
        }
    }
    let mut total: u64 = 0;
    let mut chosen: Vec<LabeledUtxo> = Vec::new();
    for u in sorted {
        if total >= target {
            break;
        }
        chosen.push((*u).clone());
        total = total.saturating_add(u.value);
    }
    chosen
}

fn branch_and_bound(pool: &[&LabeledUtxo], target: u64) -> Vec<LabeledUtxo> {
    let mut remaining = target;
    let mut chosen: Vec<LabeledUtxo> = Vec::new();
    let mut taken: BTreeSet<UtxoRef> = BTreeSet::new();
    while remaining > 0 {
        let mut best: Option<&&LabeledUtxo> = None;
        let mut best_distance: u64 = u64::MAX;
        for u in pool.iter() {
            if taken.contains(&u.utxo) {
                continue;
            }
            let distance = u.value.abs_diff(remaining);
            if distance < best_distance {
                best_distance = distance;
                best = Some(u);
            }
        }
        let Some(pick) = best else { break };
        chosen.push((*pick).clone());
        taken.insert(pick.utxo.clone());
        if pick.value >= remaining {
            break;
        }
        remaining -= pick.value;
    }
    chosen
}

/// JSON-friendly persistent label store. Maps a stringified
/// outpoint (`"<txid>:<vout>"`) to its [`UtxoLabel`]. Stored as a
/// plain map so manual inspection of `utxo_labels.json` is easy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UtxoLabelStore {
    /// Internal map keyed by `"<txid>:<vout>"`.
    pub labels: BTreeMap<String, UtxoLabel>,
}

/// Tuple-style entry surfaced to the IPC layer (so specta can
/// generate a typed pair without exposing the internal map key
/// format).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct LabeledUtxoEntry {
    /// Outpoint reference.
    pub utxo: UtxoRef,
    /// Label currently stored for this outpoint.
    pub label: UtxoLabel,
}

fn key(r: &UtxoRef) -> String {
    format!("{}:{}", r.txid, r.vout)
}

impl UtxoLabelStore {
    /// Get the label for `r`, if one was set. Returns `None`
    /// (which the host should treat as [`Origin::Unknown`]) if
    /// untagged.
    pub fn get(&self, r: &UtxoRef) -> Option<&UtxoLabel> {
        self.labels.get(&key(r))
    }

    /// Insert or replace the label for `r`.
    pub fn upsert(&mut self, r: UtxoRef, label: UtxoLabel) {
        self.labels.insert(key(&r), label);
    }

    /// Remove the label for `r`. Returns `true` if a label was
    /// present.
    pub fn remove(&mut self, r: &UtxoRef) -> bool {
        self.labels.remove(&key(r)).is_some()
    }

    /// All `(ref, label)` pairs in stable lexicographic order.
    pub fn entries(&self) -> Vec<(UtxoRef, UtxoLabel)> {
        self.labels
            .iter()
            .filter_map(|(k, v)| {
                let (txid, vout) = k.rsplit_once(':')?;
                let vout: u32 = vout.parse().ok()?;
                Some((
                    UtxoRef {
                        txid: txid.to_string(),
                        vout,
                    },
                    v.clone(),
                ))
            })
            .collect()
    }

    /// Number of labelled outpoints.
    pub fn len(&self) -> usize {
        self.labels.len()
    }

    /// `true` if no outpoints are labelled.
    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }

    /// Annotate every UTXO in `pool` with the label stored for
    /// it (or [`UtxoLabel::default`] when absent). Useful for
    /// host integration where the chain provider returns raw
    /// UTXOs and the store provides labels separately.
    pub fn annotate(&self, pool: &[(UtxoRef, u64, Option<u32>)]) -> Vec<LabeledUtxo> {
        pool.iter()
            .map(|(r, v, c)| LabeledUtxo {
                utxo: r.clone(),
                value: *v,
                confirmations: *c,
                label: self.get(r).cloned().unwrap_or_default(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(txid: &str, vout: u32, value: u64, origin: Origin) -> LabeledUtxo {
        LabeledUtxo {
            utxo: UtxoRef {
                txid: txid.into(),
                vout,
            },
            value,
            confirmations: Some(6),
            label: UtxoLabel {
                origin,
                ..Default::default()
            },
        }
    }

    #[test]
    fn detect_mix_empty_for_uniform_set() {
        let s = vec![
            u("aa", 0, 1000, Origin::Private),
            u("bb", 0, 2000, Origin::Private),
        ];
        assert!(detect_mix(&s).is_empty());
    }

    #[test]
    fn detect_mix_flags_kyc_with_private() {
        let s = vec![
            u("aa", 0, 1000, Origin::KycTainted),
            u("bb", 0, 2000, Origin::Private),
        ];
        let w = detect_mix(&s);
        assert!(matches!(w[0], MixWarning::KycMeetsPrivate { .. }));
    }

    #[test]
    fn detect_mix_flags_kyc_with_p2p() {
        let s = vec![
            u("aa", 0, 1000, Origin::KycTainted),
            u("bb", 0, 2000, Origin::P2p),
        ];
        let w = detect_mix(&s);
        assert!(w
            .iter()
            .any(|x| matches!(x, MixWarning::KycMeetsP2p { .. })));
    }

    #[test]
    fn detect_mix_flags_unknown() {
        let s = vec![
            u("aa", 0, 1000, Origin::P2p),
            u("bb", 0, 2000, Origin::Unknown),
        ];
        let w = detect_mix(&s);
        assert!(w
            .iter()
            .any(|x| matches!(x, MixWarning::UnknownOrigin { .. })));
    }

    #[test]
    fn detect_mix_emits_multiple_warnings_at_once() {
        let s = vec![
            u("aa", 0, 1000, Origin::KycTainted),
            u("bb", 0, 1000, Origin::Private),
            u("cc", 0, 1000, Origin::P2p),
            u("dd", 0, 1000, Origin::Unknown),
        ];
        let w = detect_mix(&s);
        // KycMeetsPrivate + KycMeetsP2p + UnknownOrigin
        assert_eq!(w.len(), 3);
    }

    #[test]
    fn donation_counts_as_identifying() {
        let s = vec![
            u("aa", 0, 1000, Origin::Donation),
            u("bb", 0, 2000, Origin::Private),
        ];
        let w = detect_mix(&s);
        assert!(w
            .iter()
            .any(|x| matches!(x, MixWarning::KycMeetsPrivate { .. })));
    }

    #[test]
    fn select_prefers_private_bucket_when_funded() {
        let pool = vec![
            u("aa", 0, 100_000, Origin::KycTainted),
            u("bb", 0, 60_000, Origin::Private),
            u("cc", 0, 60_000, Origin::Private),
        ];
        let s = suggest_selection(100_000, &pool, SelectionStrategy::LargestFirst).unwrap();
        assert_eq!(s.bucket, PrivacyBucket::Private);
        assert_eq!(s.total_value, 120_000);
        assert!(detect_mix(&s.inputs).is_empty());
    }

    #[test]
    fn select_falls_back_to_anonymous_when_private_short() {
        let pool = vec![
            u("aa", 0, 30_000, Origin::Private),
            u("bb", 0, 50_000, Origin::P2p),
            u("cc", 0, 60_000, Origin::P2p),
        ];
        let s = suggest_selection(100_000, &pool, SelectionStrategy::LargestFirst).unwrap();
        assert_eq!(s.bucket, PrivacyBucket::Anonymous);
        assert!(s.total_value >= 100_000);
    }

    #[test]
    fn select_uses_unlabelled_before_kyc() {
        let pool = vec![
            u("aa", 0, 60_000, Origin::Unknown),
            u("bb", 0, 60_000, Origin::Unknown),
            u("cc", 0, 200_000, Origin::KycTainted),
        ];
        let s = suggest_selection(100_000, &pool, SelectionStrategy::LargestFirst).unwrap();
        assert_eq!(s.bucket, PrivacyBucket::Unlabelled);
    }

    #[test]
    fn select_falls_back_to_kyc_only_when_nothing_else_works() {
        let pool = vec![
            u("aa", 0, 30_000, Origin::P2p),
            u("bb", 0, 200_000, Origin::KycTainted),
        ];
        let s = suggest_selection(100_000, &pool, SelectionStrategy::LargestFirst).unwrap();
        assert_eq!(s.bucket, PrivacyBucket::Identifying);
    }

    #[test]
    fn select_zero_target_errors() {
        let pool = vec![u("aa", 0, 1000, Origin::Private)];
        assert_eq!(
            suggest_selection(0, &pool, SelectionStrategy::LargestFirst),
            Err(SelectionError::ZeroTarget)
        );
    }

    #[test]
    fn select_insufficient_reports_largest_bucket() {
        let pool = vec![
            u("aa", 0, 10_000, Origin::Private),
            u("bb", 0, 20_000, Origin::P2p),
            u("cc", 0, 30_000, Origin::KycTainted),
        ];
        let err = suggest_selection(100_000, &pool, SelectionStrategy::LargestFirst).unwrap_err();
        assert_eq!(
            err,
            SelectionError::Insufficient {
                largest_bucket_total: 30_000
            }
        );
    }

    #[test]
    fn select_smallest_first_consolidates() {
        let pool = vec![
            u("aa", 0, 10_000, Origin::Private),
            u("bb", 0, 10_000, Origin::Private),
            u("cc", 0, 100_000, Origin::Private),
        ];
        let s = suggest_selection(15_000, &pool, SelectionStrategy::SmallestFirst).unwrap();
        // Should pick the two 10k UTXOs, not the 100k one.
        assert_eq!(s.inputs.len(), 2);
        assert_eq!(s.total_value, 20_000);
    }

    #[test]
    fn select_branch_and_bound_minimises_change() {
        let pool = vec![
            u("aa", 0, 33_000, Origin::Private),
            u("bb", 0, 67_000, Origin::Private),
            u("cc", 0, 200_000, Origin::Private),
        ];
        let s = suggest_selection(100_000, &pool, SelectionStrategy::BranchAndBound).unwrap();
        // 33k + 67k = exact match.
        assert_eq!(s.total_value, 100_000);
        assert_eq!(s.inputs.len(), 2);
    }

    #[test]
    fn store_round_trips() {
        let mut s = UtxoLabelStore::default();
        let r = UtxoRef {
            txid: "deadbeef".into(),
            vout: 1,
        };
        assert!(s.get(&r).is_none());
        assert!(s.is_empty());
        s.upsert(
            r.clone(),
            UtxoLabel {
                origin: Origin::Private,
                note: "Whirlpool round 1".into(),
                tags: vec!["savings".into()],
            },
        );
        assert_eq!(s.len(), 1);
        let got = s.get(&r).unwrap();
        assert_eq!(got.origin, Origin::Private);
        assert_eq!(got.note, "Whirlpool round 1");
        let json = serde_json::to_string(&s).unwrap();
        let back: UtxoLabelStore = serde_json::from_str(&json).unwrap();
        assert_eq!(back.get(&r).unwrap().origin, Origin::Private);
        assert!(s.remove(&r));
        assert!(!s.remove(&r));
    }

    #[test]
    fn store_annotates_pool_with_default_for_missing() {
        let mut s = UtxoLabelStore::default();
        s.upsert(
            UtxoRef {
                txid: "aa".into(),
                vout: 0,
            },
            UtxoLabel {
                origin: Origin::Private,
                ..Default::default()
            },
        );
        let pool = vec![
            (
                UtxoRef {
                    txid: "aa".into(),
                    vout: 0,
                },
                100,
                Some(6),
            ),
            (
                UtxoRef {
                    txid: "bb".into(),
                    vout: 0,
                },
                200,
                None,
            ),
        ];
        let annotated = s.annotate(&pool);
        assert_eq!(annotated[0].label.origin, Origin::Private);
        assert_eq!(annotated[1].label.origin, Origin::Unknown);
    }

    #[test]
    fn origin_helpers_classify_correctly() {
        assert!(Origin::KycTainted.is_identifying());
        assert!(Origin::Donation.is_identifying());
        assert!(!Origin::P2p.is_identifying());
        assert!(Origin::Private.is_private());
        assert!(!Origin::Mining.is_private());
    }
}
