//! Rich transaction-history analytics.
//!
//! `apps/desktop` already persists raw `TxRecord`s in SQLite. This
//! crate adds the *display* layer: filtering by chain / direction /
//! status / date range / free-text, aggregating per-asset totals,
//! and exporting to CSV. Pure data — no DB, no I/O, no dates beyond
//! Unix seconds.
//!
//! `Tx` here is a duplicate-of-shape of `apps::desktop::db::TxRecord`
//! kept independent so this crate doesn't pull in sqlx.

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HistoryError {
    #[error("invalid filter: {0}")]
    InvalidFilter(String),
}

/// Display-shape transaction record.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Tx {
    pub txid: String,
    pub chain_id: String,
    /// `"send"` or `"receive"`.
    pub direction: String,
    pub counterparty: String,
    /// Decimal-string amount in base units.
    pub amount: String,
    pub asset: String,
    /// Decimal-string fee in the chain's native asset base units.
    pub fee: String,
    /// Unix seconds.
    pub timestamp: i64,
    /// `"pending"`, `"confirmed"`, or `"failed"`.
    pub status: String,
    pub memo: Option<String>,
}

/// Filter knobs from the UI.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct HistoryFilter {
    /// If non-empty, only txs on these chains are kept.
    pub chains: Vec<String>,
    /// If set, only txs with this direction (`"send"` / `"receive"`).
    pub direction: Option<String>,
    /// If non-empty, only txs in these statuses.
    pub statuses: Vec<String>,
    /// Inclusive lower bound on timestamp.
    pub since: Option<i64>,
    /// Inclusive upper bound on timestamp.
    pub until: Option<i64>,
    /// Free-text query, matched (case-insensitively) against txid,
    /// counterparty, asset, and memo.
    pub query: Option<String>,
    /// If set, only txs for this asset ticker (case-insensitive).
    pub asset: Option<String>,
}

/// Per-asset summary row.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AssetTotals {
    pub asset: String,
    /// Sum of `amount` across send rows.
    pub sent: String,
    /// Sum of `amount` across receive rows.
    pub received: String,
    /// `received - sent` (may be negative; signed decimal string).
    pub net: String,
    pub send_count: u32,
    pub receive_count: u32,
}

/// Per-chain summary row.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ChainTotals {
    pub chain_id: String,
    pub tx_count: u32,
    pub send_count: u32,
    pub receive_count: u32,
    pub pending_count: u32,
    pub failed_count: u32,
}

/// Result of running `summarise`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct HistorySummary {
    pub total_count: u32,
    pub by_asset: Vec<AssetTotals>,
    pub by_chain: Vec<ChainTotals>,
    /// First and last txs (by timestamp) in the filtered range.
    pub first_timestamp: Option<i64>,
    pub last_timestamp: Option<i64>,
}

/// Apply `HistoryFilter` and return the matching rows in
/// newest-first order (stable: ties broken by txid lexicographic).
pub fn filter_history(rows: &[Tx], filter: &HistoryFilter) -> Vec<Tx> {
    let q = filter
        .query
        .as_ref()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty());
    let asset = filter
        .asset
        .as_ref()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty());

    let mut out: Vec<Tx> = rows
        .iter()
        .filter(|r| {
            if !filter.chains.is_empty() && !filter.chains.iter().any(|c| c == &r.chain_id) {
                return false;
            }
            if let Some(d) = &filter.direction {
                if d != &r.direction {
                    return false;
                }
            }
            if !filter.statuses.is_empty() && !filter.statuses.iter().any(|s| s == &r.status) {
                return false;
            }
            if let Some(since) = filter.since {
                if r.timestamp < since {
                    return false;
                }
            }
            if let Some(until) = filter.until {
                if r.timestamp > until {
                    return false;
                }
            }
            if let Some(a) = &asset {
                if r.asset.to_ascii_lowercase() != *a {
                    return false;
                }
            }
            if let Some(query) = &q {
                let hit = r.txid.to_ascii_lowercase().contains(query)
                    || r.counterparty.to_ascii_lowercase().contains(query)
                    || r.asset.to_ascii_lowercase().contains(query)
                    || r.memo
                        .as_deref()
                        .map(|m| m.to_ascii_lowercase().contains(query))
                        .unwrap_or(false);
                if !hit {
                    return false;
                }
            }
            true
        })
        .cloned()
        .collect();

    out.sort_by(|a, b| {
        b.timestamp
            .cmp(&a.timestamp)
            .then_with(|| a.txid.cmp(&b.txid))
    });
    out
}

