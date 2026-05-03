//! EVM chain provider — covers Ethereum mainnet and EVM-compatible L2s.
//!
//! All networks share the same address (BIP-44 m/44'/60'/0'/0/0) so the
//! caller chooses *which* chain to query/send to via [`Network`].
//!
//! Built without `alloy` to keep the dependency surface tiny: JSON-RPC over
//! `reqwest`, secp256k1 from the same crate Bitcoin uses, keccak256 from
//! `sha3`, and a small hand-rolled RLP encoder in [`builder`].

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod builder;
pub mod erc20;
pub mod networks;

pub use networks::{Network, NETWORKS};

use async_trait::async_trait;
use atlas_chain_traits::{
    Amount, Asset, ChainError, ChainProvider, ChainResult, FeeOption, SignedTx, TxRequest,
};
use serde::{Deserialize, Serialize};

/// Generic EVM provider, parameterised by a [`Network`].
pub struct EvmProvider {
    network: &'static Network,
    rpc_url: String,
    http: reqwest::Client,
    asset: Asset,
}

impl EvmProvider {
    /// Construct a provider for the given EVM network using the network's
    /// built-in default RPC URL.
    pub fn new(network: &'static Network) -> Self {
        Self::with_rpc(network, network.rpc_url.to_string())
    }

    /// Construct a provider pointed at a user-supplied RPC URL (e.g. their
    /// own node). Atlas treats this override as authoritative.
    pub fn with_rpc(network: &'static Network, rpc_url: String) -> Self {
        Self {
            network,
            rpc_url,
            http: reqwest::Client::builder()
                .user_agent("Atlas/0.1")
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            asset: Asset {
                id: network.id.into(),
                symbol: network.symbol.into(),
                decimals: 18,
                logo: None,
            },
        }
    }

    /// Underlying [`Network`] descriptor.
    pub fn network(&self) -> &'static Network {
        self.network
    }

    /// The effective RPC URL this provider will dial.
    pub fn rpc_url(&self) -> &str {
        &self.rpc_url
    }

    /// Issue a JSON-RPC call against this network's default RPC URL.
    pub async fn rpc<T: for<'de> Deserialize<'de>>(
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

    /// Read an ERC-20 `balanceOf(holder)` from the given contract address.
    ///
    /// `holder` and `contract` are 0x-prefixed hex addresses. Returns the
    /// raw token balance in base units; the caller is responsible for
    /// applying the token's decimals.
    pub async fn token_balance(&self, holder: &str, contract: &str) -> ChainResult<u128> {
        let holder_bytes =
            parse_hex_address(holder).ok_or_else(|| ChainError::InvalidAddress(holder.into()))?;
        let calldata = erc20::balance_of_calldata(&holder_bytes);
        let data_hex = format!("0x{}", hex::encode(calldata));
        let result_hex: String = self
            .rpc(
                "eth_call",
                serde_json::json!([
                    { "to": contract, "data": data_hex },
                    "latest"
                ]),
            )
            .await?;
        parse_hex_u128(&result_hex)
    }
}

fn parse_hex_address(s: &str) -> Option<[u8; 20]> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    if s.len() != 40 {
        return None;
    }
    let mut out = [0u8; 20];
    hex::decode_to_slice(s, &mut out).ok()?;
    Some(out)
}

#[async_trait]
impl ChainProvider for EvmProvider {
    fn id(&self) -> &'static str {
        self.network.id
    }
    fn display_name(&self) -> &'static str {
        self.network.display_name
    }
    fn native_asset(&self) -> &Asset {
        &self.asset
    }

    fn validate_address(&self, address: &str) -> bool {
        // Accept lowercase or EIP-55 mixed-case 0x-prefixed 20-byte hex.
        let s = address.strip_prefix("0x").unwrap_or(address);
        s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit())
    }

    async fn balance(&self, address: &str) -> ChainResult<Amount> {
        let hex_balance: String = self
            .rpc("eth_getBalance", serde_json::json!([address, "latest"]))
            .await?;
        let wei = parse_hex_u128(&hex_balance)?;
        Ok(Amount::new(wei, self.asset.clone()))
    }

    async fn fee_options(&self) -> ChainResult<Vec<FeeOption>> {
        let gas_price_hex: String = self.rpc("eth_gasPrice", serde_json::json!([])).await?;
        let base_gas_price = parse_hex_u128(&gas_price_hex)?;
        // Standard 21k gas for a native transfer.
        let gas_limit: u128 = 21_000;
        let to_opt = |level: &str, mul_num: u128, mul_den: u128, eta: u32| FeeOption {
            level: level.into(),
            estimated_fee: Amount::new(
                base_gas_price * gas_limit * mul_num / mul_den,
                self.asset.clone(),
            ),
            eta_seconds: eta,
            // raw_hint encodes the chosen gas-price in wei as a decimal string.
            raw_hint: (base_gas_price * mul_num / mul_den).to_string(),
        };
        Ok(vec![
            to_opt("slow", 9, 10, 60 * 5),
            to_opt("normal", 1, 1, 60),
            to_opt("fast", 13, 10, 15),
        ])
    }

    async fn build_and_sign(
        &self,
        request: TxRequest,
        private_key: &[u8; 32],
    ) -> ChainResult<SignedTx> {
        // Nonce
        let nonce_hex: String = self
            .rpc(
                "eth_getTransactionCount",
                serde_json::json!([request.from, "pending"]),
            )
            .await?;
        let nonce = parse_hex_u128(&nonce_hex)? as u64;

        // Fee parameters
        let gas_price = if let Ok(v) = request.fee_level.parse::<u128>() {
            v
        } else {
            let gas_price_hex: String = self.rpc("eth_gasPrice", serde_json::json!([])).await?;
            let base = parse_hex_u128(&gas_price_hex)?;
            match request.fee_level.as_str() {
                "slow" => base * 9 / 10,
                "fast" => base * 13 / 10,
                _ => base,
            }
        };

        let gas_limit: u64 = 21_000;
        let value: u128 = request.amount.value;

        let signed = builder::sign_eip1559(
            self.network.chain_id,
            nonce,
            gas_price,     // priority
            gas_price * 2, // max
            gas_limit,
            &request.to,
            value,
            &[],
            private_key,
        )
        .map_err(ChainError::Sign)?;

        let total_fee = gas_price.saturating_mul(gas_limit as u128);
        Ok(SignedTx {
            raw_hex: signed.raw_hex,
            txid: signed.tx_hash,
            fee: Amount::new(total_fee, self.asset.clone()),
        })
    }

    async fn broadcast(&self, signed: &SignedTx) -> ChainResult<String> {
        let raw = if signed.raw_hex.starts_with("0x") {
            signed.raw_hex.clone()
        } else {
            format!("0x{}", signed.raw_hex)
        };
        let txid: String = self
            .rpc("eth_sendRawTransaction", serde_json::json!([raw]))
            .await?;
        Ok(txid)
    }
}

fn parse_hex_u128(s: &str) -> ChainResult<u128> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    if s.is_empty() {
        return Ok(0);
    }
    u128::from_str_radix(s, 16).map_err(|e| ChainError::Codec(e.to_string()))
}
