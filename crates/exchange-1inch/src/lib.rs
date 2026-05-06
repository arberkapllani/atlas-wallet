//! 1inch v6 aggregator client.
//!
//! Atlas treats every endpoint as user-overridable: the caller supplies a
//! base URL (default `https://api.1inch.dev`) and an optional bearer token
//! (1inch's developer portal issues these for free). When no token is
//! configured we still attempt the request — public mirrors exist for
//! self-hosted setups.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

use serde::{Deserialize, Serialize};

/// Default base URL for the official 1inch developer API.
pub const DEFAULT_BASE_URL: &str = "https://api.1inch.dev";

/// 1inch v6 quote response, lifted into a stable shape.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Quote {
    /// Source token address (lowercase 0x…).
    pub from_token: String,
    /// Destination token address.
    pub to_token: String,
    /// Source amount, base units, decimal string.
    pub from_amount: String,
    /// Estimated destination amount, base units, decimal string.
    pub to_amount: String,
    /// Estimated gas units for the swap (advisory).
    pub estimated_gas: u64,
    /// Optional names of protocols routed through.
    pub protocols: Vec<String>,
}

/// 1inch v6 swap response — a fully-formed transaction the caller signs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapTx {
    /// Recipient (router) contract address.
    pub to: String,
    /// Hex-encoded calldata (`0x…`).
    pub data: String,
    /// Wei value to attach (decimal string). Non-zero only for native-in swaps.
    pub value: String,
    /// Suggested gas limit.
    pub gas: u64,
    /// Estimated destination amount.
    pub to_amount: String,
}

/// Failure modes for 1inch calls.
#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message")]
pub enum ExchangeError {
    /// Network or transport failure.
    #[error("network: {0}")]
    Network(String),
    /// Response did not match the expected JSON schema.
    #[error("codec: {0}")]
    Codec(String),
    /// 1inch returned an explicit error (insufficient liquidity, bad params…).
    #[error("aggregator: {0}")]
    Aggregator(String),
    /// Caller-side configuration problem (missing api key, bad chain id, …).
    #[error("config: {0}")]
    Config(String),
}

/// 1inch v6 aggregator client.
#[derive(Clone)]
pub struct OneInchClient {
    base_url: String,
    api_key: Option<String>,
    http: reqwest::Client,
}

