//! Chain-agnostic staking primitives.
//!
//! Atlas supports user-delegated staking across six networks with
//! very different mechanics (PoS validators, pool-of-pools, ETH
//! liquid staking via Lido, Polygon's checkpoint model, etc.). Rather
//! than try to unify the *signing* side here, this crate unifies the
//! *display* side: validator metadata, position summaries, APR/APY
//! conversion, and scoring.
//!
//! Signing flows still live in their respective chain crates; this
//! one is pure data + math so the gallery / validator picker UI can
//! reuse it.

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StakingError {
    #[error("unsupported chain: {0}")]
    UnsupportedChain(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

/// Networks where Atlas surfaces native staking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum StakingChain {
    Solana,
    Cosmos,
    Cardano,
    Polkadot,
    Ethereum,
    Polygon,
}

impl StakingChain {
    pub fn id(&self) -> &'static str {
        match self {
            StakingChain::Solana => "sol",
            StakingChain::Cosmos => "atom",
            StakingChain::Cardano => "ada",
            StakingChain::Polkadot => "dot",
            StakingChain::Ethereum => "eth",
            StakingChain::Polygon => "matic",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "sol" => Some(Self::Solana),
            "atom" => Some(Self::Cosmos),
            "ada" => Some(Self::Cardano),
            "dot" => Some(Self::Polkadot),
            "eth" => Some(Self::Ethereum),
            "matic" => Some(Self::Polygon),
            _ => None,
        }
    }

    /// How many compounding periods per year are typical for this
    /// network. Used by [`apr_to_apy`].
    pub fn default_compounds_per_year(&self) -> u32 {
        match self {
            // Solana epochs: ~2 days → ~182.
            StakingChain::Solana => 182,
            // Cosmos: rewards are typically claimed manually; treat
            // as monthly compounding for display.
            StakingChain::Cosmos => 12,
            // Cardano: epoch every 5 days → 73.
            StakingChain::Cardano => 73,
            // Polkadot: era is ~24h → 365.
            StakingChain::Polkadot => 365,
            // Ethereum (via Lido) is continuously compounding; 365
            // is a good display approximation.
            StakingChain::Ethereum => 365,
            // Polygon checkpoints ~30min → cap at 365 for display
            // sanity (overstating compounding frequency hides little
            // information at low APRs).
            StakingChain::Polygon => 365,
        }
    }

    /// Typical unbonding period (days). Surface in the consent UI so
    /// the user knows their funds will be locked.
    pub fn unbonding_days(&self) -> u32 {
        match self {
            StakingChain::Solana => 3,
            StakingChain::Cosmos => 21,
            StakingChain::Cardano => 0, // liquid; no unbonding period
            StakingChain::Polkadot => 28,
            StakingChain::Ethereum => 0, // Lido stETH is liquid
            StakingChain::Polygon => 9,
        }
    }
}

/// One validator the user can delegate to.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Validator {
    pub chain: StakingChain,
    /// Chain-native identifier (vote pubkey, valoper address, pool id, etc).
    pub id: String,
    pub name: String,
    /// Commission in basis points (10000 = 100%).
    pub commission_bps: u32,
    /// Self-reported total stake in the network's base units
    /// (lamports, uatom, lovelace, plancks, wei). Stored as f64 so
    /// extreme totals (Polkadot validators) round-trip cleanly through
    /// JSON without needing u128.
    pub total_stake: f64,
    /// Net APR (after the validator's commission) as a fraction
    /// (0.045 = 4.5%).
    pub net_apr: f64,
    /// Whether this validator is currently active (in the active set
    /// / not jailed / not slashed-out).
    pub active: bool,
    /// True if Atlas considers this validator decentralisation-friendly
    /// (small share of total stake, no jail history). Used to bias the
    /// recommendation list without hiding alternatives.
    pub recommended: bool,
}

/// User's delegation to one validator.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Delegation {
    pub chain: StakingChain,
    pub validator_id: String,
    /// Delegated amount in base units.
    pub amount: f64,
    /// Pending rewards in base units. May be 0 for chains that
    /// auto-restake (Lido stETH).
    pub pending_rewards: f64,
    /// Pending unbonding amount in base units (still locked).
    pub unbonding: f64,
}

/// Aggregated staking position for one chain.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ChainPosition {
    pub chain: StakingChain,
    pub delegations: Vec<Delegation>,
    pub total_staked: f64,
    pub total_pending_rewards: f64,
    pub total_unbonding: f64,
    /// Stake-weighted average net APR across the user's delegations.
    /// 0.0 if `total_staked == 0`.
    pub weighted_net_apr: f64,
}

/// Aggregate of all chains.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StakingSummary {
    pub positions: Vec<ChainPosition>,
}

// --- Math primitives -------------------------------------------------

