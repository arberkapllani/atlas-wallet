//! Minimal client for the THORChain cross-chain swap aggregator.
//!
//! THORChain is unique among aggregators: it does *native* swaps
//! between layer-1 chains (BTC <-> ETH, ETH <-> LTC, ...) without
//! wrapped tokens or bridges. The protocol fronts liquidity from
//! its own pools and settles on the destination chain.
//!
//! From a wallet's perspective the flow is:
//!   1. GET /thorchain/quote/swap — returns an `inbound_address`
//!      on the *source* chain plus a `memo` string.
//!   2. The wallet sends the source asset to `inbound_address`
//!      with `memo` attached (OP_RETURN for BTC, calldata for
//!      ETH, etc.). THORChain's vaults observe the deposit and
//!      send the destination asset to the user-supplied
//!      `destination` address.
//!
//! Atlas never custodies funds. We only fetch the quote; the
//! resulting deposit transaction is built and signed by the
//! existing per-chain code paths.
//!
//! Docs: <https://dev.thorchain.org/swap-guide/quickstart-guide.html>

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

use serde::{Deserialize, Serialize};

/// Default THORNode URL operated by Nine Realms (community).
pub const DEFAULT_BASE_URL: &str = "https://thornode.ninerealms.com";

/// Errors emitted by the THORChain client.
#[derive(Debug, thiserror::Error)]
pub enum ThorchainError {
    /// Network / TLS / DNS failure.
    #[error("thorchain network: {0}")]
    Network(String),
    /// Server returned non-2xx with optional body.
    #[error("thorchain http {status}: {body}")]
    Http {
        /// HTTP status code.
        status: u16,
        /// Server body (truncated to 1 KiB).
        body: String,
    },
    /// Response body could not be decoded.
    #[error("thorchain decode: {0}")]
    Decode(String),
    /// Caller-side parameter problem.
    #[error("thorchain invalid input: {0}")]
    InvalidInput(String),
}

/// Quote-request parameters.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct ThorchainQuoteRequest {
    /// Source asset in THORChain notation, e.g. `"BTC.BTC"` or
    /// `"ETH.USDC-0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"`.
    pub from_asset: String,
    /// Destination asset.
    pub to_asset: String,
    /// Amount in 1e8-fixed-point THORChain base units (sats for
    /// BTC, eth*1e8 for ETH, ...). String-encoded so JS doesn't
    /// truncate large values.
    pub amount: String,
    /// Final recipient address on the destination chain.
    pub destination: String,
    /// Optional affiliate THORName.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affiliate: Option<String>,
    /// Optional affiliate basis points (0..=1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affiliate_bps: Option<u32>,
    /// Optional minimum output in 1e8 base units (slippage
    /// protection at the protocol level).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_amount_out: Option<String>,
}

/// Subset of the `/thorchain/quote/swap` response. THORChain's
/// schema includes many advisory fields; we keep what the wallet
/// actually needs to build the deposit transaction.
#[derive(Debug, Clone, Deserialize, Serialize, specta::Type)]
pub struct ThorchainQuote {
    /// Address on the source chain to send funds to.
    pub inbound_address: String,
    /// Memo string to attach to the deposit transaction.
    pub memo: String,
    /// Estimated output in 1e8 base units (decimal string).
    pub expected_amount_out: String,
    /// Total fees in destination asset, 1e8 base units.
    #[serde(default)]
    pub total_swap_seconds: Option<u64>,
    /// Block height at which the inbound address rotates.
    /// THORChain rotates vaults; deposits after this height are
    /// risky.
    #[serde(default)]
    pub expiry: Option<i64>,
    /// Suggested gas / fee on the source chain (advisory,
    /// decimal string in 1e8 base units when present). Surfaced
    /// as a JSON string in TS bindings since specta can't derive
    /// on `serde_json::Value`.
    #[serde(default)]
    #[specta(type = Option<String>)]
    pub fees: Option<serde_json::Value>,
    /// Optional minimum recommended slippage tolerance (bps).
    #[serde(default)]
    pub recommended_min_amount_in: Option<String>,
    /// Warnings the protocol attaches to the quote (e.g.
    /// "outbound delay > 24 h"). Surfaced verbatim to the UI.
    #[serde(default)]
    pub notes: Option<String>,
    /// Echoed router contract address (EVM only).
    #[serde(default)]
    pub router: Option<String>,
}

/// Stateless HTTP client targeting a THORNode.
#[derive(Debug, Clone)]
pub struct ThorchainClient {
    base_url: String,
    http: reqwest::Client,
}

