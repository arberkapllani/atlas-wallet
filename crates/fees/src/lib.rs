//! Gas / fee estimator.
//!
//! - **EIP-1559** (post-London EVM): given the latest base-fee and
//!   a recent fee history, derive a `(maxPriorityFeePerGas,
//!   maxFeePerGas)` pair for Slow / Standard / Fast tiers, plus a
//!   "bumped" pair for replacing a stuck transaction (≥ 10% bump
//!   on both, per geth replacement rules).
//!
//! - **Bitcoin / UTXO**: convert a `sat/vB` rate to a total fee in
//!   sats given a virtual-byte size estimate.
//!
//! Pure data, all integer math, no I/O.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

#[derive(Debug, Error, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "message")]
pub enum FeeError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

/// Speed tier the user picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum FeeTier {
    Slow,
    Standard,
    Fast,
}

/// EIP-1559 fee suggestion (all in wei).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Eip1559Suggestion {
    pub tier: FeeTier,
    pub max_priority_fee_per_gas: u128,
    pub max_fee_per_gas: u128,
}

/// Output of `eip1559_suggest`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Eip1559Suggestions {
    pub slow: Eip1559Suggestion,
    pub standard: Eip1559Suggestion,
    pub fast: Eip1559Suggestion,
}

/// Recent priority-fee samples in wei (one per recent block).
pub type PriorityHistory = Vec<u128>;

/// Suggest a Slow / Standard / Fast triple.
///
/// `base_fee_per_gas` is the latest block's base fee in wei.
/// `recent_priority_fees` is a list of priority-fee samples (e.g.
/// the 50th percentile per block from `eth_feeHistory`); empty list
/// falls back to a 1-gwei floor.
pub fn eip1559_suggest(
    base_fee_per_gas: u128,
    recent_priority_fees: &[u128],
) -> Result<Eip1559Suggestions, FeeError> {
    if base_fee_per_gas > u128::MAX / 4 {
        return Err(FeeError::InvalidInput("base_fee overflow".into()));
    }

    let one_gwei: u128 = 1_000_000_000;
    let p_slow = percentile(recent_priority_fees, 25).unwrap_or(one_gwei);
    let p_std = percentile(recent_priority_fees, 50).unwrap_or(one_gwei);
    let p_fast = percentile(recent_priority_fees, 90).unwrap_or(2 * one_gwei);

    // maxFee = 2 * baseFee + tip (geth default).
    let max_for = |tip: u128| -> u128 { base_fee_per_gas.saturating_mul(2).saturating_add(tip) };

    Ok(Eip1559Suggestions {
        slow: Eip1559Suggestion {
            tier: FeeTier::Slow,
            max_priority_fee_per_gas: p_slow,
            max_fee_per_gas: max_for(p_slow),
        },
        standard: Eip1559Suggestion {
            tier: FeeTier::Standard,
            max_priority_fee_per_gas: p_std,
            max_fee_per_gas: max_for(p_std),
        },
        fast: Eip1559Suggestion {
            tier: FeeTier::Fast,
            max_priority_fee_per_gas: p_fast,
            max_fee_per_gas: max_for(p_fast),
        },
    })
}

/// Apply the geth ≥10% replacement rule to both the priority fee
/// and the max fee. Always rounds up.
pub fn bump_for_replacement(s: &Eip1559Suggestion) -> Eip1559Suggestion {
    let bump = |x: u128| -> u128 {
        // ceil(x * 11 / 10).
        x.saturating_mul(11).saturating_add(9) / 10
    };
    Eip1559Suggestion {
        tier: s.tier,
        max_priority_fee_per_gas: bump(s.max_priority_fee_per_gas),
        max_fee_per_gas: bump(s.max_fee_per_gas),
    }
}

/// Total UTXO fee in sats given a sat/vB rate and a vsize estimate.
pub fn utxo_fee_sats(sat_per_vbyte: u64, vsize: u64) -> Result<u64, FeeError> {
    if sat_per_vbyte == 0 {
        return Err(FeeError::InvalidInput("sat_per_vbyte = 0".into()));
    }
    if vsize == 0 {
        return Err(FeeError::InvalidInput("vsize = 0".into()));
    }
    sat_per_vbyte
        .checked_mul(vsize)
        .ok_or_else(|| FeeError::InvalidInput("overflow".into()))
}

