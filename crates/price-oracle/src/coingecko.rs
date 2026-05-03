//! CoinGecko price oracle with simple in-memory TTL cache.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

const TTL: Duration = Duration::from_secs(5 * 60);
const BASE_URL: &str = "https://api.coingecko.com/api/v3/simple/price";

/// A single cached price observation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricePoint {
    /// USD price.
    pub usd: f64,
    /// 24h change as a percentage.
    pub usd_24h_change: f64,
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
    cache: RwLock<HashMap<String, CacheEntry>>,
}

impl PriceOracle {
    /// Build a new oracle.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                http: reqwest::Client::builder()
                    .user_agent("Atlas/0.1")
                    .build()
                    .unwrap_or_else(|_| reqwest::Client::new()),
                cache: RwLock::new(HashMap::new()),
            }),
        }
    }

    /// Fetch (or return cached) prices for the given CoinGecko ids,
    /// e.g. `["bitcoin", "ethereum", "matic-network"]`.
    pub async fn prices(&self, ids: &[&str]) -> Result<HashMap<String, PricePoint>, PriceError> {
        // Cache hit?
        let now = Instant::now();
        {
            let cache = self.inner.cache.read().await;
            if ids.iter().all(|id| {
                cache
                    .get(*id)
                    .map(|e| now.duration_since(e.fetched_at) < TTL)
                    .unwrap_or(false)
            }) {
                return Ok(ids
                    .iter()
                    .map(|id| (id.to_string(), cache[*id].point.clone()))
                    .collect());
            }
        }

        let ids_csv = ids.join(",");
        let url = format!(
            "{BASE_URL}?ids={}&vs_currencies=usd&include_24hr_change=true",
            urlencode(&ids_csv)
        );
        #[derive(Deserialize)]
        struct Raw {
            usd: f64,
            #[serde(default, rename = "usd_24h_change")]
            usd_24h_change: f64,
        }
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
        let body: HashMap<String, Raw> = resp
            .json()
            .await
            .map_err(|e| PriceError::Codec(e.to_string()))?;

        let mut result = HashMap::new();
        let mut cache = self.inner.cache.write().await;
        for (id, raw) in body {
            let p = PricePoint {
                usd: raw.usd,
                usd_24h_change: raw.usd_24h_change,
            };
            cache.insert(
                id.clone(),
                CacheEntry {
                    point: p.clone(),
                    fetched_at: now,
                },
            );
            result.insert(id, p);
        }
        Ok(result)
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

