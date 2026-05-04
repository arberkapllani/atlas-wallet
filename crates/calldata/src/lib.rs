//! Decode common EVM calldata into a human-readable summary.
//!
//! The Send-confirm UI shows the user what they're signing. Raw
//! `0x...` calldata is unreadable, so this crate decodes the
//! function selector (first 4 bytes) and any standard arguments
//! into a structured `DecodedCall`. Supported selectors cover the
//! daily-driver flows:
//!
//! * ERC-20 `transfer(address,uint256)`, `approve(address,uint256)`,
//!   `transferFrom(address,address,uint256)`,
//!   `increaseAllowance(address,uint256)`,
//!   `decreaseAllowance(address,uint256)`.
//! * ERC-721/1155 `safeTransferFrom(address,address,uint256)`,
//!   `setApprovalForAll(address,bool)`.
//! * Wrapped-native `deposit()` / `withdraw(uint256)`.
//! * Multicall sentinel (decoded as "multicall: N inner calls").
//!
//! Anything else is returned as `DecodedCall::Unknown` with the raw
//! selector — the UI surfaces it as "unrecognized call to <addr>"
//! and the user can decide whether to proceed.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

/// Failure modes for the decoder.
#[derive(Debug, Error, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "message")]
pub enum DecodeError {
    /// Calldata was not valid hex.
    #[error("invalid hex: {0}")]
    InvalidHex(String),
    /// Calldata was shorter than 4 bytes (no selector).
    #[error("calldata too short")]
    TooShort,
    /// Selector matched a known shape but the argument bytes were
    /// shorter than the ABI required.
    #[error("truncated args: {0}")]
    TruncatedArgs(String),
}

/// Result of decoding one tx.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "tag")]
pub enum DecodedCall {
    /// Empty calldata (plain native-asset transfer).
    NativeTransfer,
    /// `transfer(address recipient, uint256 amount)`.
    Erc20Transfer { recipient: String, amount: String },
    /// `transferFrom(address from, address to, uint256 amount)`.
    Erc20TransferFrom {
        from: String,
        to: String,
        amount: String,
    },
    /// `approve(address spender, uint256 amount)`.
    Erc20Approve {
        spender: String,
        amount: String,
        unlimited: bool,
    },
    /// `increaseAllowance(address spender, uint256 added)`.
    Erc20IncreaseAllowance { spender: String, added: String },
    /// `decreaseAllowance(address spender, uint256 subtracted)`.
    Erc20DecreaseAllowance { spender: String, subtracted: String },
    /// `safeTransferFrom(address from, address to, uint256 idOrAmt)`.
    NftSafeTransferFrom {
        from: String,
        to: String,
        token_id: String,
    },
    /// `setApprovalForAll(address operator, bool approved)`.
    NftSetApprovalForAll { operator: String, approved: bool },
    /// `deposit()` on a wrapped-native (e.g. WETH9).
    WrapDeposit,
    /// `withdraw(uint256)` on a wrapped-native.
    WrapWithdraw { amount: String },
    /// `multicall(bytes[])` (Uniswap V3 / Aggregator style).
    Multicall { inner_count: u32 },
    /// Selector did not match any known shape.
    Unknown { selector: String },
}

impl DecodedCall {
    /// Short, user-friendly verb (for UI badge color decisions).
    pub fn action_kind(&self) -> &'static str {
        match self {
            DecodedCall::NativeTransfer | DecodedCall::Erc20Transfer { .. } => "transfer",
            DecodedCall::Erc20TransferFrom { .. } => "transferFrom",
            DecodedCall::Erc20Approve { .. }
            | DecodedCall::Erc20IncreaseAllowance { .. }
            | DecodedCall::Erc20DecreaseAllowance { .. } => "approve",
            DecodedCall::NftSafeTransferFrom { .. } => "nft-transfer",
            DecodedCall::NftSetApprovalForAll { .. } => "nft-approve-all",
            DecodedCall::WrapDeposit => "wrap",
            DecodedCall::WrapWithdraw { .. } => "unwrap",
            DecodedCall::Multicall { .. } => "multicall",
            DecodedCall::Unknown { .. } => "unknown",
        }
    }

    /// True for high-impact operations the UI should highlight.
    pub fn is_sensitive(&self) -> bool {
        matches!(
            self,
            DecodedCall::Erc20Approve { .. }
                | DecodedCall::Erc20IncreaseAllowance { .. }
                | DecodedCall::NftSetApprovalForAll { approved: true, .. }
        )
    }
}