/// Aggregate the matching rows into per-asset and per-chain summaries.
pub fn summarise(rows: &[Tx]) -> HistorySummary {
    use std::collections::BTreeMap;

    let mut by_asset_map: BTreeMap<String, AssetTotals> = BTreeMap::new();
    let mut by_chain_map: BTreeMap<String, ChainTotals> = BTreeMap::new();
    let mut first = i64::MAX;
    let mut last = i64::MIN;

    for r in rows {
        first = first.min(r.timestamp);
        last = last.max(r.timestamp);

        let asset = by_asset_map
            .entry(r.asset.clone())
            .or_insert_with(|| AssetTotals {
                asset: r.asset.clone(),
                sent: "0".into(),
                received: "0".into(),
                net: "0".into(),
                send_count: 0,
                receive_count: 0,
            });
        let amt = parse_u128(&r.amount).unwrap_or(0);
        match r.direction.as_str() {
            "send" => {
                asset.sent = add_decimal(&asset.sent, amt);
                asset.send_count += 1;
            }
            "receive" => {
                asset.received = add_decimal(&asset.received, amt);
                asset.receive_count += 1;
            }
            _ => {}
        }
        asset.net = signed_sub(&asset.received, &asset.sent);

        let chain = by_chain_map
            .entry(r.chain_id.clone())
            .or_insert_with(|| ChainTotals {
                chain_id: r.chain_id.clone(),
                tx_count: 0,
                send_count: 0,
                receive_count: 0,
                pending_count: 0,
                failed_count: 0,
            });
        chain.tx_count += 1;
        match r.direction.as_str() {
            "send" => chain.send_count += 1,
            "receive" => chain.receive_count += 1,
            _ => {}
        }
        match r.status.as_str() {
            "pending" => chain.pending_count += 1,
            "failed" => chain.failed_count += 1,
            _ => {}
        }
    }

    HistorySummary {
        total_count: rows.len() as u32,
        by_asset: by_asset_map.into_values().collect(),
        by_chain: by_chain_map.into_values().collect(),
        first_timestamp: if first == i64::MAX { None } else { Some(first) },
        last_timestamp: if last == i64::MIN { None } else { Some(last) },
    }
}

/// Render rows as RFC-4180-ish CSV (header row included). Fields with
/// commas / quotes / newlines are double-quoted with `"` escaped as `""`.
pub fn export_csv(rows: &[Tx]) -> String {
    let mut out = String::new();
    out.push_str("timestamp,chain_id,direction,asset,amount,fee,status,counterparty,txid,memo\n");
    for r in rows {
        out.push_str(&format!("{},", r.timestamp));
        out.push_str(&csv_field(&r.chain_id));
        out.push(',');
        out.push_str(&csv_field(&r.direction));
        out.push(',');
        out.push_str(&csv_field(&r.asset));
        out.push(',');
        out.push_str(&csv_field(&r.amount));
        out.push(',');
        out.push_str(&csv_field(&r.fee));
        out.push(',');
        out.push_str(&csv_field(&r.status));
        out.push(',');
        out.push_str(&csv_field(&r.counterparty));
        out.push(',');
        out.push_str(&csv_field(&r.txid));
        out.push(',');
        out.push_str(&csv_field(r.memo.as_deref().unwrap_or("")));
        out.push('\n');
    }
    out
}

// --- helpers ---------------------------------------------------------

fn csv_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{escaped}\"")
    } else {
        s.to_string()
    }
}

fn parse_u128(s: &str) -> Option<u128> {
    s.parse::<u128>().ok()
}

fn add_decimal(current: &str, delta: u128) -> String {
    let cur = parse_u128(current).unwrap_or(0);
    cur.saturating_add(delta).to_string()
}

