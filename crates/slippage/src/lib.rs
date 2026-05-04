//! Swap slippage / deadline settings + min-out math.
//!
//! Slippage is stored in **basis points** (1 bp = 0.01 %). 50 bps =
//! 0.50 %. Deadlines are in seconds. Pure data; no I/O.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

#[derive(Debug, Error, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "message")]
pub enum SlippageError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

/// Hard cap, refusing anything above 50 % protects users from a
/// fat-fingered "5000".
pub const MAX_SLIPPAGE_BPS: u32 = 5_000;
pub const MIN_DEADLINE_SECS: u64 = 30;
pub const MAX_DEADLINE_SECS: u64 = 60 * 60; // 1 hour

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum SlippageBand {
    /// <= 50 bps (0.50 %).
    Low,
    /// 50 < x <= 200 bps (0.50 % – 2.00 %).
    Normal,
    /// 200 < x <= 1000 bps (2 % – 10 %).
    High,
    /// > 1000 bps (over 10 %): show a confirmation banner.
    Reckless,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SwapSettings {
    pub slippage_bps: u32,
    pub deadline_secs: u64,
}

impl Default for SwapSettings {
    fn default() -> Self {
        Self {
            slippage_bps: 50, // 0.50 %
            deadline_secs: 20 * 60,
        }
    }
}

impl SwapSettings {
    pub fn new(slippage_bps: u32, deadline_secs: u64) -> Result<Self, SlippageError> {
        if slippage_bps > MAX_SLIPPAGE_BPS {
            return Err(SlippageError::InvalidInput(format!(
                "slippage {slippage_bps} bps > max {MAX_SLIPPAGE_BPS}"
            )));
        }
        if !(MIN_DEADLINE_SECS..=MAX_DEADLINE_SECS).contains(&deadline_secs) {
            return Err(SlippageError::InvalidInput(format!(
                "deadline {deadline_secs}s out of [{MIN_DEADLINE_SECS}, {MAX_DEADLINE_SECS}]"
            )));
        }
        Ok(Self {
            slippage_bps,
            deadline_secs,
        })
    }

    pub fn band(&self) -> SlippageBand {
        match self.slippage_bps {
            0..=50 => SlippageBand::Low,
            51..=200 => SlippageBand::Normal,
            201..=1_000 => SlippageBand::High,
            _ => SlippageBand::Reckless,
        }
    }

    pub fn from_json(json: &str) -> Result<Self, SlippageError> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(json).map_err(|e| SlippageError::InvalidInput(e.to_string()))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Compute the `amountOutMin` to pass to a router given a quoted
/// amount and the slippage tolerance in bps. Floor division so we
/// never accept worse than the user permitted.
pub fn min_out(amount_out_quote: u128, slippage_bps: u32) -> Result<u128, SlippageError> {
    if slippage_bps > MAX_SLIPPAGE_BPS {
        return Err(SlippageError::InvalidInput(format!(
            "slippage {slippage_bps} bps > max {MAX_SLIPPAGE_BPS}"
        )));
    }
    // amount * (10000 - bps) / 10000.
    let factor = 10_000u128
        .checked_sub(slippage_bps as u128)
        .ok_or_else(|| SlippageError::InvalidInput("bps > 10000".into()))?;
    let prod = amount_out_quote
        .checked_mul(factor)
        .ok_or_else(|| SlippageError::InvalidInput("overflow".into()))?;
    Ok(prod / 10_000)
}

/// Compute the `amountInMax` to pass to an exact-out swap given a
/// quoted input and the slippage tolerance in bps. Ceil division so
/// we always allow at least the quote + slippage.
pub fn max_in(amount_in_quote: u128, slippage_bps: u32) -> Result<u128, SlippageError> {
    if slippage_bps > MAX_SLIPPAGE_BPS {
        return Err(SlippageError::InvalidInput(format!(
            "slippage {slippage_bps} bps > max {MAX_SLIPPAGE_BPS}"
        )));
    }
    let factor = 10_000u128
        .checked_add(slippage_bps as u128)
        .ok_or_else(|| SlippageError::InvalidInput("overflow".into()))?;
    let prod = amount_in_quote
        .checked_mul(factor)
        .ok_or_else(|| SlippageError::InvalidInput("overflow".into()))?;
    Ok(prod.div_ceil(10_000))
}

/// Convert a deadline-in-seconds-from-now into an absolute UNIX
/// timestamp suitable for a router's `deadline` arg.
pub fn deadline_unix(now_unix: u64, deadline_secs: u64) -> Result<u64, SlippageError> {
    if !(MIN_DEADLINE_SECS..=MAX_DEADLINE_SECS).contains(&deadline_secs) {
        return Err(SlippageError::InvalidInput("deadline out of range".into()));
    }
    now_unix
        .checked_add(deadline_secs)
        .ok_or_else(|| SlippageError::InvalidInput("overflow".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_50bps_20m() {
        let s = SwapSettings::default();
        assert_eq!(s.slippage_bps, 50);
        assert_eq!(s.deadline_secs, 20 * 60);
        assert_eq!(s.band(), SlippageBand::Low);
    }

    #[test]
    fn band_thresholds() {
        let cases = [
            (0u32, SlippageBand::Low),
            (50, SlippageBand::Low),
            (51, SlippageBand::Normal),
            (200, SlippageBand::Normal),
            (201, SlippageBand::High),
            (1_000, SlippageBand::High),
            (1_001, SlippageBand::Reckless),
            (5_000, SlippageBand::Reckless),
        ];
        for (bps, want) in cases {
            let s = SwapSettings::new(bps, 60).unwrap();
            assert_eq!(s.band(), want, "bps={bps}");
        }
    }

    #[test]
    fn rejects_slippage_above_cap() {
        let err = SwapSettings::new(MAX_SLIPPAGE_BPS + 1, 60).unwrap_err();
        assert!(matches!(err, SlippageError::InvalidInput(_)));
    }

    #[test]
    fn rejects_deadline_too_short() {
        let err = SwapSettings::new(50, MIN_DEADLINE_SECS - 1).unwrap_err();
        assert!(matches!(err, SlippageError::InvalidInput(_)));
    }

    #[test]
    fn rejects_deadline_too_long() {
        let err = SwapSettings::new(50, MAX_DEADLINE_SECS + 1).unwrap_err();
        assert!(matches!(err, SlippageError::InvalidInput(_)));
    }

    #[test]
    fn min_out_50bps() {
        // 1000 - 0.5 % = 995.
        assert_eq!(min_out(1_000, 50).unwrap(), 995);
    }

    #[test]
    fn min_out_100bps_floors() {
        assert_eq!(min_out(1_001, 100).unwrap(), 990);
    }

    #[test]
    fn min_out_zero_slippage_is_identity() {
        assert_eq!(min_out(1_000_000, 0).unwrap(), 1_000_000);
    }

    #[test]
    fn min_out_rejects_too_much_slippage() {
        let err = min_out(1_000, MAX_SLIPPAGE_BPS + 1).unwrap_err();
        assert!(matches!(err, SlippageError::InvalidInput(_)));
    }

    #[test]
    fn max_in_50bps_rounds_up() {
        // 1000 + 0.5 % = 1005.
        assert_eq!(max_in(1_000, 50).unwrap(), 1_005);
    }

    #[test]
    fn max_in_ceils_remainder() {
        // 1001 * 1.005 = 1006.005 → 1007.
        assert_eq!(max_in(1_001, 50).unwrap(), 1_007);
    }

    #[test]
    fn deadline_adds_seconds() {
        assert_eq!(deadline_unix(1_700_000_000, 60).unwrap(), 1_700_000_060);
    }

    #[test]
    fn deadline_rejects_out_of_range() {
        let err = deadline_unix(1_700_000_000, 1).unwrap_err();
        assert!(matches!(err, SlippageError::InvalidInput(_)));
    }

    #[test]
    fn json_round_trip() {
        let s = SwapSettings::new(75, 5 * 60).unwrap();
        let restored = SwapSettings::from_json(&s.to_json()).unwrap();
        assert_eq!(restored.slippage_bps, 75);
        assert_eq!(restored.deadline_secs, 300);
    }

    #[test]
    fn from_json_empty_is_default() {
        let s = SwapSettings::from_json("").unwrap();
        assert_eq!(s.slippage_bps, 50);
    }
}
