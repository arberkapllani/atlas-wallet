//! Native Bitcoin multi-signature ("multisig") for Atlas.
//!
//! Atlas wants to give regular users a real path to a 2-of-3 / 3-of-5
//! cold-storage setup without trusting Coinbase Custody or relying on a
//! browser extension. This crate handles the *descriptor* side of that:
//!
//! - Build an N-of-M `wsh(multi(...))` descriptor from a list of `xpub`s.
//! - Derive ranged receive / change addresses for the first chain (BIP-48
//!   path `m/48'/coin'/account'/2'/0/i`).
//! - Parse a previously-saved descriptor string back into a usable wallet.
//!
//! PSBT assembly + collaborative signing land in a follow-up; the
//! `bitcoin` crate's `Psbt` type already covers the wire format.

use std::str::FromStr;

use bitcoin::{
    bip32::{DerivationPath, Xpub},
    secp256k1::Secp256k1,
    Address, Network,
};
use miniscript::{
    descriptor::{DescriptorPublicKey, DescriptorXKey, Wildcard},
    Descriptor,
};
use serde::{Deserialize, Serialize};

pub mod invite;
pub mod psbt;

#[derive(Debug, thiserror::Error)]
pub enum MultisigError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("descriptor: {0}")]
    Descriptor(String),
    #[error("address derivation: {0}")]
    AddressDerivation(String),
}

pub type Result<T> = std::result::Result<T, MultisigError>;

/// A single signer in the multisig policy. `key_origin` ties the xpub
/// back to its master key fingerprint and derivation path so a hardware
/// wallet can later prove ownership before signing.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct MultisigSigner {
    /// 8-character hex master fingerprint (e.g. `"d34db33f"`).
    pub fingerprint: String,
    /// Origin path — typically `m/48'/0'/0'/2'` for a BIP-48 P2WSH
    /// account on mainnet.
    pub origin: String,
    /// Account-level extended public key (`xpub.../zpub.../...`).
    pub xpub: String,
}

/// `threshold`-of-`signers` policy. Atlas refuses to construct a wallet
/// with `threshold == 0`, `threshold > signers.len()`, or a signer set
/// containing fewer than two parties — those are footguns, not policies.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct MultisigPolicy {
    pub threshold: usize,
    pub signers: Vec<MultisigSigner>,
    /// `Mainnet` for production funds, `Testnet` for everything else.
    pub network: NetworkChoice,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum NetworkChoice {
    Mainnet,
    Testnet,
}

impl From<NetworkChoice> for Network {
    fn from(value: NetworkChoice) -> Self {
        match value {
            NetworkChoice::Mainnet => Network::Bitcoin,
            NetworkChoice::Testnet => Network::Testnet,
        }
    }
}

/// Compiled descriptor + the policy it came from. Atlas stores the
/// descriptor string verbatim so a recovery from another wallet (Sparrow,
/// Specter) is a simple paste.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct MultisigWallet {
    pub policy: MultisigPolicy,
    /// `wsh(sortedmulti(...))` descriptor string with `/<0;1>/*` ranged
    /// suffix — the standard BIP-389 form for receive + change.
    pub descriptor: String,
}

impl MultisigPolicy {
    pub fn validate(&self) -> Result<()> {
        if self.signers.len() < 2 {
            return Err(MultisigError::InvalidInput(
                "multisig requires at least two signers".into(),
            ));
        }
        if self.signers.len() > 15 {
            return Err(MultisigError::InvalidInput(
                "multisig is capped at 15 signers (consensus limit)".into(),
            ));
        }
        if self.threshold == 0 || self.threshold > self.signers.len() {
            return Err(MultisigError::InvalidInput(format!(
                "threshold must be 1..={}",
                self.signers.len()
            )));
        }
        // Reject duplicate xpubs — this is a real footgun: a policy that
        // looks "2-of-3" but has the same key listed twice degrades to
        // 1-of-2 when one signer's seed leaks.
        let mut seen = std::collections::HashSet::new();
        for s in &self.signers {
            if !seen.insert(s.xpub.trim()) {
                return Err(MultisigError::InvalidInput(
                    "duplicate xpub in signer set".into(),
                ));
            }
        }
        Ok(())
    }
}

