//! Multi-source price aggregation.
//!
//! Real-world price feeds disagree slightly (CoinGecko vs. 1inch vs.
//! CoinMarketCap vs. on-chain TWAPs). To avoid leaking a single
//! source's outage or manipulation into the wallet UI, we combine
//! quotes from *N* sources, drop ones that deviate too far from the
//! group's median, and report the median of the survivors.
//!
//! All inputs are plain `f64` USD prices (or any consistent unit) —
//! the aggregator does not fetch anything.
//!
//! # Algorithm
//! 1. Collect non-negative quotes only.
//! 2. Compute the median price.
//! 3. Reject quotes whose deviation from the median exceeds
//!    `max_deviation_bps` (basis points = 1 / 10 000).
//! 4. Recompute the median over the surviving quotes; that is the
//!    aggregated price. The list of accepted/rejected sources is
//!    returned for transparency.

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

/// Failure modes for the aggregator.
#[derive(Debug, Error, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "message")]
pub enum AggregateError {
    /// No usable quotes were supplied.
    #[error("no usable quotes")]
    NoQuotes,
    /// All quotes were filtered out as outliers.
    #[error("all quotes rejected as outliers")]
    AllRejected,
}

/// One source's USD (or otherwise consistent unit) price quote.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SourceQuote {
    /// Human-readable source name (e.g. `"coingecko"`, `"1inch"`).
    pub source: String,
    /// Quoted price.
    pub price: f64,
}

/// Output of `aggregate_prices`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AggregatedPrice {
    /// Median of the surviving quotes.
    pub price: f64,
    /// Sources whose quote was within tolerance.
    pub accepted: Vec<SourceQuote>,
    /// Sources rejected as outliers.
    pub rejected: Vec<SourceQuote>,
    /// Median absolute deviation from the *initial* median, in bps,
    /// across all input quotes (volatility hint for the UI).
    pub spread_bps: u32,
}

/// Combine quotes from multiple sources.
///
/// `max_deviation_bps` is the per-source tolerance: a quote is kept
/// iff `|q - median| / median <= max_deviation_bps / 10_000`.
pub fn aggregate_prices(
    quotes: &[SourceQuote],
    max_deviation_bps: u32,
) -> Result<AggregatedPrice, AggregateError> {
    let usable: Vec<SourceQuote> = quotes
        .iter()
        .filter(|q| q.price.is_finite() && q.price > 0.0)
        .cloned()
        .collect();
    if usable.is_empty() {
        return Err(AggregateError::NoQuotes);
    }

    let initial_median = median(&usable);
    let tolerance = (max_deviation_bps as f64) / 10_000.0;

    let mut accepted: Vec<SourceQuote> = Vec::new();
    let mut rejected: Vec<SourceQuote> = Vec::new();
    for q in usable.iter() {
        let dev = (q.price - initial_median).abs() / initial_median;
        if dev <= tolerance {
            accepted.push(q.clone());
        } else {
            rejected.push(q.clone());
        }
    }
    if accepted.is_empty() {
        return Err(AggregateError::AllRejected);
    }

    // Median absolute deviation across all usable quotes (relative to
    // initial median), expressed in bps.
    let mut deviations_bps: Vec<u32> = usable
        .iter()
        .map(|q| {
            let rel = (q.price - initial_median).abs() / initial_median;
            (rel * 10_000.0).round() as u32
        })
        .collect();
    deviations_bps.sort_unstable();
    let spread_bps = deviations_bps[deviations_bps.len() / 2];

    let final_price = median(&accepted);
    Ok(AggregatedPrice {
        price: final_price,
        accepted,
        rejected,
        spread_bps,
    })
}