impl Default for ThorchainClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ThorchainClient {
    /// New client targeting the public Nine Realms THORNode.
    pub fn new() -> Self {
        Self::with_base_url(DEFAULT_BASE_URL)
    }

    /// New client with a custom base URL (e.g. a self-hosted
    /// THORNode).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: reqwest::Client::builder()
                .user_agent(concat!("atlas-wallet/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("reqwest client"),
        }
    }

    /// Fetch a swap quote. THORChain validates inputs server-side
    /// too; the checks here just shave the obvious foot-guns
    /// before we waste a round-trip.
    pub async fn quote(
        &self,
        req: &ThorchainQuoteRequest,
    ) -> Result<ThorchainQuote, ThorchainError> {
        if req.amount.is_empty() || req.amount == "0" {
            return Err(ThorchainError::InvalidInput("amount must be > 0".into()));
        }
        if req.from_asset.eq_ignore_ascii_case(&req.to_asset) {
            return Err(ThorchainError::InvalidInput(
                "from_asset and to_asset must differ".into(),
            ));
        }
        if req.destination.is_empty() {
            return Err(ThorchainError::InvalidInput(
                "destination address is required".into(),
            ));
        }
        if let Some(bps) = req.affiliate_bps {
            if bps > 1_000 {
                return Err(ThorchainError::InvalidInput(
                    "affiliate_bps must be <= 1000 (10%)".into(),
                ));
            }
        }

        let url = format!("{}/thorchain/quote/swap", self.base_url);
        let mut q: Vec<(&str, String)> = vec![
            ("from_asset", req.from_asset.clone()),
            ("to_asset", req.to_asset.clone()),
            ("amount", req.amount.clone()),
            ("destination", req.destination.clone()),
        ];
        if let Some(a) = &req.affiliate {
            q.push(("affiliate", a.clone()));
        }
        if let Some(b) = req.affiliate_bps {
            q.push(("affiliate_bps", b.to_string()));
        }
        if let Some(m) = &req.min_amount_out {
            q.push(("min_amount_out", m.clone()));
        }
        let res = self
            .http
            .get(&url)
            .query(&q)
            .send()
            .await
            .map_err(|e| ThorchainError::Network(e.to_string()))?;
        let status = res.status();
        let bytes = res
            .bytes()
            .await
            .map_err(|e| ThorchainError::Network(e.to_string()))?;
        if !status.is_success() {
            let body = String::from_utf8_lossy(&bytes).chars().take(1024).collect();
            return Err(ThorchainError::Http {
                status: status.as_u16(),
                body,
            });
        }
        serde_json::from_slice(&bytes).map_err(|e| ThorchainError::Decode(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> ThorchainQuoteRequest {
        ThorchainQuoteRequest {
            from_asset: "BTC.BTC".into(),
            to_asset: "ETH.ETH".into(),
            amount: "100000".into(),
            destination: "0x1111111111111111111111111111111111111111".into(),
            affiliate: None,
            affiliate_bps: None,
            min_amount_out: None,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_zero_amount() {
        let c = ThorchainClient::new();
        let mut r = req();
        r.amount = "0".into();
        assert!(matches!(
            c.quote(&r).await.unwrap_err(),
            ThorchainError::InvalidInput(_)
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_same_asset() {
        let c = ThorchainClient::new();
        let mut r = req();
        r.to_asset = "btc.btc".into();
        assert!(matches!(
            c.quote(&r).await.unwrap_err(),
            ThorchainError::InvalidInput(_)
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_empty_destination() {
        let c = ThorchainClient::new();
        let mut r = req();
        r.destination = String::new();
        assert!(matches!(
            c.quote(&r).await.unwrap_err(),
            ThorchainError::InvalidInput(_)
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_excess_affiliate_bps() {
        let c = ThorchainClient::new();
        let mut r = req();
        r.affiliate_bps = Some(2_000);
        assert!(matches!(
            c.quote(&r).await.unwrap_err(),
            ThorchainError::InvalidInput(_)
        ));
    }

    #[test]
    fn quote_decodes_minimal_json() {
        // Minimal payload mirroring THORNode's actual response
        // shape; the optional fields default to None.
        let raw = r#"{
            "inbound_address":"bc1qxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "memo":"=:ETH.ETH:0x1111111111111111111111111111111111111111",
            "expected_amount_out":"5000000000"
        }"#;
        let q: ThorchainQuote = serde_json::from_str(raw).unwrap();
        assert_eq!(q.expected_amount_out, "5000000000");
        assert!(q.memo.starts_with("=:ETH.ETH:"));
        assert!(q.expiry.is_none());
    }
}
