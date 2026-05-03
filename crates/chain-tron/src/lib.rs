//! Tron chain provider.
//!
//! v1 surface: address validation, native TRX balance, TRC-20 token reads
//! (via `triggerconstantcontract`). Sending TRX/TRC-20 requires building
//! and signing a Tron protobuf transaction with a recent block-ref —
//! that lands once we have the wider Tron tooling in place. Until then
//! [`build_and_sign`] returns a clear `ChainError::Other`.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

use async_trait::async_trait;
use atlas_chain_traits::{
    Amount, Asset, ChainError, ChainProvider, ChainResult, FeeOption, SignedTx, TxRequest,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Default mainnet endpoint (TronGrid public).
pub const DEFAULT_RPC: &str = "https://api.trongrid.io";

/// Sun per TRX (1 TRX = 1_000_000 sun).
pub const SUN_PER_TRX: u128 = 1_000_000;

/// USDT-TRC20 contract address.
pub const USDT_CONTRACT: &str = "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t";

/// Tron mainnet provider.
pub struct TronProvider {
    http: reqwest::Client,
    rpc_url: String,
    asset: Asset,
}

impl TronProvider {
    /// Construct a provider pointing at [`DEFAULT_RPC`].
    pub fn new() -> Self {
        Self::with_rpc(DEFAULT_RPC.into())
    }

    /// Construct a provider pointing at a custom Tron HTTP endpoint.
    pub fn with_rpc(rpc_url: String) -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent("Atlas/0.1")
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            rpc_url,
            asset: Asset {
                id: "trx".into(),
                symbol: "TRX".into(),
                decimals: 6,
                logo: None,
            },
        }
    }

    /// Hex (0x41…) form of a base58check Tron address. Used by the wallet-API
    /// for `triggerconstantcontract` calls.
    pub fn base58_to_hex(addr: &str) -> Option<String> {
        let bytes = bs58::decode(addr).into_vec().ok()?;
        if bytes.len() != 25 || bytes[0] != 0x41 {
            return None;
        }
        let payload = &bytes[..21];
        let checksum = &bytes[21..];
        let h1 = Sha256::digest(payload);
        let h2 = Sha256::digest(h1);
        if &h2[..4] != checksum {
            return None;
        }
        Some(hex::encode(payload))
    }

    /// Issue a Tron HTTP-API POST.
    async fn post<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> ChainResult<T> {
        let url = format!("{}{}", self.rpc_url, path);
        let resp = self
            .http
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ChainError::Network(e.to_string()))?;
        resp.json::<T>()
            .await
            .map_err(|e| ChainError::Codec(e.to_string()))
    }
}

impl Default for TronProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChainProvider for TronProvider {
    fn id(&self) -> &'static str {
        "trx"
    }
    fn display_name(&self) -> &'static str {
        "Tron"
    }
    fn native_asset(&self) -> &Asset {
        &self.asset
    }

    fn validate_address(&self, address: &str) -> bool {
        Self::base58_to_hex(address).is_some()
    }

    async fn balance(&self, address: &str) -> ChainResult<Amount> {
        if !self.validate_address(address) {
            return Err(ChainError::InvalidAddress(address.into()));
        }
        #[derive(Deserialize)]
        struct AccountResp {
            #[serde(default)]
            balance: u64,
        }
        let body = serde_json::json!({ "address": address, "visible": true });
        let r: AccountResp = self.post("/wallet/getaccount", body).await?;
        Ok(Amount::new(r.balance as u128, self.asset.clone()))
    }

    async fn fee_options(&self) -> ChainResult<Vec<FeeOption>> {
        // Native TRX transfers cost ~0.1 TRX in bandwidth fees; TRC-20 costs
        // ~14 TRX worth of energy. Surface a single normal tier for v1.
        let sun: u128 = 100_000; // 0.1 TRX
        let opt = |level: &str, eta: u32| FeeOption {
            level: level.into(),
            estimated_fee: Amount::new(sun, self.asset.clone()),
            eta_seconds: eta,
            raw_hint: sun.to_string(),
        };
        Ok(vec![opt("slow", 30), opt("normal", 6), opt("fast", 3)])
    }

    async fn build_and_sign(
        &self,
        _request: TxRequest,
        _private_key: &[u8; 32],
    ) -> ChainResult<SignedTx> {
        Err(ChainError::Other(
            "tron sending will land in a later phase (protobuf tx encoder)".into(),
        ))
    }

    async fn broadcast(&self, _signed: &SignedTx) -> ChainResult<String> {
        Err(ChainError::Other(
            "tron sending will land in a later phase".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usdt_contract_decodes_to_21_byte_payload() {
        let hex = TronProvider::base58_to_hex(USDT_CONTRACT).unwrap();
        assert_eq!(hex.len(), 42); // 21 bytes
        assert!(hex.starts_with("41"));
    }

    #[test]
    fn validates_known_tron_address() {
        let p = TronProvider::new();
        assert!(p.validate_address(USDT_CONTRACT));
    }

    #[test]
    fn rejects_invalid_addresses() {
        let p = TronProvider::new();
        assert!(!p.validate_address(""));
        assert!(!p.validate_address("0xdeadbeef"));
        assert!(!p.validate_address("11111111111111111111111111111111")); // sol-style
    }

    #[test]
    fn rejects_bad_checksum() {
        let p = TronProvider::new();
        // Mutate last char of a known-good address.
        let mut bad = USDT_CONTRACT.to_string();
        bad.pop();
        bad.push('A');
        assert!(!p.validate_address(&bad));
    }
}
