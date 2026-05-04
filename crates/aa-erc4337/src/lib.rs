//! ERC-4337 (Account Abstraction) UserOperation support for Atlas.
//!
//! When a user runs an AA smart-account, they don't sign raw EVM
//! transactions — they sign a `UserOperation` that the EntryPoint
//! contract validates and executes. The hot wallet's job before signing
//! is to:
//!
//!   1. Construct the `UserOperation` (sender, nonce, calldata, gases,
//!      paymaster blob, …).
//!   2. Compute the canonical `userOpHash` for the chain + EntryPoint:
//!
//!      `userOpHash = keccak256(abi.encode(keccak256(packed), entryPoint, chainId))`
//!
//!      where `packed` = `abi.encode` of the UserOp with `initCode`,
//!      `callData`, and `paymasterAndData` replaced by their `keccak256`.
//!
//! 3. Hand the hash to the signer (HW or seed-derived key); the
//!    resulting 65-byte signature is what `signature` ends up being.
//!
//! This crate handles steps 1 and 2. It targets EntryPoint v0.6 — the
//! version every major bundler (Stackup, Pimlico, Alchemy, Biconomy)
//! still accepts in 2026 — and it intentionally avoids ethers/alloy so
//! the Tauri binary stays small.

use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};

#[derive(Debug, thiserror::Error)]
pub enum AaError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

pub type Result<T> = std::result::Result<T, AaError>;

/// EntryPoint v0.6 UserOperation. All numeric fields are stored as
/// `u128` (more than enough — even pre-EIP-1559 max gas prices fit) and
/// `u64` for nonce.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct UserOperation {
    pub sender: String,
    pub nonce: u64,
    /// Hex (`0x...`) — empty (`"0x"`) once the account is already deployed.
    pub init_code: String,
    /// Hex (`0x...`) — the inner call the account executes.
    pub call_data: String,
    pub call_gas_limit: u128,
    pub verification_gas_limit: u128,
    pub pre_verification_gas: u128,
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
    /// Hex (`0x...`) — empty (`"0x"`) when there is no paymaster. When
    /// non-empty, the first 20 bytes are the paymaster contract.
    pub paymaster_and_data: String,
}

/// Result of `user_op_hash` — surfaces the `userOpHash` plus the inner
/// `packedHash` so a tester can check intermediate values.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct UserOpHashes {
    pub user_op_hash: String,
    pub packed_hash: String,
}

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
        return Err(AaError::InvalidInput(format!(
            "address '{}' must be 20 bytes (40 hex chars)",
            s
        )));
    }
    let mut out = [0u8; 20];
    hex::decode_to_slice(trimmed, &mut out)
        .map_err(|e| AaError::InvalidInput(format!("address hex: {e}")))?;
    Ok(out)
}

fn parse_bytes(s: &str) -> Result<Vec<u8>> {
    let trimmed = s.trim().trim_start_matches("0x");
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    hex::decode(trimmed).map_err(|e| AaError::InvalidInput(format!("hex: {e}")))
}

fn enc_address(addr: [u8; 20]) -> [u8; 32] {
    let mut buf = [0u8; 32];
    buf[12..].copy_from_slice(&addr);
    buf
}

fn enc_u128(v: u128) -> [u8; 32] {
    let mut buf = [0u8; 32];
    buf[16..].copy_from_slice(&v.to_be_bytes());
    buf
}

fn enc_u64(v: u64) -> [u8; 32] {
    let mut buf = [0u8; 32];
    buf[24..].copy_from_slice(&v.to_be_bytes());
    buf
}

