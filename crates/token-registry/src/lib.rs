//! Static metadata for ERC-20 / TRC-20 tokens that ship enabled by default.
//!
//! Stored as `&'static [TokenMeta]` so callers can lookup without locks
//! or allocation. User-added tokens live in the profile registry and merge
//! over this list at runtime (TBD).

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

use serde::{Deserialize, Serialize};

/// Token contract standard.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
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

// =============================================================================
// Remote / user-imported tokens
// =============================================================================

/// Owned counterpart of [`TokenMeta`] used for tokens fetched at runtime
/// from tokenlists.org or imported manually by the user.
///
/// Static + remote tokens unify behind this type at the IPC boundary so
/// the UI can display both kinds without caring where they came from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
pub struct OwnedTokenMeta {
    /// Stable identifier — for remote tokens this is `chain_id-contract`
    /// in lowercase so duplicates dedupe naturally.
    pub id: String,
    /// On-chain symbol.
    pub symbol: String,
    /// Display name.
    pub display_name: String,
    /// Atlas chain id (`"eth"`, `"polygon"`, …).
    pub chain_id: String,
    /// Hex (EIP-55) or base58 contract address.
    pub contract: String,
    /// On-chain decimals.
    pub decimals: u8,
    /// Token standard.
    pub standard: TokenStandard,
    /// Optional logo URL (only present on remote tokens).
    pub logo_uri: Option<String>,
    /// `"static"`, `"remote"` (tokenlists.org), or `"custom"` (user-added).
    pub source: String,
}

impl From<&TokenMeta> for OwnedTokenMeta {
    fn from(t: &TokenMeta) -> Self {
        Self {
            id: t.id.to_string(),
            symbol: t.symbol.to_string(),
            display_name: t.display_name.to_string(),
            chain_id: t.chain_id.to_string(),
            contract: t.contract.to_string(),
            decimals: t.decimals,
            standard: t.standard,
            logo_uri: None,
            source: "static".into(),
        }
    }
}

/// Errors that can arise when fetching or parsing a remote token list.
#[derive(Debug, thiserror::Error)]
pub enum TokenListError {
    /// Network / transport failure.
    #[error("network error: {0}")]
    Network(String),
    /// JSON parsing failure.
    #[error("parse error: {0}")]
    Parse(String),
}

/// Map an EVM numeric chain id (EIP-155) to the Atlas chain id used in
/// our chain registry. Unknown chains return `None` and are filtered out
/// during parsing — Atlas only surfaces tokens for chains it can talk to.
pub fn atlas_id_for_evm_chain_id(chain_id: u64) -> Option<&'static str> {
    Some(match chain_id {
        1 => "eth",
        137 => "polygon",
        42161 => "arbitrum",
        10 => "optimism",
        8453 => "base",
        56 => "bsc",
        43114 => "avalanche",
        _ => return None,
    })
}

/// Parse a Uniswap-format token list JSON document into Atlas token
/// metadata. Tokens for unsupported chains are silently dropped.
///
/// Reference schema:
/// <https://uniswap.org/tokenlist.schema.json>
pub fn parse_uniswap_token_list(body: &[u8]) -> Result<Vec<OwnedTokenMeta>, TokenListError> {
    #[derive(Deserialize)]
    struct ListDoc {
        tokens: Vec<RemoteToken>,
    }
    #[derive(Deserialize)]
    struct RemoteToken {
        #[serde(rename = "chainId")]
        chain_id: u64,
        address: String,
        symbol: String,
        name: String,
        decimals: u8,
        #[serde(rename = "logoURI", default)]
        logo_uri: Option<String>,
    }

    let doc: ListDoc =
        serde_json::from_slice(body).map_err(|e| TokenListError::Parse(e.to_string()))?;
    let mut out = Vec::with_capacity(doc.tokens.len());
    for t in doc.tokens {
        let Some(atlas) = atlas_id_for_evm_chain_id(t.chain_id) else {
            continue;
        };
        let contract_lc = t.address.to_lowercase();
        out.push(OwnedTokenMeta {
            id: format!("{atlas}-{contract_lc}"),
            symbol: t.symbol,
            display_name: t.name,
            chain_id: atlas.into(),
            contract: t.address,
            decimals: t.decimals,
            standard: TokenStandard::Erc20,
            logo_uri: t.logo_uri,
            source: "remote".into(),
        });
    }
    Ok(out)
}

/// Fetch and parse a Uniswap-format token list. The caller is expected
/// to use a curated URL such as `https://tokens.uniswap.org` or one of
/// the lists registered at <https://tokenlists.org>.
pub async fn fetch_token_list(url: &str) -> Result<Vec<OwnedTokenMeta>, TokenListError> {
    let resp = atlas_net::http_client_builder()
        .user_agent("Atlas/0.1")
        .build()
        .map_err(|e| TokenListError::Network(e.to_string()))?
        .get(url)
        .send()
        .await
        .map_err(|e| TokenListError::Network(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(TokenListError::Network(format!(
            "{} from {}",
            resp.status(),
            url
        )));
    }
    let body = resp
        .bytes()
        .await
        .map_err(|e| TokenListError::Network(e.to_string()))?;
    parse_uniswap_token_list(&body)
}

/// Curated token-list URLs Atlas ships with — users can add more in
/// settings. These are the official Uniswap and CoinGecko lists, both
/// signed and audited by their maintainers.
pub const DEFAULT_TOKEN_LIST_URLS: &[&str] = &[
    "https://tokens.uniswap.org",
    "https://tokens.coingecko.com/uniswap/all.json",
];

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

    #[test]
    fn parse_uniswap_list_filters_unknown_chains() {
        let json = br#"{
            "name": "Test",
            "tokens": [
                {
                    "chainId": 1,
                    "address": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
                    "symbol": "USDC",
                    "name": "USD Coin",
                    "decimals": 6,
                    "logoURI": "https://example.com/usdc.png"
                },
                {
                    "chainId": 137,
                    "address": "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174",
                    "symbol": "USDC",
                    "name": "USD Coin (Polygon)",
                    "decimals": 6
                },
                {
                    "chainId": 999999,
                    "address": "0xdeadbeef",
                    "symbol": "FAKE",
                    "name": "Fake",
                    "decimals": 18
                }
            ]
        }"#;
        let tokens = parse_uniswap_token_list(json).expect("parses");
        assert_eq!(tokens.len(), 2, "unsupported chains must be filtered");
        assert_eq!(tokens[0].chain_id, "eth");
        assert_eq!(tokens[0].symbol, "USDC");
        assert_eq!(tokens[0].source, "remote");
        assert!(tokens[0].id.starts_with("eth-0x"));
        assert_eq!(tokens[1].chain_id, "polygon");
    }

    #[test]
    fn parse_uniswap_list_rejects_garbage() {
        let res = parse_uniswap_token_list(b"not json");
        assert!(matches!(res, Err(TokenListError::Parse(_))));
    }

    #[test]
    fn evm_chain_id_mapping() {
        assert_eq!(atlas_id_for_evm_chain_id(1), Some("eth"));
        assert_eq!(atlas_id_for_evm_chain_id(137), Some("polygon"));
        assert_eq!(atlas_id_for_evm_chain_id(0), None);
    }

    #[test]
    fn owned_from_static() {
        let static_t = by_id("usdt-erc20").unwrap();
        let owned: OwnedTokenMeta = static_t.into();
        assert_eq!(owned.source, "static");
        assert_eq!(owned.symbol, "USDT");
    }
}