/// Convert a nominal APR into APY given `n` compounding periods per
/// year. Returns `apr` unchanged if `n == 0` or `n == 1`.
pub fn apr_to_apy(apr: f64, n: u32) -> f64 {
    if n <= 1 {
        return apr;
    }
    let r = apr / n as f64;
    (1.0 + r).powi(n as i32) - 1.0
}

/// Apply a basis-points commission to a gross APR.
/// `gross=0.06`, `commission_bps=500` → 5.7%.
pub fn net_apr_after_commission(gross_apr: f64, commission_bps: u32) -> f64 {
    let cap_bps = commission_bps.min(10_000);
    let cut = cap_bps as f64 / 10_000.0;
    gross_apr * (1.0 - cut)
}

/// Aggregate per-chain delegations together with the validator list
/// (used to derive the per-delegation APR for the weighted average).
pub fn aggregate_chain(
    chain: StakingChain,
    delegations: &[Delegation],
    validators: &[Validator],
) -> Result<ChainPosition, StakingError> {
    let mut total_staked = 0.0;
    let mut total_rewards = 0.0;
    let mut total_unbonding = 0.0;
    let mut weighted_apr_num = 0.0;
    let mut owned: Vec<Delegation> = Vec::with_capacity(delegations.len());

    for d in delegations {
        if d.chain != chain {
            return Err(StakingError::InvalidInput(format!(
                "delegation for {:?} in {:?} aggregate",
                d.chain, chain
            )));
        }
        if d.amount < 0.0 || d.pending_rewards < 0.0 || d.unbonding < 0.0 {
            return Err(StakingError::InvalidInput(
                "negative delegation amount".into(),
            ));
        }
        total_staked += d.amount;
        total_rewards += d.pending_rewards;
        total_unbonding += d.unbonding;

        if let Some(v) = validators
            .iter()
            .find(|v| v.id == d.validator_id && v.chain == chain)
        {
            weighted_apr_num += d.amount * v.net_apr;
        }
        owned.push(d.clone());
    }

    let weighted_net_apr = if total_staked > 0.0 {
        weighted_apr_num / total_staked
    } else {
        0.0
    };

    Ok(ChainPosition {
        chain,
        delegations: owned,
        total_staked,
        total_pending_rewards: total_rewards,
        total_unbonding,
        weighted_net_apr,
    })
}

