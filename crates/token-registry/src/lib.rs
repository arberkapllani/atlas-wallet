//! Static metadata for ERC-20 / TRC-20 tokens that ship enabled by default.
//!
//! Stored as `&'static [TokenMeta]` so callers can lookup without locks
//! or allocation. User-added tokens live in the profile registry and merge
//! over this list at runtime (TBD).

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

use serde::{Deserialize, Serialize};

/// Token contract standard.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TokenStandard {
    /// Ethereum / EVM-compatible ERC-20 contract.
    Erc20,
    /// Tron TRC-20 contract.
    Trc20,
}

/// Metadata describing a single fungible token.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenMeta {
    /// Stable identifier, lowercase (`usdt-erc20`, `usdt-trc20`).
    pub id: &'static str,
    /// On-chain symbol.
    pub symbol: &'static str,
    /// Display name.
    pub display_name: &'static str,
    /// `chain_id` from the chain registry (`eth`, `trx`, `polygon`, …).
    pub chain_id: &'static str,
    /// Hex (EIP-55) or base58 contract address.
    pub contract: &'static str,
    /// On-chain decimals.
    pub decimals: u8,
    /// Token standard.
    pub standard: TokenStandard,
    /// Whether this token is enabled in the UI by default.
    pub enabled_by_default: bool,
}

/// Default tokens shipped with Atlas.
pub const TOKENS: &[TokenMeta] = &[
    TokenMeta {
        id: "usdt-erc20",
        symbol: "USDT",
        display_name: "Tether (Ethereum)",
        chain_id: "eth",
        contract: "0xdAC17F958D2ee523a2206206994597C13D831ec7",
        decimals: 6,
        standard: TokenStandard::Erc20,
        enabled_by_default: true,
    },
    TokenMeta {
        id: "usdt-trc20",
        symbol: "USDT",
        display_name: "Tether (Tron)",
        chain_id: "trx",
        contract: "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t",
        decimals: 6,
        standard: TokenStandard::Trc20,
        enabled_by_default: true,
    },
    TokenMeta {
        id: "usdc-erc20",
        symbol: "USDC",
        display_name: "USD Coin (Ethereum)",
        chain_id: "eth",
        contract: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
        decimals: 6,
        standard: TokenStandard::Erc20,
        enabled_by_default: false,
    },
];

/// Lookup a token by its stable [`TokenMeta::id`].
pub fn by_id(id: &str) -> Option<&'static TokenMeta> {
    TOKENS.iter().find(|t| t.id == id)
}

/// All tokens for the given chain id.
pub fn for_chain(chain_id: &str) -> impl Iterator<Item = &'static TokenMeta> + '_ {
    TOKENS.iter().filter(move |t| t.chain_id == chain_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usdt_erc20_present() {
        let t = by_id("usdt-erc20").expect("USDT ERC-20 must ship by default");
        assert_eq!(t.symbol, "USDT");
        assert_eq!(t.decimals, 6);
        assert_eq!(t.standard, TokenStandard::Erc20);
        assert!(t.enabled_by_default);
    }

    #[test]
    fn usdt_trc20_present() {
        let t = by_id("usdt-trc20").expect("USDT TRC-20 must ship by default");
        assert_eq!(t.chain_id, "trx");
        assert_eq!(t.standard, TokenStandard::Trc20);
    }

    #[test]
    fn for_chain_filters() {
        let eth_tokens: Vec<_> = for_chain("eth").collect();
        assert!(eth_tokens.iter().all(|t| t.chain_id == "eth"));
        assert!(eth_tokens.iter().any(|t| t.id == "usdt-erc20"));
    }

    #[test]
    fn unique_ids() {
        let mut ids: Vec<_> = TOKENS.iter().map(|t| t.id).collect();
        ids.sort();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "token ids must be unique");
    }
}