impl OneInchClient {
    /// Build a client. `api_key` is optional but strongly recommended on the
    /// official endpoint (rate-limited otherwise).
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        let base = base_url.into();
        let trimmed = if base.is_empty() {
            DEFAULT_BASE_URL.to_string()
        } else {
            base.trim_end_matches('/').to_string()
        };
        Self {
            base_url: trimmed,
            api_key,
            http: atlas_net::http_client_builder()
                .user_agent("Atlas/0.1")
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| atlas_net::http_client()),
        }
    }

    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_key {
            Some(k) if !k.is_empty() => req.bearer_auth(k),
            _ => req,
        }
    }

    /// Fetch a quote for swapping `amount` of `src` into `dst` on the given
    /// EIP-155 chain id. Amounts are in base units (decimal string).
    pub async fn quote(
        &self,
        chain_id: u64,
        src: &str,
        dst: &str,
        amount: &str,
    ) -> Result<Quote, ExchangeError> {
        let url = format!("{}/swap/v6.0/{}/quote", self.base_url, chain_id);
        let req = self.http.get(&url).query(&[
            ("src", src),
            ("dst", dst),
            ("amount", amount),
            ("includeProtocols", "true"),
            ("includeGas", "true"),
        ]);
        let resp = self
            .auth(req)
            .send()
            .await
            .map_err(|e| ExchangeError::Network(e.to_string()))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| ExchangeError::Network(e.to_string()))?;
        if !status.is_success() {
            return Err(ExchangeError::Aggregator(extract_error(
                &body,
                status.as_u16(),
            )));
        }
        let v: serde_json::Value =
            serde_json::from_str(&body).map_err(|e| ExchangeError::Codec(e.to_string()))?;

        let to_amount = v
            .get("dstAmount")
            .and_then(|x| x.as_str())
            .ok_or_else(|| ExchangeError::Codec("missing dstAmount".into()))?
            .to_string();
        let estimated_gas = v.get("gas").and_then(|x| x.as_u64()).unwrap_or(0);
        let protocols = v
            .get("protocols")
            .and_then(|x| x.as_array())
            .map(|arr| collect_protocol_names(arr))
            .unwrap_or_default();

        Ok(Quote {
            from_token: src.to_lowercase(),
            to_token: dst.to_lowercase(),
            from_amount: amount.to_string(),
            to_amount,
            estimated_gas,
            protocols,
        })
    }

    /// Build a swap transaction. `from_address` is the EOA that will sign,
    /// `slippage_bps` is the maximum acceptable slippage in basis points
    /// (e.g. `100` for 1%).
    pub async fn swap(
        &self,
        chain_id: u64,
        src: &str,
        dst: &str,
        amount: &str,
        from_address: &str,
        slippage_bps: u16,
    ) -> Result<SwapTx, ExchangeError> {
        let slippage_pct = (slippage_bps as f64) / 100.0;
        let url = format!("{}/swap/v6.0/{}/swap", self.base_url, chain_id);
        let slippage_str = format!("{}", slippage_pct);
        let req = self.http.get(&url).query(&[
            ("src", src),
            ("dst", dst),
            ("amount", amount),
            ("from", from_address),
            ("slippage", slippage_str.as_str()),
            ("disableEstimate", "false"),
        ]);
        let resp = self
            .auth(req)
            .send()
            .await
            .map_err(|e| ExchangeError::Network(e.to_string()))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| ExchangeError::Network(e.to_string()))?;
        if !status.is_success() {
            return Err(ExchangeError::Aggregator(extract_error(
                &body,
                status.as_u16(),
            )));
        }
        let v: serde_json::Value =
            serde_json::from_str(&body).map_err(|e| ExchangeError::Codec(e.to_string()))?;

        let tx = v
            .get("tx")
            .ok_or_else(|| ExchangeError::Codec("missing tx".into()))?;
        let to = string_field(tx, "to")?;
        let data = string_field(tx, "data")?;
        let value = string_field(tx, "value")?;
        let gas = tx.get("gas").and_then(|x| x.as_u64()).unwrap_or(0);
        let to_amount = v
            .get("dstAmount")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();

        Ok(SwapTx {
            to,
            data,
            value,
            gas,
            to_amount,
        })
    }
}

fn string_field(obj: &serde_json::Value, key: &str) -> Result<String, ExchangeError> {
    obj.get(key)
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| ExchangeError::Codec(format!("missing {key}")))
}

fn collect_protocol_names(arr: &[serde_json::Value]) -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    fn walk(v: &serde_json::Value, names: &mut std::collections::BTreeSet<String>) {
        match v {
            serde_json::Value::Array(items) => {
                for it in items {
                    walk(it, names);
                }
            }
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::String(name)) = map.get("name") {
                    names.insert(name.clone());
                }
                for (_, child) in map {
                    walk(child, names);
                }
            }
            _ => {}
        }
    }
    for item in arr {
        walk(item, &mut names);
    }
    names.into_iter().collect()
}

fn extract_error(body: &str, status: u16) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(d) = v.get("description").and_then(|x| x.as_str()) {
            return d.to_string();
        }
        if let Some(d) = v.get("error").and_then(|x| x.as_str()) {
            return d.to_string();
        }
    }
    format!(
        "status {status}: {}",
        body.chars().take(200).collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_protocol_names_dedupes() {
        let raw = serde_json::json!([
            [
                [
                    { "name": "UNISWAP_V3", "part": 70 },
                    { "name": "UNISWAP_V2", "part": 30 }
                ]
            ],
            [
                [{ "name": "UNISWAP_V3", "part": 100 }]
            ]
        ]);
        let arr = raw.as_array().unwrap();
        let names = collect_protocol_names(arr);
        assert_eq!(
            names,
            vec!["UNISWAP_V2".to_string(), "UNISWAP_V3".to_string()]
        );
    }

    #[test]
    fn extract_error_pulls_description() {
        let body = r#"{"description":"insufficient liquidity"}"#;
        assert_eq!(extract_error(body, 400), "insufficient liquidity");
    }

    #[test]
    fn extract_error_falls_back_to_status_snippet() {
        let body = "<html>Not Found</html>";
        assert!(extract_error(body, 404).starts_with("status 404"));
    }
}