/// Decode calldata. `data` may include or omit the `0x` prefix and
/// may be empty (decoded as `NativeTransfer`).
pub fn decode(data: &str) -> Result<DecodedCall, DecodeError> {
    let trimmed = data.trim();
    let stripped = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    if stripped.is_empty() {
        return Ok(DecodedCall::NativeTransfer);
    }
    let bytes = hex::decode(stripped).map_err(|e| DecodeError::InvalidHex(e.to_string()))?;
    if bytes.len() < 4 {
        return Err(DecodeError::TooShort);
    }
    let selector = format!("0x{}", hex::encode(&bytes[0..4]));
    let args = &bytes[4..];

    Ok(match selector.as_str() {
        // transfer(address,uint256)
        "0xa9059cbb" => {
            let (recipient, amount) = read_addr_uint(args, "transfer")?;
            DecodedCall::Erc20Transfer { recipient, amount }
        }
        // transferFrom(address,address,uint256)
        "0x23b872dd" => {
            need(args, 96, "transferFrom")?;
            let from = decode_address(&args[0..32]);
            let to = decode_address(&args[32..64]);
            let amount = decode_uint256(&args[64..96]);
            DecodedCall::Erc20TransferFrom { from, to, amount }
        }
        // approve(address,uint256)
        "0x095ea7b3" => {
            let (spender, amount) = read_addr_uint(args, "approve")?;
            let unlimited = is_unlimited_uint(&amount);
            DecodedCall::Erc20Approve {
                spender,
                amount,
                unlimited,
            }
        }
        // increaseAllowance(address,uint256)
        "0x39509351" => {
            let (spender, added) = read_addr_uint(args, "increaseAllowance")?;
            DecodedCall::Erc20IncreaseAllowance { spender, added }
        }
        // decreaseAllowance(address,uint256)
        "0xa457c2d7" => {
            let (spender, subtracted) = read_addr_uint(args, "decreaseAllowance")?;
            DecodedCall::Erc20DecreaseAllowance {
                spender,
                subtracted,
            }
        }
        // safeTransferFrom(address,address,uint256) — ERC-721 selector.
        "0x42842e0e" => {
            need(args, 96, "safeTransferFrom")?;
            let from = decode_address(&args[0..32]);
            let to = decode_address(&args[32..64]);
            let token_id = decode_uint256(&args[64..96]);
            DecodedCall::NftSafeTransferFrom { from, to, token_id }
        }
        // setApprovalForAll(address,bool)
        "0xa22cb465" => {
            need(args, 64, "setApprovalForAll")?;
            let operator = decode_address(&args[0..32]);
            let approved = args[63] != 0;
            DecodedCall::NftSetApprovalForAll { operator, approved }
        }
        // deposit()
        "0xd0e30db0" => DecodedCall::WrapDeposit,
        // withdraw(uint256)
        "0x2e1a7d4d" => {
            need(args, 32, "withdraw")?;
            DecodedCall::WrapWithdraw {
                amount: decode_uint256(&args[0..32]),
            }
        }
        // multicall(bytes[]) — Uniswap V3 router (0xac9650d8) and
        // multicall(uint256,bytes[]) — Uniswap V3 router-with-deadline (0x5ae401dc).
        "0xac9650d8" | "0x5ae401dc" => {
            let inner_count = decode_multicall_count(args).unwrap_or(0);
            DecodedCall::Multicall { inner_count }
        }
        _ => DecodedCall::Unknown { selector },
    })
}

// --- internals -------------------------------------------------------

fn need(args: &[u8], min: usize, fname: &str) -> Result<(), DecodeError> {
    if args.len() < min {
        Err(DecodeError::TruncatedArgs(fname.into()))
    } else {
        Ok(())
    }
}

fn read_addr_uint(args: &[u8], fname: &str) -> Result<(String, String), DecodeError> {
    need(args, 64, fname)?;
    Ok((decode_address(&args[0..32]), decode_uint256(&args[32..64])))
}

fn decode_address(word: &[u8]) -> String {
    // Address sits in the last 20 bytes of a 32-byte word.
    let addr_bytes = &word[12..32];
    format!("0x{}", hex::encode(addr_bytes))
}