/// Compute the EntryPoint v0.6 `userOpHash` for `op` on `chain_id`.
/// The 11-slot inner blob mirrors the EntryPoint's
/// `getUserOpHash` exactly:
///
/// `pack = abi.encode(sender, nonce, keccak(initCode), keccak(callData),
///                    callGasLimit, verificationGasLimit, preVerificationGas,
///                    maxFeePerGas, maxPriorityFeePerGas, keccak(paymasterAndData))`
pub fn user_op_hash(entry_point: &str, chain_id: u64, op: &UserOperation) -> Result<UserOpHashes> {
    let sender = parse_address(&op.sender)?;
    let entry = parse_address(entry_point)?;

    let init_code = parse_bytes(&op.init_code)?;
    let call_data = parse_bytes(&op.call_data)?;
    let paymaster = parse_bytes(&op.paymaster_and_data)?;

    let init_hash = keccak(&init_code);
    let call_hash = keccak(&call_data);
    let paymaster_hash = keccak(&paymaster);

    // 10 slots * 32 = 320 bytes for the packed blob.
    let mut packed = Vec::with_capacity(320);
    packed.extend_from_slice(&enc_address(sender));
    packed.extend_from_slice(&enc_u64(op.nonce));
    packed.extend_from_slice(&init_hash);
    packed.extend_from_slice(&call_hash);
    packed.extend_from_slice(&enc_u128(op.call_gas_limit));
    packed.extend_from_slice(&enc_u128(op.verification_gas_limit));
    packed.extend_from_slice(&enc_u128(op.pre_verification_gas));
    packed.extend_from_slice(&enc_u128(op.max_fee_per_gas));
    packed.extend_from_slice(&enc_u128(op.max_priority_fee_per_gas));
    packed.extend_from_slice(&paymaster_hash);

    let packed_hash = keccak(&packed);

    // Outer envelope: keccak(packedHash || entryPoint || chainId).
    let mut envelope = Vec::with_capacity(96);
    envelope.extend_from_slice(&packed_hash);
    envelope.extend_from_slice(&enc_address(entry));
    envelope.extend_from_slice(&enc_u64(chain_id));
    let user_op_hash = keccak(&envelope);

    Ok(UserOpHashes {
        user_op_hash: format!("0x{}", hex::encode(user_op_hash)),
        packed_hash: format!("0x{}", hex::encode(packed_hash)),
    })
}

