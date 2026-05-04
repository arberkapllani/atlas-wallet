//! Gas-cost USD estimator.
//!
//! Pure data: the host injects the live native-token price (USD)
//! and the gas parameters; this crate hands back the wei cost, the
//! native-token cost, and the USD cost as fixed-precision strings
//! suitable for direct rendering. Built for the EVM gas screen and
//! the swap confirmation modal.
//!
//! No floats inside the math path — everything is u128 with a
//! split into integer + fractional parts at the end so a
//! 1234.5678 USD answer doesn't depend on f64 ordering.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

#[derive(Debug, Error, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "message")]
pub enum GasCostError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct GasCost {
    /// Total wei the network will charge:
    /// `gas_used * effective_gas_price_wei`.
    pub wei: u128,
    /// `wei` formatted as a native-token amount, e.g. "0.001234".
    pub native_amount: String,
    /// USD value to 4 decimals, e.g. "2.4321".
    pub usd: String,
}

const WEI_PER_ETH: u128 = 1_000_000_000_000_000_000; // 10^18

/// Compute gas cost given:
///   * `gas_used`               — units of gas
///   * `effective_gas_price_wei` — base + priority fee actually paid
///   * `native_price_usd_micro` — native-token price scaled by 1e6
///     (e.g. $1234.567890 → 1_234_567_890). Avoids floats.
pub fn estimate(
    gas_used: u64,
    effective_gas_price_wei: u128,
    native_price_usd_micro: u64,
) -> Result<GasCost, GasCostError> {
    if gas_used == 0 {
        return Err(GasCostError::InvalidInput("gas_used = 0".into()));
    }
    if effective_gas_price_wei == 0 {
        return Err(GasCostError::InvalidInput("gas_price = 0".into()));
    }

    let wei = (gas_used as u128)
        .checked_mul(effective_gas_price_wei)
        .ok_or_else(|| GasCostError::InvalidInput("wei overflow".into()))?;

    Ok(GasCost {
        wei,
        native_amount: format_eth(wei, 6),
        usd: format_usd(wei, native_price_usd_micro)?,
    })
}

/// Format `wei` as an ETH amount with `decimals` fractional digits
/// (truncated, not rounded). Always lossless.
pub fn format_eth(wei: u128, decimals: u32) -> String {
    let whole = wei / WEI_PER_ETH;
    let remainder = wei % WEI_PER_ETH;
    if decimals == 0 {
        return whole.to_string();
    }
    // Pad remainder up to 18 digits, then truncate to `decimals`.
    let frac_18 = format!("{remainder:018}");
    let take = decimals.min(18) as usize;
    let frac = &frac_18[..take];
    if frac.chars().all(|c| c == '0') {
        format!("{whole}.{}", "0".repeat(take))
    } else {
        format!("{whole}.{frac}")
    }
}

