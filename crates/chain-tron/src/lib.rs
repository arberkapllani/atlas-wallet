//! Tron chain provider.
//!
//! v2 surface: address validation, native TRX balance, TRC-20 token reads
//! (via `triggerconstantcontract`), **and** native TRX + TRC-20 sending
//! through a hand-rolled protobuf encoder (see [`crate::transaction`]).

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod proto;
pub mod transaction;

use async_trait::async_trait;
use atlas_chain_traits::{
    Amount, Asset, ChainError, ChainProvider, ChainResult, FeeOption, SignedTx, TxRequest,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::transaction::{
    encode_raw_tx, sign_raw_tx, transfer_contract_envelope, trc20_transfer_contract_envelope,
    RawTxParams, TronAddress, DEFAULT_TRC20_FEE_LIMIT_SUN,
};

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
            http: atlas_net::http_client_builder()
                .user_agent("Atlas/0.1")
                .build()
                .unwrap_or_else(|_| atlas_net::http_client()),
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
        Self::base58_to_bytes(addr).map(hex::encode)
    }

    /// Decode a base58check Tron address into its 21-byte payload
    /// (`0x41` prefix + 20-byte hash).
    pub fn base58_to_bytes(addr: &str) -> Option<TronAddress> {
        let bytes = bs58::decode(addr).into_vec().ok()?;
        if bytes.len() != 25 || bytes[0] != 0x41 {
            return None;
        }
        let payload = &bytes[..21];
        let h1 = Sha256::digest(payload);
        let h2 = Sha256::digest(h1);
        if h2[..4] != bytes[21..] {
            return None;
        }
        let mut out = [0u8; 21];
        out.copy_from_slice(payload);
        Some(out)
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Fetch the latest head block and extract `(ref_block_bytes,
    /// ref_block_hash)` — the two reference fields every Tron transaction
    /// must include to be valid for the next ~3 minutes.
    async fn latest_block_ref(&self) -> ChainResult<([u8; 2], [u8; 8])> {
        #[derive(Deserialize)]
        struct RawData {
            number: u64,
        }
        #[derive(Deserialize)]
        struct Header {
            raw_data: RawData,
        }
        #[derive(Deserialize)]
        struct Block {
            #[serde(rename = "blockID")]
            block_id: String,
            block_header: Header,
        }

        let block: Block = self
            .post("/wallet/getnowblock", serde_json::json!({}))
            .await?;

        let num_be = block.block_header.raw_data.number.to_be_bytes();
        let mut ref_block_bytes = [0u8; 2];
        ref_block_bytes.copy_from_slice(&num_be[6..8]);

        let id_bytes = hex::decode(&block.block_id)
            .map_err(|e| ChainError::Codec(format!("blockID hex: {e}")))?;
        if id_bytes.len() < 16 {
            return Err(ChainError::Codec("blockID too short".into()));
        }
        let mut ref_block_hash = [0u8; 8];
        ref_block_hash.copy_from_slice(&id_bytes[8..16]);

        Ok((ref_block_bytes, ref_block_hash))
    }

    /// Build, sign, and return a TRC-20 `transfer(address, uint256)`
    /// transaction ready for [`ChainProvider::broadcast`].
    pub async fn build_and_sign_trc20(
        &self,
        from: &str,
        to: &str,
        contract: &str,
        amount: u128,
        private_key: &[u8; 32],
    ) -> ChainResult<SignedTx> {
        let owner =
            Self::base58_to_bytes(from).ok_or_else(|| ChainError::InvalidAddress(from.into()))?;
        let recipient =
            Self::base58_to_bytes(to).ok_or_else(|| ChainError::InvalidAddress(to.into()))?;
        let contract_addr = Self::base58_to_bytes(contract)
            .ok_or_else(|| ChainError::InvalidAddress(contract.into()))?;

        let mut to_evm = [0u8; 20];
        to_evm.copy_from_slice(&recipient[1..]);

        let (ref_block_bytes, ref_block_hash) = self.latest_block_ref().await?;
        let ts = Self::now_ms();
        let contract_envelope =
            trc20_transfer_contract_envelope(&owner, &contract_addr, &to_evm, amount);
        let raw = encode_raw_tx(&RawTxParams {
            ref_block_bytes: &ref_block_bytes,
            ref_block_hash: &ref_block_hash,
            timestamp_ms: ts,
            expiration_ms: ts + 60_000,
            fee_limit_sun: Some(DEFAULT_TRC20_FEE_LIMIT_SUN),
            contract: contract_envelope,
        });
        let signed = sign_raw_tx(&raw, private_key).map_err(ChainError::Sign)?;

        Ok(SignedTx {
            raw_hex: signed.raw_hex,
            txid: signed.txid_hex,
            fee: Amount::new(DEFAULT_TRC20_FEE_LIMIT_SUN as u128, self.asset.clone()),
        })
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
        request: TxRequest,
        private_key: &[u8; 32],
    ) -> ChainResult<SignedTx> {
        let owner = Self::base58_to_bytes(&request.from)
            .ok_or_else(|| ChainError::InvalidAddress(request.from.clone()))?;
        let recipient = Self::base58_to_bytes(&request.to)
            .ok_or_else(|| ChainError::InvalidAddress(request.to.clone()))?;

        if request.amount.value > u64::MAX as u128 {
            return Err(ChainError::Other("amount exceeds u64 sun range".into()));
        }
        let amount_sun = request.amount.value as u64;

        let (ref_block_bytes, ref_block_hash) = self.latest_block_ref().await?;
        let ts = Self::now_ms();
        let contract_envelope = transfer_contract_envelope(&owner, &recipient, amount_sun);
        let raw = encode_raw_tx(&RawTxParams {
            ref_block_bytes: &ref_block_bytes,
            ref_block_hash: &ref_block_hash,
            timestamp_ms: ts,
            expiration_ms: ts + 60_000,
            fee_limit_sun: None,
            contract: contract_envelope,
        });
        let signed = sign_raw_tx(&raw, private_key).map_err(ChainError::Sign)?;

        Ok(SignedTx {
            raw_hex: signed.raw_hex,
            txid: signed.txid_hex,
            fee: Amount::new(100_000, self.asset.clone()), // 0.1 TRX bandwidth
        })
    }

    async fn broadcast(&self, signed: &SignedTx) -> ChainResult<String> {
        #[derive(Deserialize)]
        struct BroadcastResp {
            #[serde(default)]
            result: bool,
            #[serde(default)]
            txid: Option<String>,
            #[serde(default)]
            code: Option<String>,
            #[serde(default)]
            message: Option<String>,
        }
        let body = serde_json::json!({ "transaction": signed.raw_hex });
        let r: BroadcastResp = self.post("/wallet/broadcasthex", body).await?;
        if r.result {
            Ok(r.txid.unwrap_or_else(|| signed.txid.clone()))
        } else {
            let detail = r.message.unwrap_or_else(|| r.code.unwrap_or_default());
            Err(ChainError::Rpc(if detail.is_empty() {
                "broadcast rejected".into()
            } else {
                // The Tron API returns the message hex-encoded.
                hex::decode(&detail)
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok())
                    .unwrap_or(detail)
            }))
        }
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