/// Recommend a validator from the candidate set. Heuristic:
///
/// 1. Filter out inactive validators.
/// 2. Prefer `recommended=true` if any qualify.
/// 3. Among the remaining set, pick the one with the highest
///    [`net_apr`] **subject to** a soft cap on commission (≤ 10%) so
///    we don't push users to fee-gouging validators.
/// 4. As a tiebreaker, prefer the validator with *less* total stake
///    so we nudge users away from the largest pools.
pub fn recommend_validator(validators: &[Validator]) -> Option<&Validator> {
    let active: Vec<&Validator> = validators.iter().filter(|v| v.active).collect();
    if active.is_empty() {
        return None;
    }
    let pool: Vec<&Validator> = if active.iter().any(|v| v.recommended) {
        active.iter().copied().filter(|v| v.recommended).collect()
    } else {
        active
    };

    let acceptable: Vec<&Validator> = pool
        .iter()
        .copied()
        .filter(|v| v.commission_bps <= 1000)
        .collect();
    let candidates = if acceptable.is_empty() {
        pool
    } else {
        acceptable
    };

    candidates.into_iter().max_by(|a, b| {
        a.net_apr
            .partial_cmp(&b.net_apr)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.total_stake
                    .partial_cmp(&a.total_stake)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn v(
        chain: StakingChain,
        id: &str,
        name: &str,
        commission_bps: u32,
        total_stake: f64,
        net_apr: f64,
        active: bool,
        recommended: bool,
    ) -> Validator {
        Validator {
            chain,
            id: id.into(),
            name: name.into(),
            commission_bps,
            total_stake,
            net_apr,
            active,
            recommended,
        }
    }

    fn d(
        chain: StakingChain,
        validator_id: &str,
        amount: f64,
        rewards: f64,
        unbonding: f64,
    ) -> Delegation {
        Delegation {
            chain,
            validator_id: validator_id.into(),
            amount,
            pending_rewards: rewards,
            unbonding,
        }
    }

    #[test]
    fn chain_id_round_trips() {
        for c in [
            StakingChain::Solana,
            StakingChain::Cosmos,
            StakingChain::Cardano,
            StakingChain::Polkadot,
            StakingChain::Ethereum,
            StakingChain::Polygon,
        ] {
            assert_eq!(StakingChain::from_id(c.id()), Some(c));
        }
        assert_eq!(StakingChain::from_id("xrp"), None);
    }

    #[test]
    fn unbonding_days_documented_for_all_chains() {
        // Sanity: Cardano and Lido stETH should be 0 (liquid).
        assert_eq!(StakingChain::Cardano.unbonding_days(), 0);
        assert_eq!(StakingChain::Ethereum.unbonding_days(), 0);
        // Cosmos famously 21 days.
        assert_eq!(StakingChain::Cosmos.unbonding_days(), 21);
    }

    #[test]
    fn apr_to_apy_matches_continuous_for_high_n() {
        // 5% APR compounded daily ≈ 5.127%
        let apy = apr_to_apy(0.05, 365);
        assert!((apy - 0.05126).abs() < 1e-4);
    }

    #[test]
    fn apr_to_apy_identity_for_low_n() {
        assert_eq!(apr_to_apy(0.04, 0), 0.04);
        assert_eq!(apr_to_apy(0.04, 1), 0.04);
    }

    #[test]
    fn net_apr_after_commission_basic() {
        // 6% gross, 5% commission → 5.7%
        assert!((net_apr_after_commission(0.06, 500) - 0.057).abs() < 1e-9);
    }

    #[test]
    fn net_apr_clamps_commission_above_100pct() {
        assert_eq!(net_apr_after_commission(0.06, 50_000), 0.0);
    }

    #[test]
    fn aggregate_chain_computes_weighted_apr() {
        let vals = vec![
            v(
                StakingChain::Cosmos,
                "valA",
                "A",
                500,
                1.0,
                0.10,
                true,
                false,
            ),
            v(
                StakingChain::Cosmos,
                "valB",
                "B",
                500,
                1.0,
                0.20,
                true,
                false,
            ),
        ];
        let dels = vec![
            d(StakingChain::Cosmos, "valA", 100.0, 0.0, 0.0),
            d(StakingChain::Cosmos, "valB", 300.0, 0.0, 0.0),
        ];
        let pos = aggregate_chain(StakingChain::Cosmos, &dels, &vals).unwrap();
        assert_eq!(pos.total_staked, 400.0);
        // (100*0.10 + 300*0.20) / 400 = 0.175
        assert!((pos.weighted_net_apr - 0.175).abs() < 1e-9);
    }

    #[test]
    fn aggregate_chain_handles_empty_delegations() {
        let pos = aggregate_chain(StakingChain::Solana, &[], &[]).unwrap();
        assert_eq!(pos.total_staked, 0.0);
        assert_eq!(pos.weighted_net_apr, 0.0);
    }

    #[test]
    fn aggregate_chain_rejects_wrong_chain() {
        let bad = vec![d(StakingChain::Solana, "x", 1.0, 0.0, 0.0)];
        assert!(aggregate_chain(StakingChain::Cosmos, &bad, &[]).is_err());
    }

    #[test]
    fn aggregate_chain_rejects_negative_amounts() {
        let bad = vec![d(StakingChain::Cosmos, "v", -1.0, 0.0, 0.0)];
        assert!(aggregate_chain(StakingChain::Cosmos, &bad, &[]).is_err());
    }

    #[test]
    fn recommend_prefers_recommended_active_low_commission() {
        let vals = vec![
            v(
                StakingChain::Cosmos,
                "rec",
                "Rec",
                500,
                100.0,
                0.08,
                true,
                true,
            ),
            v(
                StakingChain::Cosmos,
                "fee",
                "FeeGouger",
                5000,
                100.0,
                0.20,
                true,
                false,
            ),
            v(
                StakingChain::Cosmos,
                "off",
                "Offline",
                300,
                100.0,
                0.30,
                false,
                true,
            ),
        ];
        let pick = recommend_validator(&vals).unwrap();
        assert_eq!(pick.id, "rec");
    }

    #[test]
    fn recommend_falls_back_when_no_recommended() {
        let vals = vec![
            v(
                StakingChain::Solana,
                "a",
                "A",
                500,
                1000.0,
                0.06,
                true,
                false,
            ),
            v(
                StakingChain::Solana,
                "b",
                "B",
                500,
                100.0,
                0.06,
                true,
                false,
            ),
        ];
        // Same APR, same commission → pick the smaller pool.
        let pick = recommend_validator(&vals).unwrap();
        assert_eq!(pick.id, "b");
    }

    #[test]
    fn recommend_returns_none_when_all_inactive() {
        let vals = vec![v(
            StakingChain::Solana,
            "a",
            "A",
            500,
            100.0,
            0.06,
            false,
            true,
        )];
        assert!(recommend_validator(&vals).is_none());
    }

    #[test]
    fn recommend_falls_back_to_high_commission_if_only_option() {
        let vals = vec![v(
            StakingChain::Solana,
            "a",
            "A",
            5000,
            100.0,
            0.06,
            true,
            false,
        )];
        let pick = recommend_validator(&vals).unwrap();
        assert_eq!(pick.id, "a");
    }
}
