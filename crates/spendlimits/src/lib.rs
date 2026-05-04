//! Per-day / per-tx spending-limit policy.
//!
//! The user opts into a daily USD cap and an optional single-tx
//! USD cap. Before broadcasting, the wallet calls
//! `evaluate(&policy, &state, now_unix, attempt_usd)` which returns
//! a `LimitDecision` (Allowed / RequiresConfirmation / Blocked) plus
//! the up-to-date state to persist if the tx then goes through.
//!
//! State rolls over once 24h have passed since `window_started_at`.
//! "Day" here is a sliding 24-hour window — calendar days are too
//! easy to game across timezones.
//!
//! Pure data; the host owns the JSON file and the clock.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

#[derive(Debug, Error, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "message")]
pub enum SpendLimitError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

const DAY_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Type)]
pub struct SpendPolicy {
    /// 0 = disabled.
    pub daily_usd_limit: u64,
    /// 0 = disabled.
    pub per_tx_usd_limit: u64,
    /// If true, exceeding the daily cap blocks the tx; otherwise
    /// it just requires an extra confirmation.
    pub hard_block_on_daily: bool,
}

impl SpendPolicy {
    pub fn validate(&self) -> Result<(), SpendLimitError> {
        if self.daily_usd_limit > 100_000_000_000 {
            return Err(SpendLimitError::InvalidInput(
                "daily_usd_limit absurdly large".into(),
            ));
        }
        if self.per_tx_usd_limit > 100_000_000_000 {
            return Err(SpendLimitError::InvalidInput(
                "per_tx_usd_limit absurdly large".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Type)]
pub struct SpendState {
    pub window_started_at: u64,
    pub spent_in_window_usd: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum LimitDecision {
    Allowed,
    RequiresConfirmation,
    Blocked,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
pub struct LimitEvaluation {
    pub decision: LimitDecision,
    /// Reason text the UI can show: "Daily cap exceeded", etc.
    /// Empty when `Allowed`.
    pub reason: ReasonCode,
    /// State to persist *if* the tx goes through. Already includes
    /// the rolled-over window and the new spend.
    pub next_state: SpendState,
    /// USD remaining in the day after this tx. Saturating at 0.
    pub remaining_after_usd: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum ReasonCode {
    None,
    PerTxCapExceeded,
    DailyCapExceeded,
}

/// Returns the policy decision plus the *prospective* next state.
/// Caller persists `next_state` only on a successful broadcast.
pub fn evaluate(
    policy: &SpendPolicy,
    state: &SpendState,
    now_unix: u64,
    attempt_usd: u64,
) -> Result<LimitEvaluation, SpendLimitError> {
    policy.validate()?;

    // Roll the window forward if we've crossed 24h since it started
    // (or if the persisted window is in the future, which would
    // happen after a clock change — clamp to "now" to be safe).
    let mut rolled = *state;
    if now_unix < rolled.window_started_at
        || now_unix.saturating_sub(rolled.window_started_at) >= DAY_SECS
    {
        rolled = SpendState {
            window_started_at: now_unix,
            spent_in_window_usd: 0,
        };
    }

    // Per-tx cap is the hardest stop.
    if policy.per_tx_usd_limit > 0 && attempt_usd > policy.per_tx_usd_limit {
        return Ok(LimitEvaluation {
            decision: LimitDecision::Blocked,
            reason: ReasonCode::PerTxCapExceeded,
            next_state: rolled,
            remaining_after_usd: policy
                .daily_usd_limit
                .saturating_sub(rolled.spent_in_window_usd),
        });
    }

    let prospective_total = rolled.spent_in_window_usd.saturating_add(attempt_usd);
    let exceeds_daily = policy.daily_usd_limit > 0 && prospective_total > policy.daily_usd_limit;

    let next_state = SpendState {
        window_started_at: rolled.window_started_at,
        spent_in_window_usd: prospective_total,
    };

    let decision = if exceeds_daily {
        if policy.hard_block_on_daily {
            LimitDecision::Blocked
        } else {
            LimitDecision::RequiresConfirmation
        }
    } else {
        LimitDecision::Allowed
    };

    let reason = match decision {
        LimitDecision::Allowed => ReasonCode::None,
        _ => ReasonCode::DailyCapExceeded,
    };

    let remaining_after_usd = policy.daily_usd_limit.saturating_sub(prospective_total);

    Ok(LimitEvaluation {
        decision,
        reason,
        next_state,
        remaining_after_usd,
    })
}

pub fn from_json(json: &str) -> Result<SpendPolicy, SpendLimitError> {
    if json.trim().is_empty() {
        return Ok(SpendPolicy::default());
    }
    serde_json::from_str(json).map_err(|e| SpendLimitError::InvalidInput(e.to_string()))
}

pub fn to_json(p: &SpendPolicy) -> String {
    serde_json::to_string(p).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pol(daily: u64, per_tx: u64, hard: bool) -> SpendPolicy {
        SpendPolicy {
            daily_usd_limit: daily,
            per_tx_usd_limit: per_tx,
            hard_block_on_daily: hard,
        }
    }

    fn st(started: u64, spent: u64) -> SpendState {
        SpendState {
            window_started_at: started,
            spent_in_window_usd: spent,
        }
    }

    #[test]
    fn no_limits_means_always_allowed() {
        let r = evaluate(&pol(0, 0, false), &st(0, 0), 100, 1_000_000).unwrap();
        assert_eq!(r.decision, LimitDecision::Allowed);
        assert_eq!(r.reason, ReasonCode::None);
    }

    #[test]
    fn within_daily_is_allowed() {
        let r = evaluate(&pol(1_000, 0, true), &st(100, 200), 200, 300).unwrap();
        assert_eq!(r.decision, LimitDecision::Allowed);
        assert_eq!(r.next_state.spent_in_window_usd, 500);
        assert_eq!(r.remaining_after_usd, 500);
    }

    #[test]
    fn exact_daily_limit_is_allowed() {
        let r = evaluate(&pol(1_000, 0, true), &st(100, 700), 200, 300).unwrap();
        assert_eq!(r.decision, LimitDecision::Allowed);
        assert_eq!(r.remaining_after_usd, 0);
    }

    #[test]
    fn over_daily_with_hard_block_is_blocked() {
        let r = evaluate(&pol(1_000, 0, true), &st(100, 800), 200, 300).unwrap();
        assert_eq!(r.decision, LimitDecision::Blocked);
        assert_eq!(r.reason, ReasonCode::DailyCapExceeded);
    }

    #[test]
    fn over_daily_without_hard_block_requires_confirmation() {
        let r = evaluate(&pol(1_000, 0, false), &st(100, 800), 200, 300).unwrap();
        assert_eq!(r.decision, LimitDecision::RequiresConfirmation);
        assert_eq!(r.reason, ReasonCode::DailyCapExceeded);
    }

    #[test]
    fn per_tx_cap_blocks_regardless_of_hard_flag() {
        let r = evaluate(&pol(10_000, 500, false), &st(100, 0), 200, 600).unwrap();
        assert_eq!(r.decision, LimitDecision::Blocked);
        assert_eq!(r.reason, ReasonCode::PerTxCapExceeded);
    }

    #[test]
    fn per_tx_cap_exactly_is_allowed() {
        let r = evaluate(&pol(10_000, 500, false), &st(100, 0), 200, 500).unwrap();
        assert_eq!(r.decision, LimitDecision::Allowed);
    }

    #[test]
    fn window_rolls_over_after_24h() {
        let day = DAY_SECS;
        let r = evaluate(&pol(1_000, 0, true), &st(0, 800), day, 300).unwrap();
        // The old 800 should be discarded → 300 spent in fresh window.
        assert_eq!(r.next_state.spent_in_window_usd, 300);
        assert_eq!(r.next_state.window_started_at, day);
        assert_eq!(r.decision, LimitDecision::Allowed);
    }

    #[test]
    fn window_does_not_roll_just_before_24h() {
        let almost = DAY_SECS - 1;
        let r = evaluate(&pol(1_000, 0, true), &st(0, 800), almost, 300).unwrap();
        // Old 800 + new 300 = 1100 > 1000.
        assert_eq!(r.decision, LimitDecision::Blocked);
        assert_eq!(r.next_state.window_started_at, 0);
    }

    #[test]
    fn clock_skew_into_past_resets_window() {
        let r = evaluate(&pol(1_000, 0, true), &st(1_000, 800), 100, 300).unwrap();
        assert_eq!(r.next_state.window_started_at, 100);
        assert_eq!(r.next_state.spent_in_window_usd, 300);
    }

    #[test]
    fn remaining_saturates_at_zero() {
        let r = evaluate(&pol(1_000, 0, false), &st(100, 800), 200, 999_999).unwrap();
        assert_eq!(r.remaining_after_usd, 0);
    }

    #[test]
    fn validate_rejects_absurd_daily() {
        let p = pol(1_000_000_000_001, 0, false);
        assert!(matches!(
            p.validate(),
            Err(SpendLimitError::InvalidInput(_))
        ));
    }

    #[test]
    fn validate_rejects_absurd_per_tx() {
        let p = pol(0, 1_000_000_000_001, false);
        assert!(matches!(
            p.validate(),
            Err(SpendLimitError::InvalidInput(_))
        ));
    }

    #[test]
    fn json_round_trip() {
        let p = pol(1_500, 500, true);
        let restored = from_json(&to_json(&p)).unwrap();
        assert_eq!(restored.daily_usd_limit, 1_500);
        assert_eq!(restored.per_tx_usd_limit, 500);
        assert!(restored.hard_block_on_daily);
    }

    #[test]
    fn from_json_empty_is_default() {
        let p = from_json("").unwrap();
        assert_eq!(p.daily_usd_limit, 0);
        assert_eq!(p.per_tx_usd_limit, 0);
        assert!(!p.hard_block_on_daily);
    }

    #[test]
    fn per_tx_zero_means_disabled() {
        let r = evaluate(&pol(10_000, 0, true), &st(100, 0), 200, 9_999).unwrap();
        assert_eq!(r.decision, LimitDecision::Allowed);
    }
}