fn signed_sub(received: &str, sent: &str) -> String {
    let r = parse_u128(received).unwrap_or(0);
    let s = parse_u128(sent).unwrap_or(0);
    if r >= s {
        (r - s).to_string()
    } else {
        format!("-{}", s - r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn tx(
        txid: &str,
        chain: &str,
        dir: &str,
        amount: &str,
        asset: &str,
        ts: i64,
        status: &str,
        memo: Option<&str>,
    ) -> Tx {
        Tx {
            txid: txid.into(),
            chain_id: chain.into(),
            direction: dir.into(),
            counterparty: format!("addr-{txid}"),
            amount: amount.into(),
            asset: asset.into(),
            fee: "1".into(),
            timestamp: ts,
            status: status.into(),
            memo: memo.map(String::from),
        }
    }

    fn fixture() -> Vec<Tx> {
        vec![
            tx("a1", "eth", "send", "1000", "ETH", 100, "confirmed", None),
            tx(
                "b2",
                "eth",
                "receive",
                "500",
                "ETH",
                200,
                "confirmed",
                Some("salary"),
            ),
            tx("c3", "btc", "send", "20000", "BTC", 150, "pending", None),
            tx("d4", "eth", "send", "300", "USDC", 250, "failed", None),
            tx(
                "e5",
                "btc",
                "receive",
                "5000",
                "BTC",
                300,
                "confirmed",
                None,
            ),
        ]
    }

    #[test]
    fn filter_by_chain() {
        let rows = fixture();
        let f = HistoryFilter {
            chains: vec!["btc".into()],
            ..Default::default()
        };
        let out = filter_history(&rows, &f);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|r| r.chain_id == "btc"));
    }

    #[test]
    fn filter_by_direction_and_status() {
        let rows = fixture();
        let f = HistoryFilter {
            direction: Some("send".into()),
            statuses: vec!["confirmed".into()],
            ..Default::default()
        };
        let out = filter_history(&rows, &f);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].txid, "a1");
    }

    #[test]
    fn filter_by_date_range_inclusive() {
        let rows = fixture();
        let f = HistoryFilter {
            since: Some(150),
            until: Some(250),
            ..Default::default()
        };
        let out = filter_history(&rows, &f);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn filter_by_query_searches_memo_and_counterparty() {
        let rows = fixture();
        let f = HistoryFilter {
            query: Some("salary".into()),
            ..Default::default()
        };
        let out = filter_history(&rows, &f);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].txid, "b2");

        let f2 = HistoryFilter {
            query: Some("ADDR-A1".into()),
            ..Default::default()
        };
        let out2 = filter_history(&rows, &f2);
        assert_eq!(out2.len(), 1);
        assert_eq!(out2[0].txid, "a1");
    }

    #[test]
    fn filter_by_asset_case_insensitive() {
        let rows = fixture();
        let f = HistoryFilter {
            asset: Some("usdc".into()),
            ..Default::default()
        };
        let out = filter_history(&rows, &f);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].asset, "USDC");
    }

    #[test]
    fn filter_returns_newest_first() {
        let rows = fixture();
        let out = filter_history(&rows, &HistoryFilter::default());
        let timestamps: Vec<i64> = out.iter().map(|r| r.timestamp).collect();
        let mut expected = timestamps.clone();
        expected.sort_by(|a, b| b.cmp(a));
        assert_eq!(timestamps, expected);
    }

    #[test]
    fn summarise_per_asset_totals_match() {
        let rows = fixture();
        let s = summarise(&rows);
        let eth = s.by_asset.iter().find(|a| a.asset == "ETH").unwrap();
        assert_eq!(eth.sent, "1000");
        assert_eq!(eth.received, "500");
        assert_eq!(eth.net, "-500");
        assert_eq!(eth.send_count, 1);
        assert_eq!(eth.receive_count, 1);
    }

    #[test]
    fn summarise_per_chain_counts_match() {
        let rows = fixture();
        let s = summarise(&rows);
        let eth = s.by_chain.iter().find(|c| c.chain_id == "eth").unwrap();
        assert_eq!(eth.tx_count, 3);
        assert_eq!(eth.send_count, 2);
        assert_eq!(eth.receive_count, 1);
        assert_eq!(eth.failed_count, 1);

        let btc = s.by_chain.iter().find(|c| c.chain_id == "btc").unwrap();
        assert_eq!(btc.pending_count, 1);
    }

    #[test]
    fn summarise_extremes_match_first_last_timestamp() {
        let s = summarise(&fixture());
        assert_eq!(s.first_timestamp, Some(100));
        assert_eq!(s.last_timestamp, Some(300));
    }

    #[test]
    fn summarise_empty_yields_no_extremes() {
        let s = summarise(&[]);
        assert_eq!(s.first_timestamp, None);
        assert_eq!(s.last_timestamp, None);
        assert_eq!(s.total_count, 0);
    }

    #[test]
    fn export_csv_contains_header_and_rows() {
        let rows = fixture();
        let csv = export_csv(&rows);
        assert!(csv.starts_with(
            "timestamp,chain_id,direction,asset,amount,fee,status,counterparty,txid,memo"
        ));
        assert_eq!(csv.lines().count(), 1 + rows.len());
    }

    #[test]
    fn export_csv_escapes_special_chars() {
        let row = Tx {
            memo: Some("hello, \"world\"\nnext line".into()),
            ..tx("z", "eth", "send", "1", "ETH", 1, "confirmed", None)
        };
        let csv = export_csv(&[row]);
        // Quoted field with escaped quotes.
        assert!(csv.contains("\"hello, \"\"world\"\""));
    }

    #[test]
    fn signed_net_handles_negative() {
        assert_eq!(signed_sub("100", "200"), "-100");
        assert_eq!(signed_sub("200", "200"), "0");
        assert_eq!(signed_sub("300", "100"), "200");
    }
}