/// Encode `(target, value, data)` as the calldata for a SimpleAccount's
/// `execute(address,uint256,bytes)` selector — what most ERC-4337
/// accounts use as their inner call dispatcher. Returned as a hex
/// string with `0x` prefix.
pub fn encode_execute_calldata(target: &str, value: u128, data: &str) -> Result<String> {
    let target = parse_address(target)?;
    let inner = parse_bytes(data)?;

    // selector = keccak("execute(address,uint256,bytes)")[0..4]
    let selector = {
        let h = keccak(b"execute(address,uint256,bytes)");
        [h[0], h[1], h[2], h[3]]
    };

    // Layout: selector + 3 head words + 1 length word + padded body.
    // bytes is dynamic; head[2] = offset to data length = 0x60.
    let pad_len = (32 - (inner.len() % 32)) % 32;
    let mut buf = Vec::with_capacity(4 + 32 * 4 + inner.len() + pad_len);
    buf.extend_from_slice(&selector);
    buf.extend_from_slice(&enc_address(target));
    buf.extend_from_slice(&enc_u128(value));
    // offset to bytes payload (relative to start of args, after selector)
    buf.extend_from_slice(&enc_u128(0x60));
    // length
    buf.extend_from_slice(&enc_u128(inner.len() as u128));
    buf.extend_from_slice(&inner);
    buf.extend(std::iter::repeat_n(0u8, pad_len));

    Ok(format!("0x{}", hex::encode(buf)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op_zero() -> UserOperation {
        UserOperation {
            sender: "0x0000000000000000000000000000000000000001".into(),
            nonce: 0,
            init_code: "0x".into(),
            call_data: "0x".into(),
            call_gas_limit: 0,
            verification_gas_limit: 0,
            pre_verification_gas: 0,
            max_fee_per_gas: 0,
            max_priority_fee_per_gas: 0,
            paymaster_and_data: "0x".into(),
        }
    }

    #[test]
    fn hash_is_deterministic() {
        let ep = "0x5FF137D4b0FDCD49DcA30c7CF57E578a026d2789"; // EP v0.6 mainnet
        let a = user_op_hash(ep, 1, &op_zero()).unwrap();
        let b = user_op_hash(ep, 1, &op_zero()).unwrap();
        assert_eq!(a.user_op_hash, b.user_op_hash);
        assert_eq!(a.packed_hash, b.packed_hash);
        assert_eq!(a.user_op_hash.len(), 66);
    }

    #[test]
    fn chain_id_changes_hash() {
        let ep = "0x5FF137D4b0FDCD49DcA30c7CF57E578a026d2789";
        let h1 = user_op_hash(ep, 1, &op_zero()).unwrap().user_op_hash;
        let h137 = user_op_hash(ep, 137, &op_zero()).unwrap().user_op_hash;
        assert_ne!(h1, h137);
    }

    #[test]
    fn entry_point_changes_hash() {
        let mut op = op_zero();
        op.sender = "0x0000000000000000000000000000000000000002".into();
        let h_a = user_op_hash("0x5FF137D4b0FDCD49DcA30c7CF57E578a026d2789", 1, &op)
            .unwrap()
            .user_op_hash;
        let h_b = user_op_hash(
            "0x0000000071727De22E5E9d8BAf0edAc6f37da032", // EP v0.7 address
            1,
            &op,
        )
        .unwrap()
        .user_op_hash;
        assert_ne!(h_a, h_b);
    }

    #[test]
    fn calldata_changes_hash() {
        let ep = "0x5FF137D4b0FDCD49DcA30c7CF57E578a026d2789";
        let mut op = op_zero();
        let h0 = user_op_hash(ep, 1, &op).unwrap().user_op_hash;
        op.call_data = "0xdeadbeef".into();
        let h1 = user_op_hash(ep, 1, &op).unwrap().user_op_hash;
        assert_ne!(h0, h1);
    }

    #[test]
    fn paymaster_changes_hash() {
        let ep = "0x5FF137D4b0FDCD49DcA30c7CF57E578a026d2789";
        let mut op = op_zero();
        let h0 = user_op_hash(ep, 1, &op).unwrap().user_op_hash;
        op.paymaster_and_data = "0xcafebabe".into();
        let h1 = user_op_hash(ep, 1, &op).unwrap().user_op_hash;
        assert_ne!(h0, h1);
    }

    #[test]
    fn invalid_sender_rejected() {
        let mut op = op_zero();
        op.sender = "garbage".into();
        let err = user_op_hash("0x5FF137D4b0FDCD49DcA30c7CF57E578a026d2789", 1, &op).unwrap_err();
        assert!(matches!(err, AaError::InvalidInput(_)));
    }

    #[test]
    fn execute_calldata_starts_with_selector() {
        // selector("execute(address,uint256,bytes)") = 0xb61d27f6
        let cd =
            encode_execute_calldata("0x1111111111111111111111111111111111111111", 0, "0x").unwrap();
        assert!(cd.starts_with("0xb61d27f6"));
        // Empty bytes still pads to a length word: 4 + 3*32 + 32 = 132 bytes
        // = 264 hex chars + "0x".
        assert_eq!(cd.len(), 2 + 264);
    }

    #[test]
    fn execute_calldata_pads_dynamic_bytes() {
        // 1-byte payload should be right-padded to 32 bytes.
        let cd = encode_execute_calldata("0x1111111111111111111111111111111111111111", 1, "0xff")
            .unwrap();
        let raw = hex::decode(cd.trim_start_matches("0x")).unwrap();
        // selector(4) + 3 heads(96) + length(32) + padded body(32) = 164.
        assert_eq!(raw.len(), 164);
        // last 32 bytes: 0xff followed by 31 zero bytes.
        assert_eq!(raw[164 - 32], 0xff);
        assert!(raw[164 - 31..].iter().all(|b| *b == 0));
    }
}
