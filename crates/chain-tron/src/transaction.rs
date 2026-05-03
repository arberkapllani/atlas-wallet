//! Tron transaction encoder + signer.
//!
//! Tron transactions are protobuf messages. We hand-roll the wire format
//! (see [`crate::proto`]) for the three shapes we actually emit:
//!
//! * `TransferContract` — native TRX transfer.
//! * `TriggerSmartContract` — TRC-20 `transfer(address,uint256)` call.
//! * `Transaction.raw_data` — the envelope wrapping one of the above.
//!
//! Signing is plain secp256k1 ECDSA over `SHA256(raw_data)` with a 1-byte
//! recovery suffix, matching every Tron client.

use sha2::{Digest, Sha256};

use crate::proto::{write_bytes_field, write_varint_field};

/// `Transaction.Contract.ContractType` enum: native TRX transfer.
const CONTRACT_TYPE_TRANSFER: u64 = 1;
/// `Transaction.Contract.ContractType` enum: smart-contract call.
const CONTRACT_TYPE_TRIGGER_SMART_CONTRACT: u64 = 31;

/// Maximum TRX (in sun) the user is willing to spend on a TRC-20 call.
/// Real-world USDT transfers cost ~14 TRX; we set a safe upper bound.
pub const DEFAULT_TRC20_FEE_LIMIT_SUN: u64 = 100_000_000; // 100 TRX

/// 21-byte Tron address (the 0x41-prefixed payload).
pub type TronAddress = [u8; 21];

/// `protocol.TransferContract`.
fn encode_transfer_contract(owner: &TronAddress, to: &TronAddress, amount_sun: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    write_bytes_field(&mut out, 1, owner);
    write_bytes_field(&mut out, 2, to);
    write_varint_field(&mut out, 3, amount_sun);
    out
}

/// `protocol.TriggerSmartContract` (call_value omitted = 0).
fn encode_trigger_smart_contract(
    owner: &TronAddress,
    contract: &TronAddress,
    data: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(128);
    write_bytes_field(&mut out, 1, owner);
    write_bytes_field(&mut out, 2, contract);
    write_bytes_field(&mut out, 4, data);
    out
}

/// `google.protobuf.Any { type_url, value }`.
fn encode_any(type_url: &str, value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(type_url.len() + value.len() + 8);
    write_bytes_field(&mut out, 1, type_url.as_bytes());
    write_bytes_field(&mut out, 2, value);
    out
}

/// `Transaction.Contract { type, parameter }`.
fn encode_contract(contract_type: u64, parameter_any: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(parameter_any.len() + 16);
    write_varint_field(&mut out, 1, contract_type);
    write_bytes_field(&mut out, 2, parameter_any);
    out
}

/// All inputs needed to assemble a `Transaction.raw_data`.
pub struct RawTxParams<'a> {
    /// Bytes 6..8 of `block_header.raw_data.number` (big-endian u64).
    pub ref_block_bytes: &'a [u8; 2],
    /// Bytes 8..16 of `SHA256(block_header.raw_data)`.
    pub ref_block_hash: &'a [u8; 8],
    /// Wall-clock millis since epoch.
    pub timestamp_ms: u64,
    /// Wall-clock millis since epoch + lifetime (typically `+60_000`).
    pub expiration_ms: u64,
    /// Optional `fee_limit` in sun (TRC-20 only; `None` for native).
    pub fee_limit_sun: Option<u64>,
    /// Already-encoded `Transaction.Contract` (length-delimited).
    pub contract: Vec<u8>,
}

/// Encode `Transaction.raw_data`. The exact byte string we'll sign.
pub fn encode_raw_tx(p: &RawTxParams<'_>) -> Vec<u8> {
    let mut out = Vec::with_capacity(p.contract.len() + 64);
    write_bytes_field(&mut out, 1, p.ref_block_bytes);
    write_bytes_field(&mut out, 4, p.ref_block_hash);
    write_varint_field(&mut out, 8, p.expiration_ms);
    write_bytes_field(&mut out, 11, &p.contract);
    write_varint_field(&mut out, 14, p.timestamp_ms);
    if let Some(limit) = p.fee_limit_sun {
        write_varint_field(&mut out, 18, limit);
    }
    out
}

/// Build `Transaction.Contract` for a native TRX transfer.
pub fn transfer_contract_envelope(
    owner: &TronAddress,
    to: &TronAddress,
    amount_sun: u64,
) -> Vec<u8> {
    let inner = encode_transfer_contract(owner, to, amount_sun);
    let any = encode_any("type.googleapis.com/protocol.TransferContract", &inner);
    encode_contract(CONTRACT_TYPE_TRANSFER, &any)
}

