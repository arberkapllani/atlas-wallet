//! Portfolio diversification metrics.
//!
//! Given a per-asset USD value list, compute:
//! * each asset's weight (0..1)
//! * the Herfindahl-Hirschman Index (HHI) on the 0..10000 scale a
//!   regulator would recognise: 1 / N (perfectly diversified)
//!   maps to 10000/N, single-asset maps to 10000.
//! * a friendly band (Diversified / Balanced / Concentrated /
//!   HighlyConcentrated) the dashboard can colour.
//! * the largest position's share, useful for a "Your top holding
//!   is X% of the portfolio" callout.
//!
//! All math is f64; scores are clamped into a closed range so a
//! denormalised input can't escape the band.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

#[derive(Debug, Error, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "message")]
pub enum DiversificationError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Holding {
    pub symbol: String,
    pub usd_value: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum DiversificationBand {
    Diversified,
    Balanced,
    Concentrated,
    HighlyConcentrated,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PositionWeight {
    pub symbol: String,
    pub usd_value: f64,
    /// Weight as a fraction of the total, 0..=1.
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DiversificationReport {
    pub total_usd: f64,
    pub position_count: usize,
    /// 0..=10000, single-asset = 10000.
    pub hhi: u32,
    pub band: DiversificationBand,
    pub largest_share: f64,
    pub positions: Vec<PositionWeight>,
}

/// Compute the diversification report. Negative values are
/// rejected; zero values are dropped (closed positions don't
/// affect concentration).
pub fn analyse(holdings: &[Holding]) -> Result<DiversificationReport, DiversificationError> {
    for h in holdings {
        if !h.usd_value.is_finite() {
            return Err(DiversificationError::InvalidInput(format!(
                "{}: non-finite usd_value",
                h.symbol
            )));
        }
        if h.usd_value < 0.0 {
            return Err(DiversificationError::InvalidInput(format!(
                "{}: negative usd_value",
                h.symbol
            )));
        }
    }
    let active: Vec<&Holding> = holdings.iter().filter(|h| h.usd_value > 0.0).collect();
    let total: f64 = active.iter().map(|h| h.usd_value).sum();
    if total <= 0.0 || active.is_empty() {
        return Ok(DiversificationReport {
            total_usd: 0.0,
            position_count: 0,
            hhi: 0,
            band: DiversificationBand::Diversified,
            largest_share: 0.0,
            positions: Vec::new(),
        });
    }

    let mut positions: Vec<PositionWeight> = active
        .iter()
        .map(|h| PositionWeight {
            symbol: h.symbol.clone(),
            usd_value: h.usd_value,
            weight: h.usd_value / total,
        })
        .collect();
    positions.sort_by(|a, b| {
        b.weight
            .partial_cmp(&a.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // HHI in 0..=10000.
    let hhi_f = positions.iter().map(|p| p.weight * p.weight).sum::<f64>() * 10_000.0;
    let hhi = hhi_f.round().clamp(0.0, 10_000.0) as u32;

    let largest_share = positions.first().map(|p| p.weight).unwrap_or(0.0);

    Ok(DiversificationReport {
        total_usd: total,
        position_count: positions.len(),
        hhi,
        band: classify(hhi),
        largest_share,
        positions,
    })
}

fn classify(hhi: u32) -> DiversificationBand {
    // US DoJ-style brackets, adapted for retail portfolios:
    //   < 1500   = competitive market   → Diversified
    //   1500–2500 = moderate           → Balanced
    //   2500–5000 = concentrated       → Concentrated
    //   > 5000    = highly concentrated → HighlyConcentrated
    match hhi {
        0..=1_499 => DiversificationBand::Diversified,
        1_500..=2_499 => DiversificationBand::Balanced,
        2_500..=4_999 => DiversificationBand::Concentrated,
        _ => DiversificationBand::HighlyConcentrated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(symbol: &str, v: f64) -> Holding {
        Holding {
            symbol: symbol.to_string(),
            usd_value: v,
        }
    }

    #[test]
    fn empty_portfolio_returns_zeros() {
        let r = analyse(&[]).unwrap();
        assert_eq!(r.total_usd, 0.0);
        assert_eq!(r.position_count, 0);
        assert_eq!(r.hhi, 0);
        assert_eq!(r.band, DiversificationBand::Diversified);
    }

    #[test]
    fn all_zero_holdings_treated_as_empty() {
        let r = analyse(&[h("BTC", 0.0), h("ETH", 0.0)]).unwrap();
        assert_eq!(r.position_count, 0);
    }

    #[test]
    fn single_asset_is_ten_thousand() {
        let r = analyse(&[h("BTC", 1_000.0)]).unwrap();
        assert_eq!(r.hhi, 10_000);
        assert_eq!(r.band, DiversificationBand::HighlyConcentrated);
        assert_eq!(r.largest_share, 1.0);
    }

    #[test]
    fn two_equal_assets_is_five_thousand() {
        let r = analyse(&[h("BTC", 500.0), h("ETH", 500.0)]).unwrap();
        assert_eq!(r.hhi, 5_000);
        assert_eq!(r.band, DiversificationBand::HighlyConcentrated);
    }

    #[test]
    fn four_equal_assets_is_2500() {
        let r = analyse(&[
            h("BTC", 250.0),
            h("ETH", 250.0),
            h("SOL", 250.0),
            h("ADA", 250.0),
        ])
        .unwrap();
        assert_eq!(r.hhi, 2_500);
        assert_eq!(r.band, DiversificationBand::Concentrated);
    }

    #[test]
    fn ten_equal_assets_is_one_thousand() {
        let holdings: Vec<Holding> = (0..10).map(|i| h(&format!("T{i}"), 100.0)).collect();
        let r = analyse(&holdings).unwrap();
        assert_eq!(r.hhi, 1_000);
        assert_eq!(r.band, DiversificationBand::Diversified);
    }

    #[test]
    fn classify_band_boundaries() {
        assert_eq!(classify(0), DiversificationBand::Diversified);
        assert_eq!(classify(1_499), DiversificationBand::Diversified);
        assert_eq!(classify(1_500), DiversificationBand::Balanced);
        assert_eq!(classify(2_499), DiversificationBand::Balanced);
        assert_eq!(classify(2_500), DiversificationBand::Concentrated);
        assert_eq!(classify(4_999), DiversificationBand::Concentrated);
        assert_eq!(classify(5_000), DiversificationBand::HighlyConcentrated);
        assert_eq!(classify(10_000), DiversificationBand::HighlyConcentrated);
    }

    #[test]
    fn weights_sum_to_one() {
        let r = analyse(&[h("BTC", 700.0), h("ETH", 200.0), h("SOL", 100.0)]).unwrap();
        let sum: f64 = r.positions.iter().map(|p| p.weight).sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }

    #[test]
    fn positions_sorted_by_weight_desc() {
        let r = analyse(&[h("SOL", 100.0), h("BTC", 700.0), h("ETH", 200.0)]).unwrap();
        let symbols: Vec<&str> = r.positions.iter().map(|p| p.symbol.as_str()).collect();
        assert_eq!(symbols, vec!["BTC", "ETH", "SOL"]);
    }

    #[test]
    fn largest_share_matches_top_position() {
        let r = analyse(&[h("BTC", 700.0), h("ETH", 200.0), h("SOL", 100.0)]).unwrap();
        assert!((r.largest_share - 0.7).abs() < 1e-9);
    }

    #[test]
    fn negative_value_is_rejected() {
        let err = analyse(&[h("BTC", -1.0)]).unwrap_err();
        assert!(matches!(err, DiversificationError::InvalidInput(_)));
    }

    #[test]
    fn nan_value_is_rejected() {
        let err = analyse(&[h("BTC", f64::NAN)]).unwrap_err();
        assert!(matches!(err, DiversificationError::InvalidInput(_)));
    }

    #[test]
    fn infinite_value_is_rejected() {
        let err = analyse(&[h("BTC", f64::INFINITY)]).unwrap_err();
        assert!(matches!(err, DiversificationError::InvalidInput(_)));
    }

    #[test]
    fn zero_holdings_dropped_but_active_count_preserved() {
        let r = analyse(&[h("BTC", 100.0), h("DOGE", 0.0), h("ETH", 100.0)]).unwrap();
        assert_eq!(r.position_count, 2);
        assert_eq!(r.hhi, 5_000);
    }

    #[test]
    fn ninety_ten_split_falls_in_high_concentration() {
        let r = analyse(&[h("BTC", 900.0), h("ETH", 100.0)]).unwrap();
        // 0.9^2 + 0.1^2 = 0.82 → 8200.
        assert_eq!(r.hhi, 8_200);
        assert_eq!(r.band, DiversificationBand::HighlyConcentrated);
    }

    #[test]
    fn total_usd_sums_active_positions() {
        let r = analyse(&[h("BTC", 700.0), h("ETH", 200.0), h("SOL", 100.0)]).unwrap();
        assert_eq!(r.total_usd, 1_000.0);
    }
}
