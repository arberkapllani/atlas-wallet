//! Token approval (allowance) risk analysis.
//!
//! After a user clicks "Approve" on an ERC-20 / ERC-721 / ERC-1155
//! contract, the spender holds permission to move (potentially
//! unlimited) tokens until the user explicitly revokes it. This is
//! the single most common drain vector in EVM ecosystems.
//!
//! This crate takes a snapshot of `Approval` rows (already fetched
//! from a chain indexer) and produces:
//! * A risk score per approval (`Critical` / `High` / `Medium` / `Low`).
//! * A sorted, deduplicated revocation queue suitable for the UI.
//! * Aggregate stats per chain.
//!
//! It is pure data — no RPC, no signing, no I/O.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use specta::Type;

/// Asset standard for an approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum ApprovalKind {
    /// `approve(spender, amount)` on an ERC-20.
    Erc20,
    /// `approve(spender, tokenId)` on an ERC-721.
    Erc721,
    /// `setApprovalForAll(spender, true)` (works for both 721 and 1155).
    ForAll,
}

/// Computed risk tier — UI surfaces these as colored badges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
pub enum RiskLevel {
    /// Low-impact (small fixed allowance, well-known spender).
    Low,
    /// Worth reviewing (e.g. moderate allowance, recent first-seen).
    Medium,
    /// Likely needs revocation (unlimited allowance to a single dApp).
    High,
    /// Almost certainly malicious / unknown spender with broad reach.
    Critical,
}

/// One outstanding approval as seen on-chain.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Approval {
    pub chain_id: String,
    pub token_address: String,
    pub token_symbol: String,
    /// Decimal places (use 0 for ERC-721 / `ForAll`).
    pub token_decimals: u8,
    pub spender_address: String,
    /// Optional curated label — `None` if the spender is unknown.
    pub spender_label: Option<String>,
    /// Decimal-string allowance in base units. For `ForAll` this is
    /// the literal string `"unlimited"`. For ERC-721 single-token
    /// approvals this is the token id.
    pub allowance: String,
    pub kind: ApprovalKind,
    /// Unix seconds — when the approval was first observed.
    pub first_seen: i64,
    /// Unix seconds — when the spender last moved tokens via this
    /// approval. `None` if never used.
    pub last_used: Option<i64>,
}

/// One row in the revoke queue. Keeps the original `Approval` plus
/// the computed risk tier and a short human-readable rationale.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ApprovalRisk {
    pub approval: Approval,
    pub level: RiskLevel,
    pub reasons: Vec<String>,
}

/// Per-chain summary for the dashboard header.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ApprovalSummary {
    pub chain_id: String,
    pub total: u32,
    pub critical: u32,
    pub high: u32,
    pub medium: u32,
    pub low: u32,
    pub unique_spenders: u32,
}

/// Tunables. Defaults are conservative.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ApprovalConfig {
    /// Spenders flagged as known-malicious (lower-cased addresses).
    pub blocklist: Vec<String>,
    /// Spenders that are well-known dApps (lower-cased addresses).
    /// These cap their risk at `High` (unlimited) or `Medium` (finite).
    pub allowlist: Vec<String>,
    /// "Unlimited" allowance threshold in base units. Approvals above
    /// this are considered effectively unlimited even if numerically
    /// finite. Default: `2^200` ≈ 1.6e60.
    pub unlimited_threshold: String,
    /// Approvals last used (or first-seen) longer than this many
    /// seconds ago are considered stale and bumped one tier up.
    pub stale_after_secs: i64,
}

impl Default for ApprovalConfig {
    fn default() -> Self {
        Self {
            blocklist: Vec::new(),
            allowlist: Vec::new(),
            unlimited_threshold: "1606938044258990275541962092341162602522202993782792835301376"
                .to_string(), // 2^200
            stale_after_secs: 60 * 60 * 24 * 180, // 180 days
        }
    }
}

/// Score every approval against `config` and return rows sorted
/// `Critical` first (then High → Medium → Low). Within a tier rows
/// are sorted by chain id then token symbol for stable display.
pub fn analyze(approvals: &[Approval], config: &ApprovalConfig, now: i64) -> Vec<ApprovalRisk> {
    let mut rows: Vec<ApprovalRisk> = approvals.iter().map(|a| score(a, config, now)).collect();
    rows.sort_by(|a, b| {
        b.level
            .cmp(&a.level)
            .then_with(|| a.approval.chain_id.cmp(&b.approval.chain_id))
            .then_with(|| a.approval.token_symbol.cmp(&b.approval.token_symbol))
    });
    rows
}

