//! Shared value types for the chain abstraction layer.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Result alias used by [`ChainProvider`](crate::ChainProvider) impls.
pub type ChainResult<T> = std::result::Result<T, ChainError>;

/// Failure modes shared across every chain.
#[derive(Debug, Error, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", content = "message")]
pub enum ChainError {
    /// Address validation failed.
    #[error("invalid address: {0}")]
    InvalidAddress(String),
    /// Insufficient balance to cover amount + fees.
    #[error("insufficient funds")]
    InsufficientFunds,
    /// Network / RPC failure.
    #[error("network error: {0}")]
    Network(String),
    /// RPC node returned an error response.
    #[error("rpc error: {0}")]
    Rpc(String),
    /// Transaction signing failed.
    #[error("sign error: {0}")]
    Sign(String),
    /// Transaction encoding/decoding failed.
    #[error("codec error: {0}")]
    Codec(String),
    /// Anything else.
    #[error("other: {0}")]
    Other(String),
}

/// A cryptocurrency asset (native or token).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
pub struct Asset {
    /// Stable identifier — usually the lowercase ticker (`btc`, `eth`, `usdc`).
    pub id: String,
    /// Display ticker, e.g. `BTC`.
    pub symbol: String,
    /// Decimals (8 for BTC, 18 for ETH, 6 for USDC, …).
    pub decimals: u8,
    /// Optional logo URL/hint for the UI.
    pub logo: Option<String>,
}

/// A precise on-chain amount: integer base units + asset metadata.
///
/// `value` is denominated in the asset's smallest unit (sat for BTC,
/// wei for ETH). We use `u128` as a JSON-friendly carrier for any
/// realistic balance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
pub struct Amount {
    /// Integer value in base units.
    pub value: u128,
    /// Asset this amount denominates.
    pub asset: Asset,
}

impl Amount {
    /// Construct a new amount from base units + asset.
    pub fn new(value: u128, asset: Asset) -> Self {
        Self { value, asset }
    }

    /// Format as a human string with full precision (no trailing zeros stripped).
    pub fn format_full(&self) -> String {
        let d = self.asset.decimals as usize;
        if d == 0 {
            return format!("{} {}", self.value, self.asset.symbol);
        }
        let s = self.value.to_string();
        if s.len() <= d {
            let padded = format!("{:0>width$}", s, width = d);
            format!("0.{} {}", padded, self.asset.symbol)
        } else {
            let split = s.len() - d;
            format!("{}.{} {}", &s[..split], &s[split..], self.asset.symbol)
        }
    }
}

/// Three canonical fee tiers presented to the user.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct FeeOption {
    /// `"slow" | "normal" | "fast"`.
    pub level: String,
    /// Estimated fee for a typical transaction.
    pub estimated_fee: Amount,
    /// Estimated time-to-confirm in seconds.
    pub eta_seconds: u32,
    /// Implementation-defined hint payload (e.g. sat/vB or gas price), JSON-encoded as string for FFI simplicity.
    pub raw_hint: String,
}

/// User-supplied request to send funds.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct TxRequest {
    /// Sender address (must be one we can derive the private key for).
    pub from: String,
    /// Recipient address.
    pub to: String,
    /// Amount to send (must use the chain's native asset for Phase 1).
    pub amount: Amount,
    /// Selected fee tier (one of the [`FeeOption::level`] strings).
    pub fee_level: String,
    /// Optional memo / data hex blob.
    pub memo: Option<String>,
}

/// A signed transaction, ready for broadcast.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SignedTx {
    /// Hex-encoded raw transaction bytes.
    pub raw_hex: String,
    /// Optimistic txid (matches what `broadcast` will return).
    pub txid: String,
    /// Final network fee charged.
    pub fee: Amount,
}
