//! Minimal client for the ChangeNOW v2 swap aggregator.
//!
//! ChangeNOW is a non-custodial swap provider that exposes a
//! simple two-call flow:
//!
//!   1. `GET /v2/exchange/estimated-amount` — read-only quote.
//!   2. `POST /v2/exchange` — create a transaction; the response
//!      includes `payinAddress` (deposit address on the source
//!      chain). The wallet sends funds there itself; ChangeNOW
//!      pays out to the user-supplied `payoutAddress` on the
//!      destination chain.
//!
//! Atlas never custodies funds; we just call ChangeNOW and let
//! the existing per-chain code build/sign the deposit.
//!
//! The estimate endpoint is open. Creating a transaction and
//! polling its status both require an API key issued by
//! ChangeNOW's partner program (passed via `x-changenow-api-key`
//! header).
//!
//! Docs: <https://documenter.getpostman.com/view/8180765/TVRrYr8C>

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

use serde::{Deserialize, Serialize};

/// Default v2 base URL.
pub const DEFAULT_BASE_URL: &str = "https://api.changenow.io/v2";

/// Errors emitted by the ChangeNOW client.
#[derive(Debug, thiserror::Error)]
pub enum ChangeNowError {
    /// Network / TLS / DNS failure.
    #[error("changenow network: {0}")]
    Network(String),
    /// Server returned non-2xx with optional body.
    #[error("changenow http {status}: {body}")]
    Http {
        /// HTTP status code.
        status: u16,
        /// Server body (truncated to 1 KiB).
        body: String,
    },
    /// Response body could not be decoded.
    #[error("changenow decode: {0}")]
    Decode(String),
    /// Caller-side parameter problem.
    #[error("changenow invalid input: {0}")]
    InvalidInput(String),
    /// Endpoint requires an API key but none was configured.
    #[error("changenow api key required")]
    ApiKeyRequired,
}

/// Read-only estimate request.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EstimateRequest {
    /// Source ticker (lowercase, e.g. `"btc"`, `"eth"`).
    pub from_currency: String,
    /// Destination ticker.
    pub to_currency: String,
    /// Source-chain network (e.g. `"btc"`, `"eth"`, `"bsc"`).
    /// Optional — ChangeNOW infers it for cross-chain unique
    /// tickers, required when ambiguous (e.g. USDT lives on
    /// dozens of networks).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_network: Option<String>,
    /// Destination-chain network. Same rule as `from_network`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_network: Option<String>,
    /// Source amount as a decimal string in the *display* unit
    /// (i.e. "0.05" BTC, not 5000000 sats). ChangeNOW's API is
    /// decimal-string oriented.
    pub from_amount: String,
    /// `"standard"` or `"fixed-rate"`. Standard = floating rate,
    /// Fixed = locked-in rate at quote time (smaller spread).
    pub flow: String,
}

/// Subset of the estimate response.
#[derive(Debug, Clone, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Estimate {
    /// Echoed source ticker.
    pub from_currency: String,
    /// Echoed destination ticker.
    pub to_currency: String,
    /// Estimated destination amount (decimal string, display
    /// unit). May be missing if amount is below the protocol
    /// minimum, in which case `min_amount` will be set.
    #[serde(default)]
    pub to_amount: Option<String>,
    /// Echoed flow (`"standard"` / `"fixed-rate"`).
    pub flow: String,
    /// Indicative network fee (decimal string, destination
    /// asset, display unit).
    #[serde(default)]
    pub network_fee: Option<String>,
    /// Minimum source amount accepted on this pair.
    #[serde(default)]
    pub min_amount: Option<String>,
    /// Maximum source amount accepted on this pair.
    #[serde(default)]
    pub max_amount: Option<String>,
    /// Quote / rate id (only for fixed-rate flow). Required when
    /// later creating a fixed-rate exchange.
    #[serde(default)]
    pub rate_id: Option<String>,
}

