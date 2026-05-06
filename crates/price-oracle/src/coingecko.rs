//! CoinGecko price oracle with simple in-memory TTL cache.
//!
//! Cache is keyed by `(coingecko_id, vs_currency)` so switching display
//! currency does not poison earlier results.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

const TTL: Duration = Duration::from_secs(5 * 60);
const BASE_URL: &str = "https://api.coingecko.com/api/v3/simple/price";

/// A single cached price observation.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct PricePoint {
    /// Price in the requested fiat currency.
    pub price: f64,
    /// 24h change as a percentage.
    pub change_24h: f64,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    point: PricePoint,
    fetched_at: Instant,
}

/// Cached price oracle. Cheap to clone (`Arc` inside).
#[derive(Clone)]
pub struct PriceOracle {
    inner: Arc<Inner>,
}

struct Inner {
    http: reqwest::Client,
    cache: RwLock<HashMap<(String, String), CacheEntry>>,
}

impl PriceOracle {
    /// Build a new oracle.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                http: atlas_net::http_client_builder()
                    .user_agent("Atlas/0.1")
                    .build()
                    .unwrap_or_else(|_| atlas_net::http_client()),
                cache: RwLock::new(HashMap::new()),
            }),
        }
    }

    /// Fetch (or return cached) prices for the given CoinGecko ids in the
    /// requested fiat ticker (`"usd"`, `"eur"`, `"gbp"`, …).
    pub async fn prices_in(
        &self,
        ids: &[&str],
        vs_currency: &str,
    ) -> Result<HashMap<String, PricePoint>, PriceError> {
        let vs = vs_currency.to_ascii_lowercase();
        let now = Instant::now();
        // Cache hit?
        {
            let cache = self.inner.cache.read().await;
            if ids.iter().all(|id| {
                cache
                    .get(&(id.to_string(), vs.clone()))
                    .map(|e| now.duration_since(e.fetched_at) < TTL)
                    .unwrap_or(false)
            }) {
                return Ok(ids
                    .iter()
                    .map(|id| {
                        let key = (id.to_string(), vs.clone());
                        (id.to_string(), cache[&key].point.clone())
                    })
                    .collect());
            }
        }

        let ids_csv = ids.join(",");
        let url = format!(
            "{BASE_URL}?ids={}&vs_currencies={}&include_24hr_change=true",
            urlencode(&ids_csv),
            urlencode(&vs),
        );

        // CoinGecko returns
        // `{ "<id>": { "<vs>": <price>, "<vs>_24h_change": <pct> } }`.
        let resp = self
            .inner
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| PriceError::Network(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(PriceError::Network(format!("status {}", resp.status())));
        }
        let body: HashMap<String, HashMap<String, f64>> = resp
            .json()
            .await
            .map_err(|e| PriceError::Codec(e.to_string()))?;

        let change_key = format!("{vs}_24h_change");
        let mut result = HashMap::new();
        let mut cache = self.inner.cache.write().await;
        for (id, fields) in body {
            let price = match fields.get(vs.as_str()) {
                Some(p) => *p,
                None => continue,
            };
            let change_24h = fields.get(&change_key).copied().unwrap_or(0.0);
            let p = PricePoint { price, change_24h };
            cache.insert(
                (id.clone(), vs.clone()),
                CacheEntry {
                    point: p.clone(),
                    fetched_at: now,
                },
            );
            result.insert(id, p);
        }
        Ok(result)
    }

    /// Backwards-compatible USD-only helper.
    pub async fn prices(&self, ids: &[&str]) -> Result<HashMap<String, PricePoint>, PriceError> {
        self.prices_in(ids, "usd").await
    }
}

impl Default for PriceOracle {
    fn default() -> Self {
        Self::new()
    }
}

/// Failure modes for the price oracle.
#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message")]
pub enum PriceError {
    /// Network / transport failure.
    #[error("network: {0}")]
    Network(String),
    /// Response did not match the expected JSON schema.
    #[error("codec: {0}")]
    Codec(String),
}

fn urlencode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' | ',' => c.to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}
