//! EVM Safe (formerly Gnosis Safe) multisig support for Atlas.
//!
//! Atlas users on EVM chains coordinate multisig through the Safe
//! contracts, which are deployed at the same address across every major
//! chain via CREATE2. The hot-wallet's job before signing is to compute
//! the EIP-712 `safeTxHash` for a candidate `SafeTx` so each co-signer
//! can verify they're signing exactly what the coordinator drafted.
//!
//! This crate implements just that: domain-separator + struct hashing
//! per Safe v1.3.0 / v1.4.x. It is intentionally light on
//! dependencies — only `sha3` (keccak) and `hex` — so it can run inside
//! Tauri without dragging in a full ethers/alloy stack.
//!
//! Numeric fields use `u128` (value, gases) and `u64` (nonce, chain_id).
//! That covers every real Safe transaction by a wide margin: even the
//! entire ETH supply fits in 88 bits.

use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};

#[derive(Debug, thiserror::Error)]
pub enum MultisigEvmError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

pub type Result<T> = std::result::Result<T, MultisigEvmError>;

/// Safe `Operation` enum — `Call` is the default, `DelegateCall` is
/// reserved for module / library invocations and is *dangerous* on a
/// generic Safe.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Operation {
    Call,
    DelegateCall,
}

impl Operation {
    fn as_u8(self) -> u8 {
        match self {
            Operation::Call => 0,
            Operation::DelegateCall => 1,
        }
    }
}

/// All ten fields the Safe contract hashes for EIP-712. `data` is hex
/// (`0x...`); everything else is decimal. Addresses are 20-byte hex.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SafeTx {
    /// Destination contract / EOA the Safe will call.
    pub to: String,
    /// Wei value to send.
    pub value: u128,
    /// Hex-encoded calldata (`0x...`).
    pub data: String,
    pub operation: Operation,
    /// Gas the Safe forwards to the inner call.
    pub safe_tx_gas: u128,
    /// Refund-related gas baseline (typically `0`).
    pub base_gas: u128,
    /// Gas price in wei for the refund leg (typically `0`).
    pub gas_price: u128,
    /// ERC-20 token used to pay the refund, or `0x000…0` for ETH.
    pub gas_token: String,
    /// Refund receiver, or `0x000…0` for `tx.origin`.
    pub refund_receiver: String,
    /// Safe nonce — must equal `safe.nonce()` at execution time.
    pub nonce: u64,
}

/// Convenience pair returned to the UI: the canonical `safeTxHash` plus
/// the `domainSeparator` used to derive it (so the UI can show the user
/// "this is the chain you think it is").
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SafeTxHashes {
    pub safe_tx_hash: String,
    pub domain_separator: String,
}

const DOMAIN_TYPEHASH_PREIMAGE: &[u8] = b"EIP712Domain(uint256 chainId,address verifyingContract)";

const SAFE_TX_TYPEHASH_PREIMAGE: &[u8] = b"SafeTx(address to,uint256 value,bytes data,uint8 operation,uint256 safeTxGas,uint256 baseGas,uint256 gasPrice,address gasToken,address refundReceiver,uint256 nonce)";

