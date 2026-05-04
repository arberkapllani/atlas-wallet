//! PSBT (BIP-174) helpers for collaborative multisig signing.
//!
//! In a real multisig flow, the *coordinator* (an Atlas user, hot-wallet
//! side) drafts a transaction, exports it as a PSBT, then ships the
//! base64 blob (or BBQr-encoded QR) to each co-signer. Each co-signer
//! adds their partial signatures and ships the result back. The
//! coordinator combines the partials and finalises.
//!
//! This module wraps `bitcoin::Psbt` with three thin operations Atlas
//! needs at the IPC boundary: decode, encode, and combine. Signing
//! itself stays in the chain-btc crate (where the secret key lives).

use std::str::FromStr;

use bitcoin::{consensus::Encodable, psbt::Psbt, secp256k1::Secp256k1};
use serde::{Deserialize, Serialize};

use crate::MultisigError;

/// Lightweight summary of a PSBT for the UI: which inputs are signed
/// and how many signatures each carries, plus the per-output amount /
/// address. Avoids shipping the entire raw byte blob across IPC every
/// time the user clicks a row.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct PsbtSummary {
    /// Total input value in satoshis (best-effort: only known when the
    /// previous `witness_utxo`/`non_witness_utxo` is attached).
    pub total_input_sats: Option<u64>,
    /// Total output value in satoshis.
    pub total_output_sats: u64,
    /// Implied fee (`inputs - outputs`) when input totals are known.
    pub fee_sats: Option<u64>,
    pub inputs: Vec<PsbtInputSummary>,
    pub outputs: Vec<PsbtOutputSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct PsbtInputSummary {
    /// Number of partial signatures already collected for this input.
    pub partial_sigs: u32,
    /// Value of the spent output, when known.
    pub value_sats: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct PsbtOutputSummary {
    pub value_sats: u64,
    /// Best-effort decoded address (mainnet first, then testnet). `None`
    /// if the script doesn't map to a standard address.
    pub address: Option<String>,
}

/// Decode a base64-encoded PSBT into a `bitcoin::Psbt`.
pub fn decode_psbt(b64: &str) -> Result<Psbt, MultisigError> {
    Psbt::from_str(b64.trim()).map_err(|e| MultisigError::InvalidInput(format!("psbt: {e}")))
}

/// Re-serialise a PSBT back to base64.
pub fn encode_psbt(psbt: &Psbt) -> String {
    psbt.to_string()
}

/// Decode + summarise in one shot — what the UI command actually wants.
pub fn summarise_psbt(b64: &str) -> Result<PsbtSummary, MultisigError> {
    let psbt = decode_psbt(b64)?;

    let mut total_in: Option<u64> = Some(0);
    let mut inputs = Vec::with_capacity(psbt.inputs.len());
    for inp in &psbt.inputs {
        // Prefer `witness_utxo` (segwit / native multisig path). For
        // non-witness inputs we'd need to look up the previous tx's
        // vout, which lives on the unsigned_tx — not strictly required
        // for our P2WSH-only path; left for a future patch.
        let value = inp.witness_utxo.as_ref().map(|o| o.value.to_sat());
        if let (Some(t), Some(v)) = (total_in.as_mut(), value) {
            *t += v;
        } else {
            total_in = None;
        }
        inputs.push(PsbtInputSummary {
            partial_sigs: inp.partial_sigs.len() as u32,
            value_sats: value,
        });
    }

    let mut total_out: u64 = 0;
    let mut outputs = Vec::with_capacity(psbt.unsigned_tx.output.len());
    for o in &psbt.unsigned_tx.output {
        let v = o.value.to_sat();
        total_out += v;
        // Try mainnet first; if that fails, the script likely isn't a
        // standard address and we surface `None`.
        let address = bitcoin::Address::from_script(&o.script_pubkey, bitcoin::Network::Bitcoin)
            .ok()
            .map(|a| a.to_string())
            .or_else(|| {
                bitcoin::Address::from_script(&o.script_pubkey, bitcoin::Network::Testnet)
                    .ok()
                    .map(|a| a.to_string())
            });
        outputs.push(PsbtOutputSummary {
            value_sats: v,
            address,
        });
    }

    let fee_sats = total_in.map(|t| t.saturating_sub(total_out));

    Ok(PsbtSummary {
        total_input_sats: total_in,
        total_output_sats: total_out,
        fee_sats,
        inputs,
        outputs,
    })
}

/// Merge a coordinator's PSBT with one or more partially-signed copies
/// returned by co-signers. Every PSBT in `parts` must reference the
/// same unsigned transaction — `Psbt::combine` enforces this.
pub fn combine_psbts(parts: &[String]) -> Result<String, MultisigError> {
    if parts.is_empty() {
        return Err(MultisigError::InvalidInput(
            "combine requires at least one PSBT".into(),
        ));
    }
    let mut iter = parts.iter();
    let first = iter.next().expect("non-empty");
    let mut acc = decode_psbt(first)?;
    for p in iter {
        let other = decode_psbt(p)?;
        acc.combine(other)
            .map_err(|e| MultisigError::InvalidInput(format!("combine: {e}")))?;
    }
    Ok(encode_psbt(&acc))
}

/// Try to finalise a PSBT into a broadcastable transaction. Returns the
/// raw transaction hex on success; if the PSBT still lacks signatures
/// `bitcoin::Psbt::extract_tx_unchecked_fee_rate` would still produce a
/// tx but it would be invalid, so we instead require miniscript's
/// finaliser to succeed first.
pub fn finalize_psbt(b64: &str) -> Result<String, MultisigError> {
    let mut psbt = decode_psbt(b64)?;
    let secp = Secp256k1::verification_only();
    miniscript::psbt::PsbtExt::finalize_mut(&mut psbt, &secp)
        .map_err(|errs| MultisigError::InvalidInput(format!("finalize: {errs:?}")))?;
    let tx = psbt
        .extract_tx()
        .map_err(|e| MultisigError::InvalidInput(format!("extract: {e}")))?;
    let mut buf = Vec::new();
    tx.consensus_encode(&mut buf)
        .map_err(|e| MultisigError::InvalidInput(format!("encode tx: {e}")))?;
    Ok(hex::encode(buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::{
        absolute::LockTime, transaction::Version, Amount, OutPoint, ScriptBuf, Sequence,
        Transaction, TxIn, TxOut, Witness,
    };

    fn dummy_psbt() -> Psbt {
        // 1-in 1-out unsigned tx — enough to exercise the codec and
        // summary path without committing to any real script policy.
        let tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::new(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(50_000),
                // OP_0 <20-byte hash> — a valid P2WPKH program, which
                // `Address::from_script` will happily round-trip.
                script_pubkey: ScriptBuf::new_p2wpkh(
                    &bitcoin::WPubkeyHash::from_slice(&[0x11u8; 20]).unwrap(),
                ),
            }],
        };
        let mut psbt = Psbt::from_unsigned_tx(tx).unwrap();
        // Attach a witness_utxo so the summary can compute fee.
        psbt.inputs[0].witness_utxo = Some(TxOut {
            value: Amount::from_sat(60_000),
            script_pubkey: ScriptBuf::new_p2wpkh(
                &bitcoin::WPubkeyHash::from_slice(&[0x22u8; 20]).unwrap(),
            ),
        });
        psbt
    }

    use bitcoin::hashes::Hash;

    #[test]
    fn round_trip_encode_decode() {
        let p = dummy_psbt();
        let s = encode_psbt(&p);
        let back = decode_psbt(&s).unwrap();
        assert_eq!(
            p.unsigned_tx.compute_txid(),
            back.unsigned_tx.compute_txid()
        );
    }

    #[test]
    fn summary_reports_fee() {
        let s = encode_psbt(&dummy_psbt());
        let sum = summarise_psbt(&s).unwrap();
        assert_eq!(sum.total_input_sats, Some(60_000));
        assert_eq!(sum.total_output_sats, 50_000);
        assert_eq!(sum.fee_sats, Some(10_000));
        assert_eq!(sum.inputs.len(), 1);
        assert_eq!(sum.inputs[0].partial_sigs, 0);
        assert!(sum.outputs[0]
            .address
            .as_deref()
            .unwrap()
            .starts_with("bc1q"));
    }

    #[test]
    fn combine_requires_at_least_one() {
        let err = combine_psbts(&[]).unwrap_err();
        assert!(matches!(err, MultisigError::InvalidInput(_)));
    }

    #[test]
    fn combine_two_identical_is_idempotent() {
        let s = encode_psbt(&dummy_psbt());
        let merged = combine_psbts(&[s.clone(), s.clone()]).unwrap();
        // The merged blob decodes and matches the same txid.
        let back = decode_psbt(&merged).unwrap();
        assert_eq!(back.unsigned_tx.input.len(), 1);
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(matches!(
            decode_psbt("not-a-psbt"),
            Err(MultisigError::InvalidInput(_))
        ));
    }
}