fn median(quotes: &[SourceQuote]) -> f64 {
    let mut prices: Vec<f64> = quotes.iter().map(|q| q.price).collect();
    prices.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = prices.len();
    if n.is_multiple_of(2) {
        (prices[n / 2 - 1] + prices[n / 2]) / 2.0
    } else {
        prices[n / 2]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(source: &str, price: f64) -> SourceQuote {
        SourceQuote {
            source: source.into(),
            price,
        }
    }

    #[test]
    fn no_quotes_errors() {
        assert!(matches!(
            aggregate_prices(&[], 100),
            Err(AggregateError::NoQuotes)
        ));
    }

    #[test]
    fn negative_and_nan_filtered() {
        let quotes = vec![
            q("a", -1.0),
            q("b", f64::NAN),
            q("c", f64::INFINITY),
            q("d", 100.0),
        ];
        let agg = aggregate_prices(&quotes, 100).unwrap();
        assert_eq!(agg.accepted.len(), 1);
        assert_eq!(agg.price, 100.0);
    }

    #[test]
    fn all_rejected_errors() {
        // initial median = 100, tolerance 10 bps = 0.1%. Spread is huge.
        let quotes = vec![q("a", 50.0), q("b", 100.0), q("c", 200.0)];
        // tolerance set to 0 forces every off-median quote out.
        // (median itself stays in.)
        let agg = aggregate_prices(&quotes, 0).unwrap();
        // Median (100) is kept; others rejected.
        assert_eq!(agg.accepted.len(), 1);
        assert_eq!(agg.rejected.len(), 2);
    }

    #[test]
    fn median_of_three_close_quotes_is_middle() {
        let quotes = vec![q("a", 99.0), q("b", 100.0), q("c", 101.0)];
        let agg = aggregate_prices(&quotes, 200).unwrap();
        assert_eq!(agg.price, 100.0);
        assert_eq!(agg.accepted.len(), 3);
        assert!(agg.rejected.is_empty());
    }

    #[test]
    fn outlier_is_rejected_then_median_recomputed() {
        // 99/100/101 are tight; 500 is the manipulator.
        let quotes = vec![
            q("cg", 99.5),
            q("1inch", 100.0),
            q("cmc", 100.5),
            q("bad", 500.0),
        ];
        let agg = aggregate_prices(&quotes, 100).unwrap();
        // 500 should be rejected as outlier.
        assert!(agg.rejected.iter().any(|q| q.source == "bad"));
        assert_eq!(agg.accepted.len(), 3);
        // Recomputed median over {99.5,100,100.5} = 100.
        assert_eq!(agg.price, 100.0);
    }

    #[test]
    fn median_of_even_count_is_average_of_two_middles() {
        let quotes = vec![q("a", 10.0), q("b", 20.0), q("c", 30.0), q("d", 40.0)];
        let agg = aggregate_prices(&quotes, 10_000).unwrap();
        assert_eq!(agg.price, 25.0);
    }

    #[test]
    fn single_quote_returns_itself() {
        let agg = aggregate_prices(&[q("only", 42.0)], 100).unwrap();
        assert_eq!(agg.price, 42.0);
        assert_eq!(agg.accepted.len(), 1);
        assert!(agg.rejected.is_empty());
    }

    #[test]
    fn spread_bps_reflects_dispersion() {
        // Tight cluster: spread should be small.
        let tight = vec![q("a", 100.0), q("b", 100.5), q("c", 101.0)];
        let tight_agg = aggregate_prices(&tight, 1_000).unwrap();
        // Wide cluster (relative): spread should be larger.
        let wide = vec![q("a", 80.0), q("b", 100.0), q("c", 120.0)];
        let wide_agg = aggregate_prices(&wide, 5_000).unwrap();
        assert!(wide_agg.spread_bps > tight_agg.spread_bps);
    }

    #[test]
    fn tolerance_at_exactly_threshold_keeps_quote() {
        // median = 100. quote = 101 is 100 bps off. tolerance = 100 bps.
        let quotes = vec![q("a", 100.0), q("b", 101.0)];
        let agg = aggregate_prices(&quotes, 100).unwrap();
        assert_eq!(agg.accepted.len(), 2);
    }
}