fn decode_uint256(word: &[u8]) -> String {
    // Render as decimal big-int by repeated /10.
    let mut digits = Vec::new();
    let mut limbs: Vec<u32> = word
        .chunks(4)
        .map(|c| {
            let mut buf = [0u8; 4];
            buf[..c.len()].copy_from_slice(c);
            u32::from_be_bytes(buf)
        })
        .collect();
    while limbs.iter().any(|&x| x != 0) {
        let mut carry: u64 = 0;
        for limb in limbs.iter_mut() {
            let cur = (carry << 32) | (*limb as u64);
            *limb = (cur / 10) as u32;
            carry = cur % 10;
        }
        digits.push((b'0' + carry as u8) as char);
    }
    if digits.is_empty() {
        "0".into()
    } else {
        digits.iter().rev().collect()
    }
}

fn is_unlimited_uint(amount: &str) -> bool {
    // Any value within 1 bps of 2^256-1 is treated as unlimited.
    const MAX: &str =
        "115792089237316195423570985008687907853269984665640564039457584007913129639935";
    if amount.len() > MAX.len() {
        return true;
    }
    if amount.len() < MAX.len() - 1 {
        return false;
    }
    // Equal length → numeric compare. Length-1 → still finite.
    if amount.len() == MAX.len() {
        amount >= &MAX[..MAX.len() - 2]
    } else {
        false
    }
}