/// Exchange-creation request.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateExchangeRequest {
    /// Source ticker.
    pub from_currency: String,
    /// Destination ticker.
    pub to_currency: String,
    /// Optional source network override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_network: Option<String>,
    /// Optional destination network override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_network: Option<String>,
    /// Source amount (decimal string, display unit).
    pub from_amount: String,
    /// User's destination address.
    pub address: String,
    /// Optional memo / destination tag (XRP, XLM, ...).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_id: Option<String>,
    /// Optional refund address (paid back if the exchange fails).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refund_address: Option<String>,
    /// `"standard"` or `"fixed-rate"`.
    pub flow: String,
    /// `rateId` from the matching estimate, required for
    /// fixed-rate flow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_id: Option<String>,
}

/// Created-exchange response: contains the deposit address the
/// wallet must send the source asset to.
#[derive(Debug, Clone, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CreatedExchange {
    /// ChangeNOW transaction id (used to poll status later).
    pub id: String,
    /// Deposit address on the source chain.
    pub payin_address: String,
    /// Optional deposit memo / destination tag.
    #[serde(default)]
    pub payin_extra_id: Option<String>,
    /// Echoed payout address.
    pub payout_address: String,
    /// Echoed source ticker.
    pub from_currency: String,
    /// Echoed destination ticker.
    pub to_currency: String,
    /// Source amount actually expected (decimal string, display).
    #[serde(default)]
    pub from_amount: Option<String>,
    /// Estimated destination amount.
    #[serde(default)]
    pub to_amount: Option<String>,
    /// Echoed flow.
    pub flow: String,
}

/// Stateless HTTP client targeting ChangeNOW v2.
#[derive(Debug, Clone)]
pub struct ChangeNowClient {
    base_url: String,
    api_key: Option<String>,
    http: reqwest::Client,
}

impl ChangeNowClient {
    /// New client with default base URL and no API key. The
    /// estimate endpoint is open; creating a transaction will
    /// fail with `ApiKeyRequired` until a key is set.
    pub fn new() -> Self {
        Self::with(DEFAULT_BASE_URL, None)
    }

    /// New client with an API key.
    pub fn with_api_key(api_key: impl Into<String>) -> Self {
        Self::with(DEFAULT_BASE_URL, Some(api_key.into()))
    }