/// Aggregate per-chain stats for the dashboard.
pub fn summarise(rows: &[ApprovalRisk]) -> Vec<ApprovalSummary> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, ApprovalSummary> = BTreeMap::new();
    let mut spender_sets: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();
    for r in rows {
        let entry = map
            .entry(r.approval.chain_id.clone())
            .or_insert_with(|| ApprovalSummary {
                chain_id: r.approval.chain_id.clone(),
                total: 0,
                critical: 0,
                high: 0,
                medium: 0,
                low: 0,
                unique_spenders: 0,
            });
        entry.total += 1;
        match r.level {
            RiskLevel::Critical => entry.critical += 1,
            RiskLevel::High => entry.high += 1,
            RiskLevel::Medium => entry.medium += 1,
            RiskLevel::Low => entry.low += 1,
        }
        spender_sets
            .entry(r.approval.chain_id.clone())
            .or_default()
            .insert(r.approval.spender_address.to_ascii_lowercase());
    }
    for (chain, set) in spender_sets {
        if let Some(s) = map.get_mut(&chain) {
            s.unique_spenders = set.len() as u32;
        }
    }
    map.into_values().collect()
}

// --- internals -------------------------------------------------------

fn score(a: &Approval, cfg: &ApprovalConfig, now: i64) -> ApprovalRisk {
    let mut reasons = Vec::new();
    let spender = a.spender_address.to_ascii_lowercase();
    let unlimited = is_unlimited(a, cfg);
    let stale = is_stale(a, cfg, now);
    let blocked = cfg
        .blocklist
        .iter()
        .any(|s| s.eq_ignore_ascii_case(&spender));
    let allowed = cfg
        .allowlist
        .iter()
        .any(|s| s.eq_ignore_ascii_case(&spender));
    let unknown_spender = a.spender_label.is_none();

    let mut level = RiskLevel::Low;

    if blocked {
        level = RiskLevel::Critical;
        reasons.push("spender on blocklist".into());
    }

    if unlimited {
        reasons.push("unlimited allowance".into());
        level = level.max(if unknown_spender {
            RiskLevel::Critical
        } else {
            RiskLevel::High
        });
    }

    if matches!(a.kind, ApprovalKind::ForAll) {
        reasons.push("setApprovalForAll grants whole-collection access".into());
        level = level.max(if unknown_spender {
            RiskLevel::Critical
        } else {
            RiskLevel::High
        });
    }

    if unknown_spender && !blocked {
        reasons.push("spender not in curated registry".into());
        level = level.max(RiskLevel::Medium);
    }

    if stale {
        reasons.push("approval has been idle for a long time".into());
        // Bump exactly one tier (capped at Critical).
        level = match level {
            RiskLevel::Low => RiskLevel::Medium,
            RiskLevel::Medium => RiskLevel::High,
            RiskLevel::High => RiskLevel::Critical,
            RiskLevel::Critical => RiskLevel::Critical,
        };
    }

    if allowed && !blocked {
        // Curated allowlist caps risk at High (for unlimited) /
        // Medium (otherwise). Blocklist always wins.
        let cap = if unlimited {
            RiskLevel::High
        } else {
            RiskLevel::Medium
        };
        if level > cap {
            level = cap;
            reasons.push("spender on curated allowlist (risk capped)".into());
        }
    }

    ApprovalRisk {
        approval: a.clone(),
        level,
        reasons,
    }
}

fn is_unlimited(a: &Approval, cfg: &ApprovalConfig) -> bool {
    if matches!(a.kind, ApprovalKind::ForAll) {
        return true;
    }
    if a.allowance.eq_ignore_ascii_case("unlimited") {
        return true;
    }
    // Compare as decimal big-strings via length, then lex.
    let allowance = a.allowance.trim_start_matches('0');
    let threshold = cfg.unlimited_threshold.trim_start_matches('0');
    let allowance = if allowance.is_empty() { "0" } else { allowance };
    let threshold = if threshold.is_empty() { "0" } else { threshold };
    if allowance.len() > threshold.len() {
        return true;
    }
    if allowance.len() < threshold.len() {
        return false;
    }
    allowance >= threshold
}