fn decode_multicall_count(args: &[u8]) -> Option<u32> {
    // bytes[] is encoded as: <head offset 32B> <length 32B> <data...>.
    // For multicall(bytes[]) the offset is at args[0..32] (= 0x20),
    // and the length sits at args[offset..offset+32].
    if args.len() < 64 {
        return None;
    }
    let offset = u32::from_be_bytes(args[28..32].try_into().ok()?) as usize;
    if args.len() < offset + 32 {
        return None;
    }
    let len = u32::from_be_bytes(args[offset + 28..offset + 32].try_into().ok()?);
    Some(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad_addr(short: &str) -> String {
        format!("{:0>64}", short.trim_start_matches("0x"))
    }

    fn u256_hex(value: u128) -> String {
        let bytes = value.to_be_bytes();
        let mut padded = vec![0u8; 16];
        padded.extend_from_slice(&bytes);
        hex::encode(padded)
    }

    #[test]
    fn empty_is_native_transfer() {
        assert!(matches!(decode("").unwrap(), DecodedCall::NativeTransfer));
        assert!(matches!(decode("0x").unwrap(), DecodedCall::NativeTransfer));
    }

    #[test]
    fn invalid_hex_errors() {
        let err = decode("0xzz").unwrap_err();
        assert!(matches!(err, DecodeError::InvalidHex(_)));
    }

    #[test]
    fn too_short_errors() {
        let err = decode("0xaabb").unwrap_err();
        assert!(matches!(err, DecodeError::TooShort));
    }

    #[test]
    fn decodes_erc20_transfer() {
        let calldata = format!(
            "0xa9059cbb{}{}",
            pad_addr("0x000000000000000000000000000000000000beef"),
            u256_hex(1_000_000)
        );
        match decode(&calldata).unwrap() {
            DecodedCall::Erc20Transfer { recipient, amount } => {
                assert_eq!(recipient, "0x000000000000000000000000000000000000beef");
                assert_eq!(amount, "1000000");
            }
            other => panic!("wrong variant: {:?}", other),
        }
    }

    #[test]
    fn decodes_approve_with_unlimited_flag() {
        let max = "ff".repeat(32);
        let calldata = format!(
            "0x095ea7b3{}{}",
            pad_addr("0x000000000000000000000000000000000000dead"),
            max
        );
        match decode(&calldata).unwrap() {
            DecodedCall::Erc20Approve {
                spender,
                amount: _,
                unlimited,
            } => {
                assert_eq!(spender, "0x000000000000000000000000000000000000dead");
                assert!(unlimited);
            }
            other => panic!("wrong variant: {:?}", other),
        }
    }

    #[test]
    fn decodes_approve_finite_not_flagged_unlimited() {
        let calldata = format!(
            "0x095ea7b3{}{}",
            pad_addr("0x000000000000000000000000000000000000dead"),
            u256_hex(1_000)
        );
        match decode(&calldata).unwrap() {
            DecodedCall::Erc20Approve {
                amount, unlimited, ..
            } => {
                assert_eq!(amount, "1000");
                assert!(!unlimited);
            }
            other => panic!("wrong variant: {:?}", other),
        }
    }

    #[test]
    fn decodes_transfer_from() {
        let calldata = format!(
            "0x23b872dd{}{}{}",
            pad_addr("0x00000000000000000000000000000000000000aa"),
            pad_addr("0x00000000000000000000000000000000000000bb"),
            u256_hex(42)
        );
        match decode(&calldata).unwrap() {
            DecodedCall::Erc20TransferFrom { from, to, amount } => {
                assert_eq!(from, "0x00000000000000000000000000000000000000aa");
                assert_eq!(to, "0x00000000000000000000000000000000000000bb");
                assert_eq!(amount, "42");
            }
            other => panic!("wrong variant: {:?}", other),
        }
    }

    #[test]
    fn decodes_set_approval_for_all_true() {
        let mut bool_word = "0".repeat(63);
        bool_word.push('1');
        let calldata = format!(
            "0xa22cb465{}{}",
            pad_addr("0x00000000000000000000000000000000000000cc"),
            bool_word
        );
        match decode(&calldata).unwrap() {
            DecodedCall::NftSetApprovalForAll { operator, approved } => {
                assert_eq!(operator, "0x00000000000000000000000000000000000000cc");
                assert!(approved);
            }
            other => panic!("wrong variant: {:?}", other),
        }
    }

    #[test]
    fn decodes_set_approval_for_all_false() {
        let bool_word = "0".repeat(64);
        let calldata = format!(
            "0xa22cb465{}{}",
            pad_addr("0x00000000000000000000000000000000000000cc"),
            bool_word
        );
        match decode(&calldata).unwrap() {
            DecodedCall::NftSetApprovalForAll { approved, .. } => {
                assert!(!approved);
            }
            other => panic!("wrong variant: {:?}", other),
        }
    }

    #[test]
    fn decodes_wrap_deposit_and_withdraw() {
        assert!(matches!(
            decode("0xd0e30db0").unwrap(),
            DecodedCall::WrapDeposit
        ));
        let calldata = format!("0x2e1a7d4d{}", u256_hex(500));
        match decode(&calldata).unwrap() {
            DecodedCall::WrapWithdraw { amount } => assert_eq!(amount, "500"),
            other => panic!("wrong variant: {:?}", other),
        }
    }

    #[test]
    fn decodes_safe_transfer_from_nft() {
        let calldata = format!(
            "0x42842e0e{}{}{}",
            pad_addr("0x00000000000000000000000000000000000000aa"),
            pad_addr("0x00000000000000000000000000000000000000bb"),
            u256_hex(7)
        );
        match decode(&calldata).unwrap() {
            DecodedCall::NftSafeTransferFrom { token_id, .. } => assert_eq!(token_id, "7"),
            other => panic!("wrong variant: {:?}", other),
        }
    }

    #[test]
    fn unknown_selector_preserves_hex() {
        let r = decode("0xdeadbeef00").unwrap();
        match r {
            DecodedCall::Unknown { selector } => assert_eq!(selector, "0xdeadbeef"),
            other => panic!("wrong variant: {:?}", other),
        }
    }

    #[test]
    fn decodes_multicall_count() {
        // multicall(bytes[]) with 3 inner calls. Skip the inner data —
        // we only care about the count. offset = 0x20, length = 3.
        let mut cd = String::from("0xac9650d8");
        cd.push_str(&"00".repeat(31));
        cd.push_str("20"); // offset = 0x20
        cd.push_str(&"00".repeat(31));
        cd.push_str("03"); // length = 3
        match decode(&cd).unwrap() {
            DecodedCall::Multicall { inner_count } => assert_eq!(inner_count, 3),
            other => panic!("wrong variant: {:?}", other),
        }
    }

    #[test]
    fn truncated_transfer_args_error() {
        let calldata = "0xa9059cbb00";
        let err = decode(calldata).unwrap_err();
        assert!(matches!(err, DecodeError::TruncatedArgs(_)));
    }

    #[test]
    fn action_kind_and_is_sensitive() {
        assert_eq!(decode("0xd0e30db0").unwrap().action_kind(), "wrap");
        let approve = decode(&format!(
            "0x095ea7b3{}{}",
            pad_addr("0x00000000000000000000000000000000000000dd"),
            u256_hex(1)
        ))
        .unwrap();
        assert!(approve.is_sensitive());

        let transfer = decode(&format!(
            "0xa9059cbb{}{}",
            pad_addr("0x00000000000000000000000000000000000000dd"),
            u256_hex(1)
        ))
        .unwrap();
        assert!(!transfer.is_sensitive());
    }
}