/// Percentile (linear-interpolation), `p` in 0..=100.
fn percentile(xs: &[u128], p: u32) -> Option<u128> {
    if xs.is_empty() {
        return None;
    }
    let mut sorted = xs.to_vec();
    sorted.sort_unstable();
    if sorted.len() == 1 {
        return Some(sorted[0]);
    }
    let idx_f = (p as f64 / 100.0) * (sorted.len() - 1) as f64;
    let lo = idx_f.floor() as usize;
    let hi = idx_f.ceil() as usize;
    if lo == hi {
        return Some(sorted[lo]);
    }
    let frac = idx_f - lo as f64;
    let lo_v = sorted[lo] as f64;
    let hi_v = sorted[hi] as f64;
    Some((lo_v + (hi_v - lo_v) * frac).round() as u128)
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: u128 = 1_000_000_000;

    #[test]
    fn empty_history_uses_one_gwei_floor() {
        let s = eip1559_suggest(20 * G, &[]).unwrap();
        assert_eq!(s.slow.max_priority_fee_per_gas, G);
        assert_eq!(s.standard.max_priority_fee_per_gas, G);
        assert_eq!(s.fast.max_priority_fee_per_gas, 2 * G);
    }

    #[test]
    fn max_fee_is_two_times_base_plus_tip() {
        let s = eip1559_suggest(10 * G, &[]).unwrap();
        assert_eq!(s.standard.max_fee_per_gas, 20 * G + G);
    }

    #[test]
    fn fast_priority_is_at_least_standard() {
        let history = vec![G, 2 * G, 3 * G, 4 * G, 5 * G, 6 * G, 7 * G, 8 * G];
        let s = eip1559_suggest(20 * G, &history).unwrap();
        assert!(s.fast.max_priority_fee_per_gas >= s.standard.max_priority_fee_per_gas);
        assert!(s.standard.max_priority_fee_per_gas >= s.slow.max_priority_fee_per_gas);
    }

    #[test]
    fn percentile_single_sample() {
        let s = eip1559_suggest(20 * G, &[5 * G]).unwrap();
        assert_eq!(s.slow.max_priority_fee_per_gas, 5 * G);
        assert_eq!(s.standard.max_priority_fee_per_gas, 5 * G);
        assert_eq!(s.fast.max_priority_fee_per_gas, 5 * G);
    }

    #[test]
    fn bump_at_least_ten_percent_priority() {
        let s = Eip1559Suggestion {
            tier: FeeTier::Standard,
            max_priority_fee_per_gas: 100,
            max_fee_per_gas: 1_000,
        };
        let b = bump_for_replacement(&s);
        assert!(b.max_priority_fee_per_gas >= 110);
        assert!(b.max_fee_per_gas >= 1_100);
    }

    #[test]
    fn bump_rounds_up_for_uneven_amounts() {
        let s = Eip1559Suggestion {
            tier: FeeTier::Standard,
            max_priority_fee_per_gas: 11,
            max_fee_per_gas: 11,
        };
        let b = bump_for_replacement(&s);
        // 11 * 1.1 = 12.1 → ceil → 13.
        assert_eq!(b.max_priority_fee_per_gas, 13);
    }

    #[test]
    fn utxo_fee_basic() {
        assert_eq!(utxo_fee_sats(10, 250).unwrap(), 2_500);
    }

    #[test]
    fn utxo_fee_rejects_zero_rate() {
        let err = utxo_fee_sats(0, 250).unwrap_err();
        assert!(matches!(err, FeeError::InvalidInput(_)));
    }

    #[test]
    fn utxo_fee_rejects_zero_vsize() {
        let err = utxo_fee_sats(10, 0).unwrap_err();
        assert!(matches!(err, FeeError::InvalidInput(_)));
    }

    #[test]
    fn utxo_fee_overflow_is_caught() {
        let err = utxo_fee_sats(u64::MAX, 2).unwrap_err();
        assert!(matches!(err, FeeError::InvalidInput(_)));
    }

    #[test]
    fn priority_history_with_outlier_doesnt_blow_up_fast_tier() {
        let mut history: Vec<u128> = (1..=10).map(|n| (n as u128) * G).collect();
        history.push(1_000 * G); // outlier
        let s = eip1559_suggest(20 * G, &history).unwrap();
        // 90th percentile of 11 sorted samples lands close to the
        // 10th sample (10 * G), not the outlier.
        assert!(s.fast.max_priority_fee_per_gas <= 11 * G);
    }
}
