//! Minimal ERC-20 helpers — `balanceOf` and `transfer` call-data encoders.
//!
//! No on-chain logic lives here; the EVM provider uses these to issue
//! token reads and writes through the same JSON-RPC connection.

use sha3::{Digest, Keccak256};

/// Build the call-data for `balanceOf(address)`.
pub fn balance_of_calldata(holder: &[u8; 20]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 32);
    out.extend_from_slice(&selector("balanceOf(address)"));
    out.extend_from_slice(&[0u8; 12]);
    out.extend_from_slice(holder);
    out
}

/// Build the call-data for `transfer(address,uint256)`.
pub fn transfer_calldata(to: &[u8; 20], value: u128) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 64);
    out.extend_from_slice(&selector("transfer(address,uint256)"));
    out.extend_from_slice(&[0u8; 12]);
    out.extend_from_slice(to);
    let mut value_be = [0u8; 32];
    value_be[16..].copy_from_slice(&value.to_be_bytes());
    out.extend_from_slice(&value_be);
    out
}

/// Build the call-data for `approve(spender, value)`.
pub fn approve_calldata(spender: &[u8; 20], value: u128) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 64);
    out.extend_from_slice(&selector("approve(address,uint256)"));
    out.extend_from_slice(&[0u8; 12]);
    out.extend_from_slice(spender);
    let mut value_be = [0u8; 32];
    value_be[16..].copy_from_slice(&value.to_be_bytes());
    out.extend_from_slice(&value_be);
    out
}

/// Build the call-data for `allowance(owner, spender)`.
pub fn allowance_calldata(owner: &[u8; 20], spender: &[u8; 20]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 64);
    out.extend_from_slice(&selector("allowance(address,address)"));
    out.extend_from_slice(&[0u8; 12]);
    out.extend_from_slice(owner);
    out.extend_from_slice(&[0u8; 12]);
    out.extend_from_slice(spender);
    out
}

fn selector(signature: &str) -> [u8; 4] {
    let h = Keccak256::digest(signature.as_bytes());
    let mut out = [0u8; 4];
    out.copy_from_slice(&h[..4]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balance_of_selector_known() {
        let s = hex::encode(selector("balanceOf(address)"));
        assert_eq!(s, "70a08231");
    }

    #[test]
    fn transfer_selector_known() {
        let s = hex::encode(selector("transfer(address,uint256)"));
        assert_eq!(s, "a9059cbb");
    }
}