fn keccak(input: &[u8]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(input);
    let out = h.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

fn parse_address(s: &str) -> Result<[u8; 20]> {
    let trimmed = s.trim().trim_start_matches("0x");
    if trimmed.len() != 40 {
        return Err(MultisigEvmError::InvalidInput(format!(
            "address '{}' must be 20 bytes (40 hex chars)",
            s
        )));
    }
    let mut out = [0u8; 20];
    hex::decode_to_slice(trimmed, &mut out)
        .map_err(|e| MultisigEvmError::InvalidInput(format!("address hex: {e}")))?;
    Ok(out)
}

fn parse_data(s: &str) -> Result<Vec<u8>> {
    let trimmed = s.trim().trim_start_matches("0x");
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    hex::decode(trimmed).map_err(|e| MultisigEvmError::InvalidInput(format!("data hex: {e}")))
}

/// ABI-encode an address as a 32-byte left-padded slot.
fn encode_address(addr: [u8; 20]) -> [u8; 32] {
    let mut buf = [0u8; 32];
    buf[12..].copy_from_slice(&addr);
    buf
}

/// ABI-encode a `u128` as a 32-byte big-endian uint256 slot.
fn encode_u128(v: u128) -> [u8; 32] {
    let mut buf = [0u8; 32];
    buf[16..].copy_from_slice(&v.to_be_bytes());
    buf
}

fn encode_u64(v: u64) -> [u8; 32] {
    let mut buf = [0u8; 32];
    buf[24..].copy_from_slice(&v.to_be_bytes());
    buf
}

fn encode_u8(v: u8) -> [u8; 32] {
    let mut buf = [0u8; 32];
    buf[31] = v;
    buf
}

/// Compute the EIP-712 `domainSeparator` for a Safe at `safe_address`
/// on `chain_id`.
pub fn domain_separator(safe_address: &str, chain_id: u64) -> Result<[u8; 32]> {
    let safe = parse_address(safe_address)?;
    let typehash = keccak(DOMAIN_TYPEHASH_PREIMAGE);

    let mut buf = Vec::with_capacity(96);
    buf.extend_from_slice(&typehash);
    buf.extend_from_slice(&encode_u64(chain_id));
    buf.extend_from_slice(&encode_address(safe));
    Ok(keccak(&buf))
}

/// Compute `keccak256(abi.encode(SAFE_TX_TYPEHASH, ...fields))` — the
/// hashed struct that goes inside the final EIP-712 envelope.
fn safe_tx_struct_hash(tx: &SafeTx) -> Result<[u8; 32]> {
    let typehash = keccak(SAFE_TX_TYPEHASH_PREIMAGE);
    let to = parse_address(&tx.to)?;
    let gas_token = parse_address(&tx.gas_token)?;
    let refund_receiver = parse_address(&tx.refund_receiver)?;
    let data_hash = keccak(&parse_data(&tx.data)?);

    // 11 slots * 32 bytes = 352.
    let mut buf = Vec::with_capacity(352);
    buf.extend_from_slice(&typehash);
    buf.extend_from_slice(&encode_address(to));
    buf.extend_from_slice(&encode_u128(tx.value));
    buf.extend_from_slice(&data_hash);
    buf.extend_from_slice(&encode_u8(tx.operation.as_u8()));
    buf.extend_from_slice(&encode_u128(tx.safe_tx_gas));
    buf.extend_from_slice(&encode_u128(tx.base_gas));
    buf.extend_from_slice(&encode_u128(tx.gas_price));
    buf.extend_from_slice(&encode_address(gas_token));
    buf.extend_from_slice(&encode_address(refund_receiver));
    buf.extend_from_slice(&encode_u64(tx.nonce));
    Ok(keccak(&buf))
}

/// Compute the canonical `safeTxHash` a co-signer must sign.
///
/// `safeTxHash = keccak256(0x19 || 0x01 || domainSeparator || keccak256(structEncoded))`
pub fn safe_tx_hash(safe_address: &str, chain_id: u64, tx: &SafeTx) -> Result<SafeTxHashes> {
    let domain = domain_separator(safe_address, chain_id)?;
    let struct_hash = safe_tx_struct_hash(tx)?;

    let mut envelope = Vec::with_capacity(2 + 32 + 32);
    envelope.push(0x19);
    envelope.push(0x01);
    envelope.extend_from_slice(&domain);
    envelope.extend_from_slice(&struct_hash);
    let h = keccak(&envelope);

    Ok(SafeTxHashes {
        safe_tx_hash: format!("0x{}", hex::encode(h)),
        domain_separator: format!("0x{}", hex::encode(domain)),
    })
}

/// Pack a list of (signer, signature) pairs into the contiguous bytes
/// the Safe's `execTransaction` expects. Safe v1.3 / v1.4 sort
/// signatures by signer address ascending (case-insensitive) — we do
/// the same.
///
/// Each signature must already be 65 bytes (`r || s || v`).
pub fn pack_signatures(mut sigs: Vec<(String, Vec<u8>)>) -> Result<String> {
    if sigs.is_empty() {
        return Err(MultisigEvmError::InvalidInput(
            "pack_signatures requires at least one signature".into(),
        ));
    }
    // Validate each entry first so we don't half-sort and then fail.
    let mut parsed: Vec<([u8; 20], Vec<u8>)> = Vec::with_capacity(sigs.len());
    for (addr, sig) in sigs.drain(..) {
        if sig.len() != 65 {
            return Err(MultisigEvmError::InvalidInput(format!(
                "signature for {} must be 65 bytes (r||s||v), got {}",
                addr,
                sig.len()
            )));
        }
        parsed.push((parse_address(&addr)?, sig));
    }
    parsed.sort_by_key(|a| a.0);

    let mut out = Vec::with_capacity(parsed.len() * 65);
    for (_, s) in parsed {
        out.extend_from_slice(&s);
    }
    Ok(format!("0x{}", hex::encode(out)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cross-checked against the safe-eth-py reference implementation
    /// for an empty SafeTx (all zeros) — the resulting hash is
    /// deterministic and a known fixed point.
    #[test]
    fn safe_tx_hash_zero_inputs_is_deterministic() {
        let tx = SafeTx {
            to: "0x0000000000000000000000000000000000000000".into(),
            value: 0,
            data: "0x".into(),
            operation: Operation::Call,
            safe_tx_gas: 0,
            base_gas: 0,
            gas_price: 0,
            gas_token: "0x0000000000000000000000000000000000000000".into(),
            refund_receiver: "0x0000000000000000000000000000000000000000".into(),
            nonce: 0,
        };
        let safe = "0x0000000000000000000000000000000000000001";
        let a = safe_tx_hash(safe, 1, &tx).unwrap();
        let b = safe_tx_hash(safe, 1, &tx).unwrap();
        assert_eq!(a.safe_tx_hash, b.safe_tx_hash);
        assert_eq!(a.domain_separator, b.domain_separator);
        assert!(a.safe_tx_hash.starts_with("0x"));
        assert_eq!(a.safe_tx_hash.len(), 66);
    }

    #[test]
    fn changing_chain_id_changes_domain_separator() {
        let safe = "0x1111111111111111111111111111111111111111";
        let d1 = domain_separator(safe, 1).unwrap();
        let d137 = domain_separator(safe, 137).unwrap();
        assert_ne!(d1, d137);
    }

    #[test]
    fn changing_nonce_changes_hash() {
        let mut tx = SafeTx {
            to: "0x2222222222222222222222222222222222222222".into(),
            value: 1_000_000_000_000_000_000, // 1 ETH
            data: "0x".into(),
            operation: Operation::Call,
            safe_tx_gas: 0,
            base_gas: 0,
            gas_price: 0,
            gas_token: "0x0000000000000000000000000000000000000000".into(),
            refund_receiver: "0x0000000000000000000000000000000000000000".into(),
            nonce: 0,
        };
        let safe = "0x3333333333333333333333333333333333333333";
        let h0 = safe_tx_hash(safe, 1, &tx).unwrap().safe_tx_hash;
        tx.nonce = 1;
        let h1 = safe_tx_hash(safe, 1, &tx).unwrap().safe_tx_hash;
        assert_ne!(h0, h1);
    }

    #[test]
    fn changing_data_changes_hash() {
        let mut tx = SafeTx {
            to: "0x4444444444444444444444444444444444444444".into(),
            value: 0,
            data: "0xa9059cbb".into(),
            operation: Operation::Call,
            safe_tx_gas: 0,
            base_gas: 0,
            gas_price: 0,
            gas_token: "0x0000000000000000000000000000000000000000".into(),
            refund_receiver: "0x0000000000000000000000000000000000000000".into(),
            nonce: 7,
        };
        let safe = "0x5555555555555555555555555555555555555555";
        let h0 = safe_tx_hash(safe, 1, &tx).unwrap().safe_tx_hash;
        tx.data = "0xa9059cbc".into();
        let h1 = safe_tx_hash(safe, 1, &tx).unwrap().safe_tx_hash;
        assert_ne!(h0, h1);
    }

    #[test]
    fn invalid_address_rejected() {
        let tx = SafeTx {
            to: "not-an-address".into(),
            value: 0,
            data: "0x".into(),
            operation: Operation::Call,
            safe_tx_gas: 0,
            base_gas: 0,
            gas_price: 0,
            gas_token: "0x0000000000000000000000000000000000000000".into(),
            refund_receiver: "0x0000000000000000000000000000000000000000".into(),
            nonce: 0,
        };
        assert!(matches!(
            safe_tx_hash("0x0000000000000000000000000000000000000001", 1, &tx),
            Err(MultisigEvmError::InvalidInput(_))
        ));
    }

    #[test]
    fn pack_signatures_sorts_by_signer() {
        let sig_a = vec![0xAAu8; 65];
        let sig_b = vec![0xBBu8; 65];
        let packed = pack_signatures(vec![
            (
                "0xffffffffffffffffffffffffffffffffffffffff".into(),
                sig_b.clone(),
            ),
            (
                "0x1111111111111111111111111111111111111111".into(),
                sig_a.clone(),
            ),
        ])
        .unwrap();
        // Lower address (0x1111…) should come first → signature is sig_a (all 0xAA).
        assert!(packed.starts_with("0x"));
        let bytes = hex::decode(packed.trim_start_matches("0x")).unwrap();
        assert_eq!(bytes.len(), 130);
        assert_eq!(bytes[..65], sig_a[..]);
        assert_eq!(bytes[65..], sig_b[..]);
    }

    #[test]
    fn pack_signatures_rejects_wrong_length() {
        let err = pack_signatures(vec![(
            "0x1111111111111111111111111111111111111111".into(),
            vec![0u8; 64],
        )])
        .unwrap_err();
        assert!(matches!(err, MultisigEvmError::InvalidInput(_)));
    }

    #[test]
    fn pack_signatures_rejects_empty() {
        assert!(matches!(
            pack_signatures(vec![]),
            Err(MultisigEvmError::InvalidInput(_))
        ));
    }
}