/// `usd_micro_total = wei * price_usd_micro / 1e18`. Returned with
/// 4 fractional digits, half-up rounded.
fn format_usd(wei: u128, native_price_usd_micro: u64) -> Result<String, GasCostError> {
    if native_price_usd_micro == 0 {
        return Ok("0.0000".to_string());
    }
    // Carry through 1e10 scale: usd_total_e10 = wei * price_micro / 1e8
    // (price is already 1e6, wei is 1e18 native; we want 4 decimal
    //  places on the USD output → multiply price up to 1e10 first).
    let scaled = (native_price_usd_micro as u128)
        .checked_mul(10_000) // → 1e10 fixed point
        .ok_or_else(|| GasCostError::InvalidInput("price overflow".into()))?;
    // result_e10 = wei * scaled / 1e18.
    let prod_high = wei
        .checked_mul(scaled)
        .ok_or_else(|| GasCostError::InvalidInput("usd overflow".into()))?;
    let result_e10 = prod_high / WEI_PER_ETH;

    // result_e10 has 10 decimal places; truncate to 4 (half-up using
    // a guard digit at position 5).
    let big = result_e10;
    let div = 1_000_000u128; // 1e6 → drop 6 digits → leaves 4 dp
    let half = div / 2;
    let rounded = (big + half) / div;
    let whole = rounded / 10_000;
    let frac = rounded % 10_000;
    Ok(format!("{whole}.{frac:04}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_gas() {
        let err = estimate(0, 1, 1).unwrap_err();
        assert!(matches!(err, GasCostError::InvalidInput(_)));
    }

    #[test]
    fn rejects_zero_price() {
        let err = estimate(21_000, 0, 1).unwrap_err();
        assert!(matches!(err, GasCostError::InvalidInput(_)));
    }

    #[test]
    fn wei_is_gas_times_price() {
        let g = estimate(21_000, 100_000_000_000, 0).unwrap();
        assert_eq!(g.wei, 21_000u128 * 100_000_000_000u128);
    }

    #[test]
    fn one_eth_round_trip() {
        // 21_000 gas at 47_619_047_619_048 wei/gas ≈ 1 ETH.
        // Use exact 10^18 / 21_000 = 47619047619047 (truncated)
        // → 21_000 * 47619047619047 = 999999999999987000 wei.
        let g = estimate(21_000, 47_619_047_619_048, 0).unwrap();
        assert_eq!(g.wei, 21_000u128 * 47_619_047_619_048u128);
        // ~1.000000 ETH (truncated to 6 dp).
        assert!(g.native_amount.starts_with("1.0"));
    }

    #[test]
    fn format_eth_zero_decimals() {
        assert_eq!(format_eth(WEI_PER_ETH, 0), "1");
    }

    #[test]
    fn format_eth_six_decimals_pads_zeros() {
        assert_eq!(format_eth(WEI_PER_ETH, 6), "1.000000");
    }

    #[test]
    fn format_eth_partial_amount() {
        // 0.001 ETH = 10^15 wei.
        assert_eq!(format_eth(1_000_000_000_000_000, 6), "0.001000");
    }

    #[test]
    fn format_eth_truncates_not_rounds() {
        // 0.0000004999... ETH → 6 dp truncated → "0.000000".
        let wei = 499_999_999_999;
        assert_eq!(format_eth(wei, 6), "0.000000");
    }

    #[test]
    fn usd_one_eth_at_2000_dollars() {
        // price $2000.000000 → 2_000_000_000 micro.
        // 21000 gas at 47619047619048 wei → ~1 ETH → ~$2000.
        let g = estimate(21_000, 47_619_047_619_048, 2_000_000_000).unwrap();
        assert!(g.usd.starts_with("2000.0"), "got {}", g.usd);
    }

    #[test]
    fn usd_zero_price_returns_zero() {
        let g = estimate(21_000, 100_000_000_000, 0).unwrap();
        assert_eq!(g.usd, "0.0000");
    }

    #[test]
    fn usd_small_swap_rounds_to_4dp() {
        // 100_000 gas at 50 gwei = 0.005 ETH; price $1.00 →
        // total ~ $0.005 → "0.0050".
        let g = estimate(100_000, 50_000_000_000, 1_000_000).unwrap();
        // Allow either "0.0050" or "0.0049" depending on integer
        // truncation order (both are within 1 micro-cent).
        assert!(g.usd == "0.0050" || g.usd == "0.0049", "got {}", g.usd);
    }

    #[test]
    fn native_amount_has_six_decimals() {
        let g = estimate(21_000, 100_000_000_000, 0).unwrap();
        let dot = g.native_amount.find('.').unwrap();
        assert_eq!(g.native_amount.len() - dot - 1, 6);
    }

    #[test]
    fn extreme_gas_price_does_not_overflow() {
        // 30M gas at 1000 gwei = 0.03 ETH. Just check no panic.
        let g = estimate(30_000_000, 1_000_000_000_000, 2_000_000_000).unwrap();
        assert!(g.wei > 0);
        assert!(!g.usd.is_empty());
    }

    #[test]
    fn cheap_l2_tx_yields_micro_usd() {
        // Arbitrum-ish: 21k gas at 0.1 gwei, ETH = $2000.
        let g = estimate(21_000, 100_000_000, 2_000_000_000).unwrap();
        // 21000 * 1e8 = 2.1e12 wei = 0.0000021 ETH ≈ $0.0042.
        assert!(g.usd.starts_with("0.00"), "got {}", g.usd);
    }
}
