//! Per-chain RPC health probe.
//!
//! Sends a small, well-known "head height" request 5 times against the
//! effective endpoint for a chain (user override → built-in default) and
//! reports latency p50/p95, the observed head height, and a status bucket.
//!
//! The probe deliberately uses raw HTTP rather than going through each
//! [`ChainProvider`] so a future provider can opt into `network_health`
//! without changing its trait surface.

use serde::Serialize;
use std::time::{Duration, Instant};

use crate::state::{default_endpoint, AppState};

/// Per-chain status bucket surfaced to the UI as a green / amber / red dot.
#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum NetworkStatus {
    /// Endpoint is reachable and responsive.
    Synced,
    /// Endpoint responds but is slow or losing samples.
    Lagging,
    /// All samples failed.
    Offline,
}

/// Result of a single `network_health` probe.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct NetworkHealth {
    /// Chain id this report describes (`"btc"`, `"eth"`, …).
    pub chain_id: String,
    /// Effective endpoint URL we probed.
    pub endpoint: String,
    /// Whether the endpoint URL is the user's override (`true`) or built-in default.
    pub is_user_override: bool,
    /// Number of samples sent.
    pub samples: usize,
    /// Number of samples that returned successfully.
    pub successes: usize,
    /// Median latency across successful samples, in milliseconds.
    pub latency_p50_ms: Option<u64>,
    /// 95th percentile latency across successful samples, in milliseconds.
    pub latency_p95_ms: Option<u64>,
    /// Most recently observed head height (block / slot number).
    pub head_height: Option<u64>,
    /// Bucketed status for the UI.
    pub status: NetworkStatus,
}

const SAMPLES: usize = 5;
/// Above this median latency we mark the endpoint as `Lagging`.
const SLOW_THRESHOLD_MS: u64 = 2000;
/// If at least this many of `SAMPLES` time out we mark `Lagging`.
const FAILURE_LAG_THRESHOLD: usize = 2;

fn percentile(sorted: &[u64], p: f64) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = ((p / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    Some(sorted[rank.min(sorted.len() - 1)])
}

/// Run the probe. Always returns `Ok(_)` — failures are encoded inside the
/// returned [`NetworkHealth`] so the UI can render every chain row.
pub async fn measure(state: &AppState, chain_id: &str) -> NetworkHealth {
    let endpoint = state
        .settings
        .rpc_override(chain_id)
        .or_else(|| default_endpoint(chain_id).map(|s| s.to_string()))
        .unwrap_or_default();
    let is_user_override = state.settings.rpc_override(chain_id).is_some();

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent("Atlas/0.1")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let mut latencies: Vec<u64> = Vec::with_capacity(SAMPLES);
    let mut last_height: Option<u64> = None;
    let mut successes = 0usize;

    for _ in 0..SAMPLES {
        let started = Instant::now();
        let result = probe_once(&http, chain_id, &endpoint).await;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        if let Ok(height) = result {
            successes += 1;
            latencies.push(elapsed_ms);
            last_height = Some(height);
        }
    }

    latencies.sort_unstable();
    let p50 = percentile(&latencies, 50.0);
    let p95 = percentile(&latencies, 95.0);

    let failures = SAMPLES - successes;
    let status = if successes == 0 {
        NetworkStatus::Offline
    } else if failures >= FAILURE_LAG_THRESHOLD
        || p50.map(|m| m > SLOW_THRESHOLD_MS).unwrap_or(false)
    {
        NetworkStatus::Lagging
    } else {
        NetworkStatus::Synced
    };

    NetworkHealth {
        chain_id: chain_id.to_string(),
        endpoint,
        is_user_override,
        samples: SAMPLES,
        successes,
        latency_p50_ms: p50,
        latency_p95_ms: p95,
        head_height: last_height,
        status,
    }
}

/// Send one chain-appropriate "head height" call. Returns the observed
/// block / slot number, or an error string on failure.
async fn probe_once(http: &reqwest::Client, chain_id: &str, endpoint: &str) -> Result<u64, String> {
    if endpoint.is_empty() {
        return Err("no endpoint configured".into());
    }

    match chain_id {
        "btc" => {
            // mempool.space / esplora REST: plain-text height.
            let url = format!("{}/blocks/tip/height", endpoint.trim_end_matches('/'));
            let resp = http.get(&url).send().await.map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                return Err(format!("status {}", resp.status()));
            }
            let body = resp.text().await.map_err(|e| e.to_string())?;
            body.trim().parse::<u64>().map_err(|e| e.to_string())
        }
        "trx" => {
            // Tron HTTP: POST /wallet/getnowblock returns the latest block JSON.
            let url = format!("{}/wallet/getnowblock", endpoint.trim_end_matches('/'));
            let resp = http
                .post(&url)
                .header("content-type", "application/json")
                .body("{}")
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                return Err(format!("status {}", resp.status()));
            }
            let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            v.get("block_header")
                .and_then(|h| h.get("raw_data"))
                .and_then(|r| r.get("number"))
                .and_then(|n| n.as_u64())
                .ok_or_else(|| "missing block_header.raw_data.number".into())
        }
        _ => {
            // Default JSON-RPC families: `eth_blockNumber` for EVM chains,
            // `getSlot` for Solana.
            let (method, hex_result) = if chain_id == "sol" {
                ("getSlot", false)
            } else {
                ("eth_blockNumber", true)
            };
            let body = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": method,
                "params": [],
            });
            let resp = http
                .post(endpoint)
                .json(&body)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                return Err(format!("status {}", resp.status()));
            }
            let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            if let Some(err) = v.get("error") {
                return Err(err.to_string());
            }
            let result = v
                .get("result")
                .ok_or_else(|| "missing result".to_string())?;
            if hex_result {
                let s = result
                    .as_str()
                    .ok_or_else(|| "expected hex string".to_string())?;
                let cleaned = s.trim_start_matches("0x");
                u64::from_str_radix(cleaned, 16).map_err(|e| e.to_string())
            } else {
                result.as_u64().ok_or_else(|| "expected number".into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_handles_empty() {
        assert!(percentile(&[], 50.0).is_none());
    }

    #[test]
    fn percentile_basic() {
        let v = vec![10u64, 20, 30, 40, 50];
        assert_eq!(percentile(&v, 50.0), Some(30));
        assert_eq!(percentile(&v, 95.0), Some(50));
        assert_eq!(percentile(&v, 0.0), Some(10));
    }
}
