//! Address-poisoning attack detector.
//!
//! In an address-poisoning attack, the attacker sends 0-value (or
//! dust) transactions from a wallet whose first/last hex characters
//! match an address the victim recently sent funds to. The
//! attacker's address then appears in the victim's tx history, and
//! when the user copy-pastes from history they accidentally send to
//! the look-alike instead of the real destination.
//!
//! This crate scans a list of incoming transfers against a list of
//! "trusted" counterparties (addresses the user has previously sent
//! funds to) and flags entries whose source matches the look-alike
//! pattern.
//!
//! Pure data — no I/O, no chain. Address-format agnostic: just
//! compares prefix/suffix of the lower-cased string after stripping
//! `0x`.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

/// Failure modes.
#[derive(Debug, Error, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "message")]
pub enum PoisonError {
    /// Threshold parameters out of range.
    #[error("invalid config: {0}")]
    InvalidConfig(String),
}

/// One incoming transfer the user received.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct IncomingTransfer {
    pub txid: String,
    pub from: String,
    /// Decimal-string amount in base units. Empty / "0" treated as zero.
    pub amount: String,
    /// Unix seconds.
    pub timestamp: i64,
    pub asset: String,
}

/// One row in the user's outbound history.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TrustedCounterparty {
    /// Address the user previously sent funds to.
    pub address: String,
    /// First time the user sent here (Unix seconds). Used to ensure
    /// the look-alike arrived *after* the legitimate transfer.
    pub first_seen: i64,
}

/// Knobs for the detector.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PoisonConfig {
    /// Minimum number of leading hex characters that must match for
    /// a transfer to be flagged. Default: 6.
    pub min_prefix: u32,
    /// Minimum number of trailing hex characters that must match.
    /// Default: 6.
    pub min_suffix: u32,
    /// Decimal-string upper bound on "dust" amount (inclusive).
    /// Transfers <= this in base units are extra-suspect. Default
    /// `"1000000"` — 1e6 base units (≈ 1 USDC, ≈ 1e-12 ETH).
    pub dust_threshold: String,
}

impl Default for PoisonConfig {
    fn default() -> Self {
        Self {
            min_prefix: 6,
            min_suffix: 6,
            dust_threshold: "1000000".into(),
        }
    }
}

/// One flagged transfer.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PoisonAlert {
    pub transfer: IncomingTransfer,
    /// The trusted counterparty whose address is being mimicked.
    pub mimicked: TrustedCounterparty,
    /// Number of leading hex characters that match.
    pub matching_prefix: u32,
    /// Number of trailing hex characters that match.
    pub matching_suffix: u32,
    /// True if amount ≤ `dust_threshold`.
    pub is_dust: bool,
}

/// Scan `transfers` against `trusted` and return a sorted list of
/// alerts (most-suspicious first: dust before non-dust, then by
/// matching length descending).
pub fn detect(
    transfers: &[IncomingTransfer],
    trusted: &[TrustedCounterparty],
    config: &PoisonConfig,
) -> Result<Vec<PoisonAlert>, PoisonError> {
    if config.min_prefix == 0 && config.min_suffix == 0 {
        return Err(PoisonError::InvalidConfig(
            "min_prefix and min_suffix cannot both be zero".into(),
        ));
    }
    if config.min_prefix > 40 || config.min_suffix > 40 {
        return Err(PoisonError::InvalidConfig(
            "prefix/suffix lengths exceed 40".into(),
        ));
    }

    let mut alerts: Vec<PoisonAlert> = Vec::new();
    for t in transfers {
        let from_norm = normalise(&t.from);
        // Skip self-poisoning of an exact-match (legit re-send).
        let exact_match = trusted.iter().any(|c| normalise(&c.address) == from_norm);
        if exact_match {
            continue;
        }
        let mut best: Option<(TrustedCounterparty, u32, u32)> = None;
        for c in trusted {
            // Only consider counterparties we trusted *before* this
            // incoming tx — otherwise we'd false-positive on the very
            // first send to a new address that happens to share a
            // prefix.
            if c.first_seen >= t.timestamp {
                continue;
            }
            let target = normalise(&c.address);
            let prefix = common_prefix(&from_norm, &target);
            let suffix = common_suffix(&from_norm, &target);
            if prefix < config.min_prefix && suffix < config.min_suffix {
                continue;
            }
            // Better = longer combined match.
            let total = prefix + suffix;
            match &best {
                Some((_, p, s)) if (*p + *s) >= total => {}
                _ => best = Some((c.clone(), prefix, suffix)),
            }
        }
        if let Some((mimicked, matching_prefix, matching_suffix)) = best {
            let is_dust = is_dust_amount(&t.amount, &config.dust_threshold);
            alerts.push(PoisonAlert {
                transfer: t.clone(),
                mimicked,
                matching_prefix,
                matching_suffix,
                is_dust,
            });
        }
    }

    // Most suspicious first: dust > non-dust, then total match length desc.
    alerts.sort_by(|a, b| {
        b.is_dust
            .cmp(&a.is_dust)
            .then_with(|| {
                (b.matching_prefix + b.matching_suffix)
                    .cmp(&(a.matching_prefix + a.matching_suffix))
            })
            .then_with(|| a.transfer.timestamp.cmp(&b.transfer.timestamp))
    });
    Ok(alerts)
}