fn is_stale(a: &Approval, cfg: &ApprovalConfig, now: i64) -> bool {
    let reference = a.last_used.unwrap_or(a.first_seen);
    if reference == 0 {
        return false;
    }
    now.saturating_sub(reference) > cfg.stale_after_secs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approval(
        spender: &str,
        label: Option<&str>,
        allowance: &str,
        kind: ApprovalKind,
        first_seen: i64,
        last_used: Option<i64>,
    ) -> Approval {
        Approval {
            chain_id: "eth".into(),
            token_address: "0xtoken".into(),
            token_symbol: "TKN".into(),
            token_decimals: 18,
            spender_address: spender.into(),
            spender_label: label.map(String::from),
            allowance: allowance.into(),
            kind,
            first_seen,
            last_used,
        }
    }

    #[test]
    fn unlimited_to_unknown_spender_is_critical() {
        let a = approval(
            "0xbeef",
            None,
            "unlimited",
            ApprovalKind::Erc20,
            1_000,
            None,
        );
        let r = analyze(&[a], &ApprovalConfig::default(), 1_000)
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(r.level, RiskLevel::Critical);
    }

    #[test]
    fn unlimited_to_known_spender_is_high() {
        let a = approval(
            "0xuni",
            Some("Uniswap V3 Router"),
            "unlimited",
            ApprovalKind::Erc20,
            1_000,
            None,
        );
        let r = analyze(&[a], &ApprovalConfig::default(), 1_000)
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(r.level, RiskLevel::High);
    }

    #[test]
    fn finite_allowance_known_spender_is_low() {
        let a = approval(
            "0xuni",
            Some("Uniswap V3 Router"),
            "1000",
            ApprovalKind::Erc20,
            1_000,
            Some(1_000),
        );
        let r = analyze(&[a], &ApprovalConfig::default(), 1_000)
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(r.level, RiskLevel::Low);
    }

    #[test]
    fn for_all_unknown_spender_is_critical() {
        let a = approval("0xx", None, "0", ApprovalKind::ForAll, 1_000, None);
        let r = analyze(&[a], &ApprovalConfig::default(), 1_000)
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(r.level, RiskLevel::Critical);
    }

    #[test]
    fn blocklist_overrides_to_critical() {
        let cfg = ApprovalConfig {
            blocklist: vec!["0xbad".into()],
            ..Default::default()
        };
        let a = approval(
            "0xbad",
            Some("Pretend Legit"),
            "1",
            ApprovalKind::Erc20,
            1_000,
            None,
        );
        let r = analyze(&[a], &cfg, 1_000).into_iter().next().unwrap();
        assert_eq!(r.level, RiskLevel::Critical);
    }

    #[test]
    fn allowlist_caps_unlimited_at_high() {
        let cfg = ApprovalConfig {
            allowlist: vec!["0xuni".into()],
            ..Default::default()
        };
        let a = approval(
            "0xuni",
            Some("Uniswap V3 Router"),
            "unlimited",
            ApprovalKind::Erc20,
            1_000,
            None,
        );
        let r = analyze(&[a], &cfg, 1_000).into_iter().next().unwrap();
        assert_eq!(r.level, RiskLevel::High);
    }

    #[test]
    fn stale_finite_low_becomes_medium() {
        let cfg = ApprovalConfig::default();
        let now = 1_000_000_000;
        let stale_first_seen = now - cfg.stale_after_secs - 1;
        let a = approval(
            "0xuni",
            Some("Uniswap V3 Router"),
            "1000",
            ApprovalKind::Erc20,
            stale_first_seen,
            None,
        );
        let r = analyze(&[a], &cfg, now).into_iter().next().unwrap();
        assert_eq!(r.level, RiskLevel::Medium);
    }

    #[test]
    fn unknown_spender_finite_is_medium() {
        let a = approval(
            "0xnew",
            None,
            "100",
            ApprovalKind::Erc20,
            1_000,
            Some(1_000),
        );
        let r = analyze(&[a], &ApprovalConfig::default(), 1_000)
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(r.level, RiskLevel::Medium);
    }

    #[test]
    fn unlimited_threshold_treats_max_uint256_as_unlimited() {
        // 2^256 - 1
        let max = "115792089237316195423570985008687907853269984665640564039457584007913129639935";
        let a = approval(
            "0xuni",
            Some("Router"),
            max,
            ApprovalKind::Erc20,
            1_000,
            None,
        );
        let r = analyze(&[a], &ApprovalConfig::default(), 1_000)
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(r.level, RiskLevel::High);
    }

    #[test]
    fn analyze_sorts_critical_first() {
        let cfg = ApprovalConfig::default();
        let crit = approval("0xa", None, "unlimited", ApprovalKind::Erc20, 1_000, None);
        let low = approval(
            "0xuni",
            Some("Router"),
            "1",
            ApprovalKind::Erc20,
            1_000,
            Some(1_000),
        );
        let rows = analyze(&[low, crit], &cfg, 1_000);
        assert_eq!(rows[0].level, RiskLevel::Critical);
        assert_eq!(rows[1].level, RiskLevel::Low);
    }

    #[test]
    fn summarise_groups_by_chain_and_counts_unique_spenders() {
        let cfg = ApprovalConfig::default();
        let rows = analyze(
            &[
                approval("0xa", None, "unlimited", ApprovalKind::Erc20, 1_000, None),
                approval("0xa", None, "1", ApprovalKind::Erc20, 1_000, Some(1_000)),
                approval("0xb", None, "0", ApprovalKind::ForAll, 1_000, None),
            ],
            &cfg,
            1_000,
        );
        let s = summarise(&rows);
        let eth = s.iter().find(|s| s.chain_id == "eth").unwrap();
        assert_eq!(eth.total, 3);
        assert_eq!(eth.unique_spenders, 2);
    }

    #[test]
    fn allowlist_is_overridden_by_blocklist() {
        let cfg = ApprovalConfig {
            blocklist: vec!["0xshared".into()],
            allowlist: vec!["0xshared".into()],
            ..Default::default()
        };
        let a = approval(
            "0xshared",
            Some("Shared"),
            "1",
            ApprovalKind::Erc20,
            1_000,
            Some(1_000),
        );
        let r = analyze(&[a], &cfg, 1_000).into_iter().next().unwrap();
        assert_eq!(r.level, RiskLevel::Critical);
    }
}