/// Build a `wsh(sortedmulti(...))` descriptor from `policy`. Uses
/// `sortedmulti` rather than plain `multi` so the address depends only
/// on the *set* of keys, not the order — important for collaborative
/// flows where each party may list signers differently.
pub fn build_descriptor(policy: &MultisigPolicy) -> Result<MultisigWallet> {
    policy.validate()?;

    let mut keys: Vec<DescriptorPublicKey> = Vec::with_capacity(policy.signers.len());
    for s in &policy.signers {
        let fp = s.fingerprint.trim();
        if fp.len() != 8 || !fp.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(MultisigError::InvalidInput(format!(
                "invalid fingerprint '{}': expected 8 hex chars",
                s.fingerprint
            )));
        }
        let origin = DerivationPath::from_str(s.origin.trim())
            .map_err(|e| MultisigError::InvalidInput(format!("origin path: {e}")))?;
        let xpub = Xpub::from_str(s.xpub.trim())
            .map_err(|e| MultisigError::InvalidInput(format!("xpub: {e}")))?;
        let fingerprint = bitcoin::bip32::Fingerprint::from_str(fp)
            .map_err(|e| MultisigError::InvalidInput(format!("fingerprint parse: {e}")))?;
        keys.push(DescriptorPublicKey::XPub(DescriptorXKey {
            origin: Some((fingerprint, origin)),
            xkey: xpub,
            // No derivation path baked in: the multipath suffix
            // (`/<0;1>/*`) we append below provides receive + change.
            derivation_path: DerivationPath::default(),
            wildcard: Wildcard::None,
        }));
    }

    // sortedmulti(threshold, key1, key2, ...) — descriptors are
    // unambiguous to parse and cross-wallet portable.
    let key_list = keys
        .iter()
        .map(|k| format!("{}/<0;1>/*", k))
        .collect::<Vec<_>>()
        .join(",");
    let inner = format!("sortedmulti({},{})", policy.threshold, key_list);
    let descriptor_str = format!("wsh({})", inner);

    // Sanity-check by parsing it back through miniscript.
    Descriptor::<DescriptorPublicKey>::from_str(&descriptor_str)
        .map_err(|e| MultisigError::Descriptor(e.to_string()))?;

    Ok(MultisigWallet {
        policy: policy.clone(),
        descriptor: descriptor_str,
    })
}