// --- helpers ---------------------------------------------------------

fn normalise(addr: &str) -> String {
    addr.trim()
        .to_ascii_lowercase()
        .trim_start_matches("0x")
        .to_string()
}

fn common_prefix(a: &str, b: &str) -> u32 {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count() as u32
}

fn common_suffix(a: &str, b: &str) -> u32 {
    a.chars()
        .rev()
        .zip(b.chars().rev())
        .take_while(|(x, y)| x == y)
        .count() as u32
}

fn is_dust_amount(amount: &str, threshold: &str) -> bool {
    let amount = amount.trim();
    if amount.is_empty() || amount == "0" {
        return true;
    }
    let amount = amount.trim_start_matches('0');
    let threshold = threshold.trim_start_matches('0');
    let amount = if amount.is_empty() { "0" } else { amount };
    let threshold = if threshold.is_empty() { "0" } else { threshold };
    if amount.len() < threshold.len() {
        return true;
    }
    if amount.len() > threshold.len() {
        return false;
    }
    amount <= threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xfer(txid: &str, from: &str, amount: &str, ts: i64) -> IncomingTransfer {
        IncomingTransfer {
            txid: txid.into(),
            from: from.into(),
            amount: amount.into(),
            timestamp: ts,
            asset: "USDC".into(),
        }
    }

    fn trust(addr: &str, ts: i64) -> TrustedCounterparty {
        TrustedCounterparty {
            address: addr.into(),
            first_seen: ts,
        }
    }

    #[test]
    fn long_prefix_and_suffix_match_is_flagged() {
        // Real recipient.
        let real = "0xabcdef1234567890aaaaaaaaaaaaaaaaaaaa1234";
        // Look-alike: same first 6 hex chars (after 0x) and same last 4.
        let fake = "0xabcdef99999999999999999999999999991234";
        let transfers = vec![xfer("t1", fake, "1", 200)];
        let trusted = vec![trust(real, 100)];
        let alerts = detect(&transfers, &trusted, &PoisonConfig::default()).unwrap();
        assert_eq!(alerts.len(), 1);
        assert!(alerts[0].matching_prefix >= 6);
        assert!(alerts[0].is_dust);
    }

    #[test]
    fn exact_match_is_not_flagged() {
        let real = "0xabcdef1234567890aaaaaaaaaaaaaaaaaaaa1234";
        let transfers = vec![xfer("t1", real, "1", 200)];
        let trusted = vec![trust(real, 100)];
        let alerts = detect(&transfers, &trusted, &PoisonConfig::default()).unwrap();
        assert!(alerts.is_empty());
    }

    #[test]
    fn no_match_is_not_flagged() {
        let real = "0xabcdef1234567890aaaaaaaaaaaaaaaaaaaa1234";
        let unrelated = "0x9999999999999999999999999999999999999999";
        let transfers = vec![xfer("t1", unrelated, "1", 200)];
        let trusted = vec![trust(real, 100)];
        let alerts = detect(&transfers, &trusted, &PoisonConfig::default()).unwrap();
        assert!(alerts.is_empty());
    }

    #[test]
    fn pre_existing_send_required() {
        // Trusted entry first_seen is *after* the incoming xfer.
        // That should NOT trigger an alert — we hadn't trusted it yet.
        let real = "0xabcdef1234567890aaaaaaaaaaaaaaaaaaaa1234";
        let fake = "0xabcdef99999999999999999999999999991234";
        let transfers = vec![xfer("t1", fake, "1", 100)];
        let trusted = vec![trust(real, 200)];
        let alerts = detect(&transfers, &trusted, &PoisonConfig::default()).unwrap();
        assert!(alerts.is_empty());
    }

    #[test]
    fn prefix_only_with_zero_suffix_threshold_works() {
        let real = "0xabcdef1234567890aaaaaaaaaaaaaaaaaaaa1234";
        let fake = "0xabcdef9999999999999999999999999999bbbb";
        let transfers = vec![xfer("t1", fake, "1", 200)];
        let trusted = vec![trust(real, 100)];
        let cfg = PoisonConfig {
            min_prefix: 6,
            min_suffix: 0,
            ..Default::default()
        };
        let alerts = detect(&transfers, &trusted, &cfg).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].matching_prefix, 6);
    }

    #[test]
    fn dust_amount_flagged_truthy_for_zero_and_one() {
        assert!(is_dust_amount("0", "1000000"));
        assert!(is_dust_amount("", "1000000"));
        assert!(is_dust_amount("1", "1000000"));
        assert!(is_dust_amount("1000000", "1000000"));
        assert!(!is_dust_amount("1000001", "1000000"));
        assert!(!is_dust_amount("99999999999999999999999", "1000000"));
    }

    #[test]
    fn alerts_sorted_dust_first_then_match_length() {
        let real = "0xabcdef1234567890aaaaaaaaaaaaaaaaaaaa1234";
        let fake_dust = "0xabcdef99999999999999999999999999991234";
        let fake_big = "0xabcdef88888888888888888888888888881234";
        let transfers = vec![
            xfer("t-big", fake_big, "100000000000000000000", 200),
            xfer("t-dust", fake_dust, "1", 201),
        ];
        let trusted = vec![trust(real, 100)];
        let alerts = detect(&transfers, &trusted, &PoisonConfig::default()).unwrap();
        assert_eq!(alerts[0].transfer.txid, "t-dust");
    }

    #[test]
    fn invalid_config_rejected() {
        let cfg = PoisonConfig {
            min_prefix: 0,
            min_suffix: 0,
            dust_threshold: "0".into(),
        };
        let err = detect(&[], &[], &cfg).unwrap_err();
        assert!(matches!(err, PoisonError::InvalidConfig(_)));
    }

    #[test]
    fn case_insensitive_match() {
        let real = "0xABCDEF1234567890aaaaaaaaaaaaaaaaaaaa1234";
        let fake = "0xabcdef99999999999999999999999999991234";
        let transfers = vec![xfer("t1", fake, "1", 200)];
        let trusted = vec![trust(real, 100)];
        let alerts = detect(&transfers, &trusted, &PoisonConfig::default()).unwrap();
        assert_eq!(alerts.len(), 1);
    }

    #[test]
    fn picks_best_match_when_multiple_trusted() {
        let trusted_a = "0xabcdef00000000000000000000000000000000";
        let trusted_b = "0xab0000000000000000000000000000000000ab";
        let fake = "0xabcdef99999999999999999999999999999999";
        let transfers = vec![xfer("t1", fake, "1", 300)];
        let trusted = vec![trust(trusted_a, 100), trust(trusted_b, 100)];
        let cfg = PoisonConfig {
            min_prefix: 4,
            min_suffix: 0,
            ..Default::default()
        };
        let alerts = detect(&transfers, &trusted, &cfg).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].mimicked.address, trusted_a);
    }
}