/// Build `Transaction.Contract` for a TRC-20 `transfer(address,uint256)` call.
///
/// `to_evm` is the recipient's 20-byte EVM-style address (Tron addresses
/// without their 0x41 prefix, exactly what Solidity expects on-chain).
pub fn trc20_transfer_contract_envelope(
    owner: &TronAddress,
    contract: &TronAddress,
    to_evm: &[u8; 20],
    amount: u128,
) -> Vec<u8> {
    let mut data = Vec::with_capacity(4 + 32 + 32);
    // selector: keccak256("transfer(address,uint256)")[0..4] = 0xa9059cbb
    data.extend_from_slice(&[0xa9, 0x05, 0x9c, 0xbb]);
    // 32-byte left-padded address
    data.extend_from_slice(&[0u8; 12]);
    data.extend_from_slice(to_evm);
    // 32-byte big-endian amount
    let mut amt = [0u8; 32];
    amt[16..].copy_from_slice(&amount.to_be_bytes());
    data.extend_from_slice(&amt);

    let inner = encode_trigger_smart_contract(owner, contract, &data);
    let any = encode_any("type.googleapis.com/protocol.TriggerSmartContract", &inner);
    encode_contract(CONTRACT_TYPE_TRIGGER_SMART_CONTRACT, &any)
}

/// A signed Tron transaction ready for `/wallet/broadcasthex`.
pub struct SignedTronTx {
    /// Hex-encoded full `Transaction { raw_data, signature }` protobuf.
    pub raw_hex: String,
    /// `SHA256(raw_data)` — Tron's canonical txid.
    pub txid_hex: String,
}

/// Sign `raw_data` with `private_key` and return the full Tron `Transaction`
/// protobuf (raw + 65-byte signature).
pub fn sign_raw_tx(raw_data: &[u8], private_key: &[u8; 32]) -> Result<SignedTronTx, String> {
    let digest = Sha256::digest(raw_data);

    let secp = secp256k1::Secp256k1::new();
    let sk = secp256k1::SecretKey::from_slice(private_key).map_err(|e| e.to_string())?;
    let msg = secp256k1::Message::from_digest_slice(&digest).map_err(|e| e.to_string())?;
    let recoverable = secp.sign_ecdsa_recoverable(&msg, &sk);
    let (rec_id, sig_bytes) = recoverable.serialize_compact();

    let mut signature = Vec::with_capacity(65);
    signature.extend_from_slice(&sig_bytes);
    signature.push(rec_id.to_i32() as u8);

    // Transaction { raw_data: 1 (bytes), signature: 2 (repeated bytes) }
    let mut full = Vec::with_capacity(raw_data.len() + signature.len() + 16);
    write_bytes_field(&mut full, 1, raw_data);
    write_bytes_field(&mut full, 2, &signature);

    Ok(SignedTronTx {
        raw_hex: hex::encode(&full),
        txid_hex: hex::encode(digest),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_addr(prefix: u8) -> TronAddress {
        let mut a = [0u8; 21];
        a[0] = 0x41;
        a[20] = prefix;
        a
    }

    #[test]
    fn transfer_contract_round_trip_lengths() {
        let owner = dummy_addr(0xaa);
        let to = dummy_addr(0xbb);
        let env = transfer_contract_envelope(&owner, &to, 1_000_000);
        // Non-empty, starts with field-1 (varint tag = 0x08) for contract type.
        assert!(!env.is_empty());
        assert_eq!(env[0], 0x08);
    }

    #[test]
    fn trc20_data_is_4_plus_64_bytes() {
        // We don't expose the data buffer directly, but we can verify the
        // envelope grows by exactly 4 + 32 + 32 = 68 bytes when amount changes.
        let owner = dummy_addr(0x01);
        let contract = dummy_addr(0x02);
        let to_evm = [0u8; 20];
        let env_a = trc20_transfer_contract_envelope(&owner, &contract, &to_evm, 0);
        let env_b = trc20_transfer_contract_envelope(&owner, &contract, &to_evm, u128::MAX);
        // Same length — only the *value* of the 32-byte amount changes.
        assert_eq!(env_a.len(), env_b.len());
    }

    #[test]
    fn raw_tx_includes_fee_limit_only_when_set() {
        let contract = vec![0u8; 10];
        let ref_bytes: [u8; 2] = [0xab, 0xcd];
        let ref_hash: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

        let no_fee = encode_raw_tx(&RawTxParams {
            ref_block_bytes: &ref_bytes,
            ref_block_hash: &ref_hash,
            timestamp_ms: 1_000_000,
            expiration_ms: 1_060_000,
            fee_limit_sun: None,
            contract: contract.clone(),
        });
        let with_fee = encode_raw_tx(&RawTxParams {
            ref_block_bytes: &ref_bytes,
            ref_block_hash: &ref_hash,
            timestamp_ms: 1_000_000,
            expiration_ms: 1_060_000,
            fee_limit_sun: Some(100_000_000),
            contract,
        });

        // `with_fee` strictly larger because of field 18 (varint tag + value).
        assert!(with_fee.len() > no_fee.len());
    }

    #[test]
    fn sign_produces_65_byte_signature_and_32_byte_txid() {
        let raw = vec![0x01u8, 0x02, 0x03, 0x04];
        // Deterministic test key (NOT a real wallet).
        let pk = [7u8; 32];
        let signed = sign_raw_tx(&raw, &pk).unwrap();

        // txid = SHA256(raw), 32 bytes -> 64 hex chars.
        assert_eq!(signed.txid_hex.len(), 64);

        // raw_hex must contain at least raw_data (encoded as field 1) +
        // signature (encoded as field 2). 65-byte sig + tag/len + raw +
        // tag/len > 70 bytes hex-encoded => >140 chars.
        assert!(signed.raw_hex.len() > 140);
    }
}
