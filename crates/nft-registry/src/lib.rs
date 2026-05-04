//! Watch-only NFT discovery via the [Reservoir] aggregator.
//!
//! Reservoir indexes NFTs across most major EVM marketplaces
//! (OpenSea, Blur, LooksRare, X2Y2, …) and exposes a unified REST
//! API. We use it for read-only display only; minting, listing and
//! transfers are not part of this crate.
//!
//! [Reservoir]: https://docs.reservoir.tools/

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Reservoir host per Atlas EVM chain id. Only chains where
/// Reservoir publishes a v3 host are supported; everything else
/// returns [`NftError::UnsupportedChain`].
pub fn reservoir_host_for(chain_id: &str) -> Option<&'static str> {
    match chain_id {
        "eth" => Some("https://api.reservoir.tools"),
        "polygon" => Some("https://api-polygon.reservoir.tools"),
        "arbitrum" => Some("https://api-arbitrum.reservoir.tools"),
        "optimism" => Some("https://api-optimism.reservoir.tools"),
        "base" => Some("https://api-base.reservoir.tools"),
        "bsc" => Some("https://api-bsc.reservoir.tools"),
        "avalanche" => Some("https://api-avalanche.reservoir.tools"),
        _ => None,
    }
}

/// Atlas EVM chain ids for which a Reservoir endpoint is wired in.
pub const SUPPORTED_CHAINS: &[&str] = &[
    "eth",
    "polygon",
    "arbitrum",
    "optimism",
    "base",
    "bsc",
    "avalanche",
];

/// Errors surfaced to the IPC layer.
#[derive(Debug, Error)]
pub enum NftError {
    #[error("unsupported chain `{0}`")]
    UnsupportedChain(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("decode error: {0}")]
    Decode(String),
}

/// One NFT a user owns. Mirrors the subset of Reservoir's
/// `/users/{user}/tokens/v6` payload that the UI actually renders.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct OwnedNft {
    /// Atlas chain id this NFT lives on.
    pub chain_id: String,
    /// EVM contract address (checksum or lowercase).
    pub contract: String,
    /// Token id as a decimal string (NFTs are u256).
    pub token_id: String,
    /// Optional human-readable name (`"BAYC #1234"`).
    pub name: Option<String>,
    /// Collection display name.
    pub collection: Option<String>,
    /// Best-effort image URI (already gateway-rewritten by
    /// Reservoir when possible).
    pub image: Option<String>,
    /// Most recent floor-ask price in USD. Watch-only — no
    /// guarantee the NFT can actually be sold for this.
    pub floor_usd: Option<f64>,
}

// -- Reservoir wire types --------------------------------------------------
// We model only the fields we read; everything else is ignored.

#[derive(Debug, Deserialize)]
struct UsersTokensResponse {
    tokens: Vec<TokenEntry>,
}

#[derive(Debug, Deserialize)]
struct TokenEntry {
    token: Token,
}

#[derive(Debug, Deserialize)]
struct Token {
    contract: String,
    #[serde(rename = "tokenId")]
    token_id: String,
    name: Option<String>,
    image: Option<String>,
    collection: Option<Collection>,
    #[serde(rename = "floorAsk")]
    floor_ask: Option<FloorAsk>,
}

#[derive(Debug, Deserialize)]
struct Collection {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FloorAsk {
    price: Option<Price>,
}

#[derive(Debug, Deserialize)]
struct Price {
    amount: Option<Amount>,
}

#[derive(Debug, Deserialize)]
struct Amount {
    usd: Option<f64>,
}

/// Fetch the NFTs `address` owns on `chain_id` (max 200 — Reservoir's
/// hard cap per page).
pub async fn fetch_owned_nfts(
    chain_id: &str,
    address: &str,
    api_key: Option<&str>,
) -> Result<Vec<OwnedNft>, NftError> {
    let host =
        reservoir_host_for(chain_id).ok_or_else(|| NftError::UnsupportedChain(chain_id.into()))?;
    let url = format!("{host}/users/{address}/tokens/v6?limit=200&includeTopBid=false");
    let client = reqwest::Client::new();
    let mut req = client.get(&url);
    if let Some(key) = api_key {
        req = req.header("x-api-key", key);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| NftError::Network(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(NftError::Network(format!("status {}", resp.status())));
    }
    let body: UsersTokensResponse = resp
        .json()
        .await
        .map_err(|e| NftError::Decode(e.to_string()))?;
    Ok(body
        .tokens
        .into_iter()
        .map(|e| OwnedNft {
            chain_id: chain_id.to_string(),
            contract: e.token.contract,
            token_id: e.token.token_id,
            name: e.token.name,
            collection: e.token.collection.and_then(|c| c.name),
            image: e.token.image,
            floor_usd: e
                .token
                .floor_ask
                .and_then(|f| f.price)
                .and_then(|p| p.amount)
                .and_then(|a| a.usd),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_table_round_trip() {
        for c in SUPPORTED_CHAINS {
            assert!(reservoir_host_for(c).is_some(), "missing host for {c}");
        }
    }

    #[test]
    fn unsupported_chain_returns_none() {
        assert!(reservoir_host_for("solana").is_none());
        assert!(reservoir_host_for("btc").is_none());
        assert!(reservoir_host_for("").is_none());
    }

    #[test]
    fn parses_minimal_token_payload() {
        let body = r#"{
            "tokens": [
                {
                    "token": {
                        "contract": "0xabc",
                        "tokenId": "1",
                        "name": "Test #1",
                        "image": "https://img/1.png",
                        "collection": { "name": "Test" },
                        "floorAsk": { "price": { "amount": { "usd": 12.5 } } }
                    }
                },
                {
                    "token": {
                        "contract": "0xdef",
                        "tokenId": "2"
                    }
                }
            ]
        }"#;
        let parsed: UsersTokensResponse = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.tokens.len(), 2);
        assert_eq!(parsed.tokens[0].token.token_id, "1");
        assert_eq!(parsed.tokens[1].token.contract, "0xdef");
        assert!(parsed.tokens[0].token.floor_ask.is_some());
        assert!(parsed.tokens[1].token.floor_ask.is_none());
    }
}
