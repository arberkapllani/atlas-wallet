//! Solana chain provider.
//!
//! v1 surface: address validation, native SOL balance, fee preview.
//! Transaction sending requires recent-blockhash lookup and ed25519
//! transaction encoding — those land alongside the swap aggregator
//! (Phase 4 / Jupiter integration). Until then [`build_and_sign`]
//! returns a clear `ChainError::Other` so the UI can surface a
//! "send not yet available" hint.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

use async_trait::async_trait;
use atlas_chain_traits::{
    Amount, Asset, ChainError, ChainProvider, ChainResult, FeeOption, SignedTx, TxRequest,
};
use serde::{Deserialize, Serialize};

/// Default mainnet RPC endpoint (public, rate-limited).
pub const DEFAULT_RPC: &str = "https://api.mainnet-beta.solana.com";

/// Lamports per SOL.
pub const LAMPORTS_PER_SOL: u128 = 1_000_000_000;

/// Solana mainnet provider.
pub struct SolanaProvider {
    http: reqwest::Client,
    rpc_url: String,
    asset: Asset,
}

impl SolanaProvider {
    /// Construct a provider pointing at [`DEFAULT_RPC`].
    pub fn new() -> Self {
        Self::with_rpc(DEFAULT_RPC.into())
    }

    /// Construct a provider pointing at a custom RPC endpoint.
    pub fn with_rpc(rpc_url: String) -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent("Atlas/0.1")
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            rpc_url,
            asset: Asset {
                id: "sol".into(),
                symbol: "SOL".into(),
                decimals: 9,
                logo: None,
            },
        }
    }

    async fn rpc<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> ChainResult<T> {
        #[derive(Serialize)]
        struct Req<'a> {
            jsonrpc: &'static str,
            id: u32,
            method: &'a str,
            params: serde_json::Value,
        }
        #[derive(Deserialize)]
        struct Resp<T> {
            result: Option<T>,
            error: Option<RpcErr>,
        }
        #[derive(Deserialize)]
        struct RpcErr {
            message: String,
        }
        let body = Req {
            jsonrpc: "2.0",
            id: 1,
            method,
            params,
        };
        let resp = self
            .http
            .post(&self.rpc_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ChainError::Network(e.to_string()))?;
        let parsed: Resp<T> = resp
            .json()
            .await
            .map_err(|e| ChainError::Codec(e.to_string()))?;
        if let Some(e) = parsed.error {
            return Err(ChainError::Rpc(e.message));
        }
        parsed
            .result
            .ok_or_else(|| ChainError::Rpc("empty result".into()))
    }
}

impl Default for SolanaProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChainProvider for SolanaProvider {
    fn id(&self) -> &'static str {
        "sol"
    }
    fn display_name(&self) -> &'static str {
        "Solana"
    }
    fn native_asset(&self) -> &Asset {
        &self.asset
    }

    fn validate_address(&self, address: &str) -> bool {
        // Solana addresses are base58 ed25519 public keys (32 bytes).
        match bs58::decode(address).into_vec() {
            Ok(bytes) => bytes.len() == 32,
            Err(_) => false,
        }
    }

    async fn balance(&self, address: &str) -> ChainResult<Amount> {
        if !self.validate_address(address) {
            return Err(ChainError::InvalidAddress(address.into()));
        }
        #[derive(Deserialize)]
        struct GetBalanceResult {
            value: u64,
        }
        let r: GetBalanceResult = self.rpc("getBalance", serde_json::json!([address])).await?;
        Ok(Amount::new(r.value as u128, self.asset.clone()))
    }

    async fn fee_options(&self) -> ChainResult<Vec<FeeOption>> {
        // Solana fees are essentially flat (5000 lamports / signature).
        // We expose a single tier; the UI may collapse the picker for SOL.
        let lamports: u128 = 5_000;
        let opt = |level: &str, eta: u32| FeeOption {
            level: level.into(),
            estimated_fee: Amount::new(lamports, self.asset.clone()),
            eta_seconds: eta,
            raw_hint: lamports.to_string(),
        };
        Ok(vec![opt("slow", 30), opt("normal", 15), opt("fast", 5)])
    }

    async fn build_and_sign(
        &self,
        _request: TxRequest,
        _private_key: &[u8; 32],
    ) -> ChainResult<SignedTx> {
        Err(ChainError::Other(
            "solana sending will land with the swap aggregator (Phase 4)".into(),
        ))
    }

    async fn broadcast(&self, _signed: &SignedTx) -> ChainResult<String> {
        Err(ChainError::Other(
            "solana sending will land with the swap aggregator (Phase 4)".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_known_solana_address() {
        let p = SolanaProvider::new();
        // Solana System Program — well-known, decodes to 32 bytes.
        assert!(p.validate_address("11111111111111111111111111111111"));
    }

    #[test]
    fn rejects_garbage_address() {
        let p = SolanaProvider::new();
        assert!(!p.validate_address(""));
        assert!(!p.validate_address("0xdeadbeef"));
        assert!(!p.validate_address("not-base58-!!"));
    }

    #[test]
    fn lamports_constant() {
        assert_eq!(LAMPORTS_PER_SOL, 1_000_000_000);
    }
}