    /// New client with a custom base URL and optional API key.
    pub fn with(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key,
            http: reqwest::Client::builder()
                .user_agent(concat!("atlas-wallet/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("reqwest client"),
        }
    }

    /// Fetch a read-only estimate. Doesn't require an API key.
    pub async fn estimate(&self, req: &EstimateRequest) -> Result<Estimate, ChangeNowError> {
        if req.from_amount.is_empty() || req.from_amount == "0" {
            return Err(ChangeNowError::InvalidInput(
                "from_amount must be > 0".into(),
            ));
        }
        if req.from_currency.eq_ignore_ascii_case(&req.to_currency)
            && req.from_network == req.to_network
        {
            return Err(ChangeNowError::InvalidInput(
                "from and to currencies must differ".into(),
            ));
        }
        if req.flow != "standard" && req.flow != "fixed-rate" {
            return Err(ChangeNowError::InvalidInput(
                "flow must be 'standard' or 'fixed-rate'".into(),
            ));
        }
        let url = format!("{}/exchange/estimated-amount", self.base_url);
        let mut q: Vec<(&str, String)> = vec![
            ("fromCurrency", req.from_currency.clone()),
            ("toCurrency", req.to_currency.clone()),
            ("fromAmount", req.from_amount.clone()),
            ("flow", req.flow.clone()),
        ];
        if let Some(n) = &req.from_network {
            q.push(("fromNetwork", n.clone()));
        }
        if let Some(n) = &req.to_network {
            q.push(("toNetwork", n.clone()));
        }
        let mut rb = self.http.get(&url).query(&q);
        if let Some(k) = &self.api_key {
            rb = rb.header("x-changenow-api-key", k);
        }
        let res = rb
            .send()
            .await
            .map_err(|e| ChangeNowError::Network(e.to_string()))?;
        Self::parse_json(res).await
    }

    /// Create a transaction. Requires an API key.
    pub async fn create_exchange(
        &self,
        req: &CreateExchangeRequest,
    ) -> Result<CreatedExchange, ChangeNowError> {
        let key = self
            .api_key
            .as_deref()
            .ok_or(ChangeNowError::ApiKeyRequired)?;
        if req.address.is_empty() {
            return Err(ChangeNowError::InvalidInput(
                "destination address is required".into(),
            ));
        }
        if req.from_amount.is_empty() || req.from_amount == "0" {
            return Err(ChangeNowError::InvalidInput(
                "from_amount must be > 0".into(),
            ));
        }
        if req.flow == "fixed-rate" && req.rate_id.is_none() {
            return Err(ChangeNowError::InvalidInput(
                "fixed-rate flow requires rate_id".into(),
            ));
        }
        let url = format!("{}/exchange", self.base_url);
        let res = self
            .http
            .post(&url)
            .header("x-changenow-api-key", key)
            .json(req)
            .send()
            .await
            .map_err(|e| ChangeNowError::Network(e.to_string()))?;
        Self::parse_json(res).await
    }

    async fn parse_json<T: for<'de> Deserialize<'de>>(
        res: reqwest::Response,
    ) -> Result<T, ChangeNowError> {
        let status = res.status();
        let bytes = res
            .bytes()
            .await
            .map_err(|e| ChangeNowError::Network(e.to_string()))?;
        if !status.is_success() {
            let body = String::from_utf8_lossy(&bytes).chars().take(1024).collect();
            return Err(ChangeNowError::Http {
                status: status.as_u16(),
                body,
            });
        }
        serde_json::from_slice(&bytes).map_err(|e| ChangeNowError::Decode(e.to_string()))
    }
}

impl Default for ChangeNowClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn est_req() -> EstimateRequest {
        EstimateRequest {
            from_currency: "btc".into(),
            to_currency: "eth".into(),
            from_network: None,
            to_network: None,
            from_amount: "0.05".into(),
            flow: "standard".into(),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn estimate_rejects_zero_amount() {
        let c = ChangeNowClient::new();
        let mut r = est_req();
        r.from_amount = "0".into();
        assert!(matches!(
            c.estimate(&r).await.unwrap_err(),
            ChangeNowError::InvalidInput(_)
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn estimate_rejects_same_pair() {
        let c = ChangeNowClient::new();
        let mut r = est_req();
        r.to_currency = "BTC".into();
        assert!(matches!(
            c.estimate(&r).await.unwrap_err(),
            ChangeNowError::InvalidInput(_)
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn estimate_rejects_bad_flow() {
        let c = ChangeNowClient::new();
        let mut r = est_req();
        r.flow = "fast".into();
        assert!(matches!(
            c.estimate(&r).await.unwrap_err(),
            ChangeNowError::InvalidInput(_)
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn create_requires_api_key() {
        let c = ChangeNowClient::new();
        let req = CreateExchangeRequest {
            from_currency: "btc".into(),
            to_currency: "eth".into(),
            from_network: None,
            to_network: None,
            from_amount: "0.05".into(),
            address: "0x1111111111111111111111111111111111111111".into(),
            extra_id: None,
            refund_address: None,
            flow: "standard".into(),
            rate_id: None,
        };
        assert!(matches!(
            c.create_exchange(&req).await.unwrap_err(),
            ChangeNowError::ApiKeyRequired
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn create_fixed_rate_requires_rate_id() {
        let c = ChangeNowClient::with_api_key("test-key");
        let req = CreateExchangeRequest {
            from_currency: "btc".into(),
            to_currency: "eth".into(),
            from_network: None,
            to_network: None,
            from_amount: "0.05".into(),
            address: "0x1111111111111111111111111111111111111111".into(),
            extra_id: None,
            refund_address: None,
            flow: "fixed-rate".into(),
            rate_id: None,
        };
        assert!(matches!(
            c.create_exchange(&req).await.unwrap_err(),
            ChangeNowError::InvalidInput(_)
        ));
    }

    #[test]
    fn estimate_decodes_minimal_json() {
        let raw = r#"{
            "fromCurrency":"btc",
            "toCurrency":"eth",
            "toAmount":"0.81",
            "flow":"standard"
        }"#;
        let e: Estimate = serde_json::from_str(raw).unwrap();
        assert_eq!(e.from_currency, "btc");
        assert_eq!(e.to_amount.as_deref(), Some("0.81"));
    }
}
