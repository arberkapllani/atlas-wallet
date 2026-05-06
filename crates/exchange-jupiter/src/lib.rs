//! Minimal client for the Jupiter v6 aggregator.
//!
//! Jupiter is the de-facto Solana swap router: a single
//! `/quote` request returns the best route across every Solana
//! AMM, and `/swap` returns a base-64 encoded versioned
//! transaction the wallet signs and broadcasts itself. Atlas
//! never custodies funds — the quote is read-only and the swap
//! transaction is signed locally.
//!
//! Docs: <https://station.jup.ag/docs/apis/swap-api>

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

use serde::{Deserialize, Serialize};

/// Default base URL. Jupiter exposes both a public lite endpoint
/// (rate-limited) and a paid pro endpoint; users with an API key
/// can override via `JupiterClient::with_base_url`.
pub const DEFAULT_BASE_URL: &str = "https://quote-api.jup.ag/v6";

/// Errors emitted by the Jupiter client.
#[derive(Debug, thiserror::Error)]
pub enum JupiterError {
    /// Network / TLS / DNS failure.
    #[error("jupiter network: {0}")]
    Network(String),
    /// Server returned non-2xx with optional body.
    #[error("jupiter http {status}: {body}")]
    Http {
        /// HTTP status code.
        status: u16,
        /// Server body (truncated to 1 KiB).
        body: String,
    },
    /// Response body could not be decoded.
    #[error("jupiter decode: {0}")]
    Decode(String),
    /// Caller passed an obviously-bad parameter (e.g. zero amount).
    #[error("jupiter invalid input: {0}")]
    InvalidInput(String),
}

/// Quote request. Mints are the SPL token addresses (use
/// `So11111111111111111111111111111111111111112` for wrapped SOL).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct QuoteRequest {
    /// Input mint address.
    pub input_mint: String,
    /// Output mint address.
    pub output_mint: String,
    /// Amount in input-mint base units, string-encoded so it
    /// survives JS `Number` precision loss for large lamports.
    pub amount: String,
    /// Slippage in basis points (100 = 1%).
    pub slippage_bps: u32,
    /// Optional Atlas platform fee in basis points. When set,
    /// Jupiter routes a corresponding share of the output to
    /// the `fee_account` provided at swap time. Capped at 100
    /// (1%) by Jupiter; we cap at 50 (0.5%) defensively.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform_fee_bps: Option<u32>,
}

/// Subset of the Jupiter `/quote` response that Atlas consumes.
/// Jupiter's actual schema is wider; we keep only the fields the
/// UI needs, so adding new ones doesn't ripple through the
/// frontend.
#[derive(Debug, Clone, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct JupiterQuote {
    /// Echoed input mint.
    pub input_mint: String,
    /// Echoed output mint.
    pub output_mint: String,
    /// Echoed input amount in base units.
    pub in_amount: String,
    /// Expected output amount in base units.
    pub out_amount: String,
    /// Worst-case output amount after slippage, in base units.
    pub other_amount_threshold: String,
    /// Total price impact across the route, as a string-encoded
    /// decimal in `[0, 1]` (e.g. `"0.0034"` for 0.34 %).
    pub price_impact_pct: String,
    /// Echoed slippage (Jupiter may round down).
    pub slippage_bps: u32,
    /// Raw route plan, kept opaque so we can pass it back to
    /// `/swap` verbatim. Jupiter's plan structure changes
    /// occasionally; treating it as `Value` insulates us. The
    /// frontend never inspects it, so we surface it as a JSON
    /// string in the typescript bindings.
    #[specta(type = String)]
    pub route_plan: serde_json::Value,
}

/// Swap-transaction request body posted to `/swap`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapRequest<'a> {
    /// Quote returned by `/quote`.
    pub quote_response: &'a JupiterQuote,
    /// Public key of the wallet paying for the swap.
    pub user_public_key: String,
    /// `true` to wrap / unwrap SOL automatically.
    pub wrap_and_unwrap_sol: bool,
    /// Optional Atlas referral fee account (an SPL token account
    /// owned by Atlas's referral PDA). Only applied when the
    /// matching quote was fetched with `platform_fee_bps`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_account: Option<String>,
}

/// Subset of the `/swap` response. The `swapTransaction` is a
/// base-64 encoded Solana versioned transaction the caller signs
/// and broadcasts itself.
#[derive(Debug, Clone, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SwapTransaction {
    /// Base-64 encoded versioned transaction.
    pub swap_transaction: String,
    /// Last valid blockhash (for retries / expiry).
    pub last_valid_block_height: u64,
}

/// Stateless HTTP client targeting Jupiter v6.
#[derive(Debug, Clone)]
pub struct JupiterClient {
    base_url: String,
    http: reqwest::Client,
}

impl Default for JupiterClient {
    fn default() -> Self {
        Self::new()
    }
}

impl JupiterClient {
    /// New client targeting the public quote endpoint.
    pub fn new() -> Self {
        Self::with_base_url(DEFAULT_BASE_URL)
    }