/// Derive the receive (chain=0) or change (chain=1) address at `index`.
pub fn derive_address(wallet: &MultisigWallet, change: bool, index: u32) -> Result<String> {
    let secp = Secp256k1::verification_only();
    let desc = Descriptor::<DescriptorPublicKey>::from_str(&wallet.descriptor)
        .map_err(|e| MultisigError::Descriptor(e.to_string()))?;
    // BIP-389 multipath: split into the two single-path descriptors and
    // pick the requested chain.
    let paths = desc
        .clone()
        .into_single_descriptors()
        .map_err(|e| MultisigError::Descriptor(e.to_string()))?;
    let chain_idx = if change { 1 } else { 0 };
    let single = paths
        .get(chain_idx)
        .ok_or_else(|| MultisigError::Descriptor("descriptor missing chain path".into()))?;
    let derived = single
        .at_derivation_index(index)
        .map_err(|e| MultisigError::AddressDerivation(e.to_string()))?
        .derived_descriptor(&secp)
        .map_err(|e| MultisigError::AddressDerivation(e.to_string()))?;
    let addr: Address = derived
        .address(wallet.policy.network.into())
        .map_err(|e| MultisigError::AddressDerivation(e.to_string()))?;
    Ok(addr.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::bip32::{ChildNumber, Xpriv};

    /// Derive three distinct, well-formed xpubs from a deterministic
    /// seed so the tests don't depend on network state or hardcoded
    /// strings whose checksum could drift between releases.
    fn three_xpubs() -> [String; 3] {
        let secp = Secp256k1::new();
        // 32-byte seed; same shape as `Mnemonic::to_seed` would produce.
        let seed = [0x42u8; 32];
        let master = Xpriv::new_master(Network::Bitcoin, &seed).unwrap();
        let mut out = Vec::with_capacity(3);
        for i in 0u32..3 {
            // m/48'/0'/i'/2' (BIP-48 P2WSH for account i)
            let path = vec![
                ChildNumber::from_hardened_idx(48).unwrap(),
                ChildNumber::from_hardened_idx(0).unwrap(),
                ChildNumber::from_hardened_idx(i).unwrap(),
                ChildNumber::from_hardened_idx(2).unwrap(),
            ];
            let derived = master.derive_priv(&secp, &path).unwrap();
            let xpub = Xpub::from_priv(&secp, &derived);
            out.push(xpub.to_string());
        }
        [out[0].clone(), out[1].clone(), out[2].clone()]
    }

    fn signer(fp: &str, origin: &str, xpub: &str) -> MultisigSigner {
        MultisigSigner {
            fingerprint: fp.into(),
            origin: origin.into(),
            xpub: xpub.into(),
        }
    }

    fn valid_policy() -> MultisigPolicy {
        let xs = three_xpubs();
        MultisigPolicy {
            threshold: 2,
            signers: vec![
                signer("11111111", "m/48h/0h/0h/2h", &xs[0]),
                signer("22222222", "m/48h/0h/1h/2h", &xs[1]),
                signer("33333333", "m/48h/0h/2h/2h", &xs[2]),
            ],
            network: NetworkChoice::Mainnet,
        }
    }

    #[test]
    fn validate_rejects_zero_threshold() {
        let mut p = valid_policy();
        p.threshold = 0;
        assert!(matches!(p.validate(), Err(MultisigError::InvalidInput(_))));
    }

    #[test]
    fn validate_rejects_threshold_above_signers() {
        let mut p = valid_policy();
        p.threshold = 4;
        assert!(matches!(p.validate(), Err(MultisigError::InvalidInput(_))));
    }

    #[test]
    fn validate_rejects_single_signer() {
        let mut p = valid_policy();
        p.signers.truncate(1);
        p.threshold = 1;
        assert!(matches!(p.validate(), Err(MultisigError::InvalidInput(_))));
    }

    #[test]
    fn validate_rejects_duplicate_xpubs() {
        let mut p = valid_policy();
        p.signers[1].xpub = p.signers[0].xpub.clone();
        assert!(matches!(p.validate(), Err(MultisigError::InvalidInput(_))));
    }

    #[test]
    fn validate_rejects_more_than_15_signers() {
        let mut p = valid_policy();
        // 16 distinct signer entries with distinct xpubs. We don't need
        // them to be valid base58 — `validate()` doesn't decode them, so
        // this still trips the size cap before any other check.
        p.signers = (0..16u32)
            .map(|i| MultisigSigner {
                fingerprint: "aaaaaaaa".into(),
                origin: "m/48h/0h/0h/2h".into(),
                xpub: format!("xpub-fake-{}", i),
            })
            .collect();
        p.threshold = 2;
        assert!(matches!(p.validate(), Err(MultisigError::InvalidInput(_))));
    }

    #[test]
    fn build_descriptor_emits_sortedmulti() {
        let wallet = build_descriptor(&valid_policy()).unwrap();
        assert!(wallet.descriptor.starts_with("wsh(sortedmulti(2,"));
        assert!(wallet.descriptor.contains("/<0;1>/*"));
        // miniscript may render the origin path with `'` instead of `h`;
        // either form is accepted by spec, so we just check the
        // fingerprint shows up.
        assert!(wallet.descriptor.contains("[11111111"));
    }

    #[test]
    fn derive_address_yields_bech32_p2wsh() {
        let wallet = build_descriptor(&valid_policy()).unwrap();
        let recv0 = derive_address(&wallet, false, 0).unwrap();
        // P2WSH addresses on mainnet start with `bc1q` (segwit v0).
        assert!(recv0.starts_with("bc1q"), "got {}", recv0);
        let recv1 = derive_address(&wallet, false, 1).unwrap();
        assert_ne!(recv0, recv1);
        // Change addresses live on a different chain — should differ from
        // the receive index 0 address.
        let change0 = derive_address(&wallet, true, 0).unwrap();
        assert_ne!(recv0, change0);
    }

    #[test]
    fn descriptor_round_trips_through_parser() {
        let wallet = build_descriptor(&valid_policy()).unwrap();
        // Re-parse and re-derive index 0; addresses must match.
        let again = MultisigWallet {
            policy: wallet.policy.clone(),
            descriptor: wallet.descriptor.clone(),
        };
        let a = derive_address(&wallet, false, 0).unwrap();
        let b = derive_address(&again, false, 0).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn invalid_fingerprint_rejected() {
        let mut p = valid_policy();
        p.signers[0].fingerprint = "ZZZZ".into();
        assert!(matches!(
            build_descriptor(&p),
            Err(MultisigError::InvalidInput(_))
        ));
    }
}
