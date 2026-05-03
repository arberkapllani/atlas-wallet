//! P2WPKH (BIP-84) transaction builder + signer.
//!
//! Fee algorithm: greedy largest-first UTXO selection until inputs cover
//! `amount + fee`, with `fee = vbytes(inputs, outputs) * sat_per_vb`.
//! A change output back to the sender is added when the residual exceeds
//! the dust limit (546 sats).

use bitcoin::absolute::LockTime;
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::Message;
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::transaction::Version;
use bitcoin::{
    consensus, Address, Amount as BtcAmount, CompressedPublicKey, Network, OutPoint, PublicKey,
    ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness,
};
use std::str::FromStr;

const DUST_LIMIT: u64 = 546;

/// A spendable UTXO from the mempool.space API.
#[derive(Debug, Clone)]
pub struct Utxo {
    /// Hex txid of the funding transaction.
    pub txid: String,
    /// Output index.
    pub vout: u32,
    /// Output value in satoshi.
    pub value_sats: u64,
}

/// Result of [`build_and_sign_p2wpkh`].
#[derive(Debug)]
pub struct BuiltTx {
    /// Hex-serialized fully signed transaction.
    pub raw_hex: String,
    /// Hex txid (computed locally; matches what a node will return).
    pub txid: String,
    /// Final fee, in satoshi.
    pub fee_sats: u64,
}

/// Build and sign a 1-or-many-input → 1-or-2-output P2WPKH transaction.
pub fn build_and_sign_p2wpkh(
    from: &str,
    to: &str,
    value_sats: u64,
    sat_per_vb: u64,
    utxos: &[Utxo],
    private_key: &[u8; 32],
) -> Result<BuiltTx, String> {
    let secp = secp256k1::Secp256k1::new();
    let sk = secp256k1::SecretKey::from_slice(private_key).map_err(|e| e.to_string())?;
    let pk = secp256k1::PublicKey::from_secret_key(&secp, &sk);
    let bpk = PublicKey::new(pk);
    let cpk = CompressedPublicKey::try_from(bpk).map_err(|e| e.to_string())?;

    let from_addr = Address::from_str(from)
        .map_err(|e| e.to_string())?
        .require_network(Network::Bitcoin)
        .map_err(|e| e.to_string())?;
    let to_addr = Address::from_str(to)
        .map_err(|e| e.to_string())?
        .require_network(Network::Bitcoin)
        .map_err(|e| e.to_string())?;

    // Sanity: ensure derived address matches `from`.
    let derived = Address::p2wpkh(&cpk, Network::Bitcoin);
    if derived != from_addr {
        return Err("private key does not match `from` address".into());
    }

    // Greedy: largest-first selection.
    let mut sorted = utxos.to_vec();
    sorted.sort_by_key(|u| std::cmp::Reverse(u.value_sats));

    // Iteratively add inputs until we cover (value + fee).
    let mut selected: Vec<Utxo> = Vec::new();
    let mut total_in: u64 = 0;

    for u in &sorted {
        selected.push(u.clone());
        total_in += u.value_sats;
        // Try with change first, then without.
        let fee_with_change = estimated_vbytes(selected.len(), 2) * sat_per_vb;
        if total_in >= value_sats + fee_with_change + DUST_LIMIT {
            return finalize(
                &selected,
                from_addr,
                to_addr,
                value_sats,
                fee_with_change,
                total_in,
                true,
                &cpk,
                &sk,
            );
        }
        let fee_no_change = estimated_vbytes(selected.len(), 1) * sat_per_vb;
        if total_in == value_sats + fee_no_change {
            return finalize(
                &selected,
                from_addr,
                to_addr,
                value_sats,
                fee_no_change,
                total_in,
                false,
                &cpk,
                &sk,
            );
        }
    }

    Err("insufficient funds".into())
}

#[allow(clippy::too_many_arguments)]
fn finalize(
    inputs: &[Utxo],
    from_addr: Address,
    to_addr: Address,
    value_sats: u64,
    fee_sats: u64,
    total_in: u64,
    with_change: bool,
    cpk: &CompressedPublicKey,
    sk: &secp256k1::SecretKey,
) -> Result<BuiltTx, String> {
    let secp = secp256k1::Secp256k1::new();

    let mut tx_in: Vec<TxIn> = Vec::with_capacity(inputs.len());
    let mut prev_outs: Vec<TxOut> = Vec::with_capacity(inputs.len());
    for u in inputs {
        let txid_bytes = hex_reverse(&u.txid).map_err(|e| e.to_string())?;
        let txid = bitcoin::Txid::from_raw_hash(
            <bitcoin::hashes::sha256d::Hash as bitcoin::hashes::Hash>::from_slice(&txid_bytes)
                .map_err(|e| e.to_string())?,
        );
        tx_in.push(TxIn {
            previous_output: OutPoint::new(txid, u.vout),
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        });
        prev_outs.push(TxOut {
            value: BtcAmount::from_sat(u.value_sats),
            script_pubkey: from_addr.script_pubkey(),
        });
    }

    let mut tx_out: Vec<TxOut> = Vec::with_capacity(2);
    tx_out.push(TxOut {
        value: BtcAmount::from_sat(value_sats),
        script_pubkey: to_addr.script_pubkey(),
    });
    if with_change {
        let change = total_in
            .checked_sub(value_sats + fee_sats)
            .ok_or("fee math overflow")?;
        tx_out.push(TxOut {
            value: BtcAmount::from_sat(change),
            script_pubkey: from_addr.script_pubkey(),
        });
    }

    let mut tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: tx_in,
        output: tx_out,
    };

    // Witness signatures for each input.
    let mut cache = SighashCache::new(&tx);
    let mut sigs: Vec<Witness> = Vec::with_capacity(inputs.len());
    for (i, prev) in prev_outs.iter().enumerate() {
        let sighash = cache
            .p2wpkh_signature_hash(i, &prev.script_pubkey, prev.value, EcdsaSighashType::All)
            .map_err(|e| e.to_string())?;
        let msg = Message::from_digest(sighash.to_byte_array());
        let sig = secp.sign_ecdsa(&msg, sk);
        let mut sig_bytes = sig.serialize_der().to_vec();
        sig_bytes.push(EcdsaSighashType::All as u8);
        let mut w = Witness::new();
        w.push(sig_bytes);
        w.push(cpk.to_bytes());
        sigs.push(w);
    }
    for (i, w) in sigs.into_iter().enumerate() {
        tx.input[i].witness = w;
    }

    let raw = consensus::encode::serialize_hex(&tx);
    let txid = tx.compute_txid().to_string();
    Ok(BuiltTx {
        raw_hex: raw,
        txid,
        fee_sats,
    })
}

/// Approximate vbytes for a P2WPKH tx with `n_in` inputs and `n_out` outputs.
/// Source: bitcoin core wallet heuristic — ~68 vB per p2wpkh input + 31 vB
/// per p2wpkh output + 11 vB overhead.
fn estimated_vbytes(n_in: usize, n_out: usize) -> u64 {
    11 + (n_in as u64) * 68 + (n_out as u64) * 31
}

fn hex_reverse(s: &str) -> Result<Vec<u8>, hex::FromHexError> {
    let mut v = hex::decode(s)?;
    v.reverse();
    Ok(v)
}