    /// New client with a custom base URL (e.g. a private Pro
    /// endpoint).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: atlas_net::http_client_builder()
                .user_agent(concat!("atlas-wallet/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("reqwest client"),
        }
    }

    /// Fetch a quote.
    pub async fn quote(&self, req: &QuoteRequest) -> Result<JupiterQuote, JupiterError> {
        if req.amount == "0" || req.amount.is_empty() {
            return Err(JupiterError::InvalidInput("amount must be > 0".into()));
        }
        if req.input_mint == req.output_mint {
            return Err(JupiterError::InvalidInput(
                "input and output mints must differ".into(),
            ));
        }
        if req.slippage_bps > 5_000 {
            // 50 % slippage cap — anything higher is almost
            // certainly a UI bug and a rug-pull magnet.
            return Err(JupiterError::InvalidInput(
                "slippage_bps must be <= 5000 (50%)".into(),
            ));
        }
        if let Some(bps) = req.platform_fee_bps {
            if bps > 50 {
                return Err(JupiterError::InvalidInput(
                    "platform_fee_bps must be <= 50 (0.5%)".into(),
                ));
            }
        }
        let url = format!("{}/quote", self.base_url);
        let mut q: Vec<(&str, String)> = vec![
            ("inputMint", req.input_mint.clone()),
            ("outputMint", req.output_mint.clone()),
            ("amount", req.amount.clone()),
            ("slippageBps", req.slippage_bps.to_string()),
        ];
        if let Some(bps) = req.platform_fee_bps {
            q.push(("platformFeeBps", bps.to_string()));
        }
        let res = self
            .http
            .get(&url)
            .query(&q)
            .send()
            .await
            .map_err(|e| JupiterError::Network(e.to_string()))?;
        Self::parse_json(res).await
    }

    /// Build a swap transaction for a previously fetched quote.
    pub async fn swap(
        &self,
        quote: &JupiterQuote,
        user_public_key: &str,
        wrap_and_unwrap_sol: bool,
    ) -> Result<SwapTransaction, JupiterError> {
        self.swap_with_fee(quote, user_public_key, wrap_and_unwrap_sol, None)
            .await
    }

    /// Build a swap transaction; supplies an optional fee
    /// account for Atlas's referral share.
    pub async fn swap_with_fee(
        &self,
        quote: &JupiterQuote,
        user_public_key: &str,
        wrap_and_unwrap_sol: bool,
        fee_account: Option<String>,
    ) -> Result<SwapTransaction, JupiterError> {
        let url = format!("{}/swap", self.base_url);
        let body = SwapRequest {
            quote_response: quote,
            user_public_key: user_public_key.to_string(),
            wrap_and_unwrap_sol,
            fee_account,
        };
        let res = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| JupiterError::Network(e.to_string()))?;
        Self::parse_json(res).await
    }

    async fn parse_json<T: for<'de> Deserialize<'de>>(
        res: reqwest::Response,
    ) -> Result<T, JupiterError> {
        let status = res.status();
        let bytes = res
            .bytes()
            .await
            .map_err(|e| JupiterError::Network(e.to_string()))?;
        if !status.is_success() {
            let body = String::from_utf8_lossy(&bytes).chars().take(1024).collect();
            return Err(JupiterError::Http {
                status: status.as_u16(),
                body,
            });
        }
        serde_json::from_slice(&bytes).map_err(|e| JupiterError::Decode(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_zero_amount() {
        let c = JupiterClient::new();
        let req = QuoteRequest {
            input_mint: "So11111111111111111111111111111111111111112".into(),
            output_mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
            amount: "0".into(),
            slippage_bps: 50,
            platform_fee_bps: None,
        };
        let err = c.quote(&req).await.unwrap_err();
        assert!(matches!(err, JupiterError::InvalidInput(_)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_same_mint() {
        let c = JupiterClient::new();
        let mint = "So11111111111111111111111111111111111111112";
        let req = QuoteRequest {
            input_mint: mint.into(),
            output_mint: mint.into(),
            amount: "1000000".into(),
            slippage_bps: 50,
            platform_fee_bps: None,
        };
        let err = c.quote(&req).await.unwrap_err();
        assert!(matches!(err, JupiterError::InvalidInput(_)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_excess_slippage() {
        let c = JupiterClient::new();
        let req = QuoteRequest {
            input_mint: "So11111111111111111111111111111111111111112".into(),
            output_mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
            amount: "1000000".into(),
            slippage_bps: 5_001,
            platform_fee_bps: None,
        };
        let err = c.quote(&req).await.unwrap_err();
        assert!(matches!(err, JupiterError::InvalidInput(_)));
    }

    #[test]
    fn quote_decodes_minimal_json() {
        let raw = r#"{
            "inputMint": "So11111111111111111111111111111111111111112",
            "outputMint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "inAmount": "1000000",
            "outAmount": "950000",
            "otherAmountThreshold": "940000",
            "priceImpactPct": "0.0012",
            "slippageBps": 50,
            "routePlan": []
        }"#;
        let q: JupiterQuote = serde_json::from_str(raw).unwrap();
        assert_eq!(q.in_amount, "1000000");
        assert_eq!(q.slippage_bps, 50);
    }
}
