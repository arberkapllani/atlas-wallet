//! Bitcoin provider backed by the mempool.space public REST API.

use crate::builder::{build_and_sign_p2wpkh, Utxo};
use async_trait::async_trait;
use atlas_chain_traits::{
    Amount, Asset, ChainError, ChainProvider, ChainResult, FeeOption, SignedTx, TxRequest,
};
use serde::Deserialize;

/// Bitcoin chain provider.
pub struct BitcoinProvider {
    base_url: String,
    http: reqwest::Client,
    asset: Asset,
}

impl BitcoinProvider {
    /// Construct a provider against the given base URL (e.g. `https://mempool.space/api`).
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: atlas_net::http_client_builder()
                .user_agent("Atlas/0.1")
                .build()
                .unwrap_or_else(|_| atlas_net::http_client()),
            asset: crate::btc_asset(),
        }
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> ChainResult<T> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| ChainError::Network(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(ChainError::Rpc(format!("{} on {}", resp.status(), url)));
        }
        resp.json::<T>()
            .await
            .map_err(|e| ChainError::Codec(e.to_string()))
    }
}

#[derive(Debug, Deserialize)]
struct AddressResp {
    chain_stats: AddrStats,
    mempool_stats: AddrStats,
}

#[derive(Debug, Deserialize)]
struct AddrStats {
    funded_txo_sum: u64,
    spent_txo_sum: u64,
}

#[derive(Debug, Deserialize)]
struct UtxoResp {
    txid: String,
    vout: u32,
    value: u64,
    status: UtxoStatus,
}

#[derive(Debug, Deserialize)]
struct UtxoStatus {
    confirmed: bool,
}

#[derive(Debug, Deserialize)]
struct FeesResp {
    #[serde(rename = "fastestFee")]
    fastest_fee: u64,
    #[serde(rename = "halfHourFee")]
    half_hour_fee: u64,
    #[serde(rename = "hourFee")]
    hour_fee: u64,
}

#[async_trait]
impl ChainProvider for BitcoinProvider {
    fn id(&self) -> &'static str {
        "btc"
    }
    fn display_name(&self) -> &'static str {
        "Bitcoin"
    }
    fn native_asset(&self) -> &Asset {
        &self.asset
    }

    fn validate_address(&self, address: &str) -> bool {
        bitcoin::Address::from_str(address)
            .map(|a| a.require_network(bitcoin::Network::Bitcoin).is_ok())
            .unwrap_or(false)
    }

    async fn balance(&self, address: &str) -> ChainResult<Amount> {
        let resp: AddressResp = self.get_json(&format!("/address/{address}")).await?;
        let confirmed = resp
            .chain_stats
            .funded_txo_sum
            .saturating_sub(resp.chain_stats.spent_txo_sum);
        let pending = resp
            .mempool_stats
            .funded_txo_sum
            .saturating_sub(resp.mempool_stats.spent_txo_sum);
        Ok(Amount::new(
            (confirmed + pending) as u128,
            self.asset.clone(),
        ))
    }

    async fn fee_options(&self) -> ChainResult<Vec<FeeOption>> {
        let fees: FeesResp = self.get_json("/v1/fees/recommended").await?;
        // Approximate fee for a typical 1-in 2-out P2WPKH tx ≈ 141 vbytes.
        let vbytes: u64 = 141;
        let to_opt = |level: &str, sat_per_vb: u64, eta: u32| FeeOption {
            level: level.into(),
            estimated_fee: Amount::new((sat_per_vb * vbytes) as u128, self.asset.clone()),
            eta_seconds: eta,
            raw_hint: sat_per_vb.to_string(),
        };
        Ok(vec![
            to_opt("slow", fees.hour_fee.max(1), 60 * 60),
            to_opt("normal", fees.half_hour_fee.max(1), 30 * 60),
            to_opt("fast", fees.fastest_fee.max(1), 10 * 60),
        ])
    }

    async fn build_and_sign(
        &self,
        request: TxRequest,
        private_key: &[u8; 32],
    ) -> ChainResult<SignedTx> {
        let utxos: Vec<UtxoResp> = self
            .get_json(&format!("/address/{}/utxo", request.from))
            .await?;
        let utxos: Vec<Utxo> = utxos
            .into_iter()
            .filter(|u| u.status.confirmed)
            .map(|u| Utxo {
                txid: u.txid,
                vout: u.vout,
                value_sats: u.value,
            })
            .collect();
        if utxos.is_empty() {
            return Err(ChainError::InsufficientFunds);
        }

        let sat_per_vb: u64 = request
            .fee_level
            .parse::<u64>()
            .ok()
            .or(match request.fee_level.as_str() {
                "slow" => Some(2),
                "normal" => Some(8),
                "fast" => Some(20),
                _ => None,
            })
            .unwrap_or(8);

        let value_sats: u64 = u64::try_from(request.amount.value)
            .map_err(|_| ChainError::Other("amount too large".into()))?;

        let signed = build_and_sign_p2wpkh(
            &request.from,
            &request.to,
            value_sats,
            sat_per_vb,
            &utxos,
            private_key,
        )
        .map_err(|e| ChainError::Sign(e.to_string()))?;

        Ok(SignedTx {
            raw_hex: signed.raw_hex,
            txid: signed.txid,
            fee: Amount::new(signed.fee_sats as u128, self.asset.clone()),
        })
    }

    async fn broadcast(&self, signed: &SignedTx) -> ChainResult<String> {
        let url = format!("{}/tx", self.base_url);
        let resp = self
            .http
            .post(&url)
            .body(signed.raw_hex.clone())
            .send()
            .await
            .map_err(|e| ChainError::Network(e.to_string()))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ChainError::Rpc(format!("{status}: {body}")));
        }
        let txid = resp
            .text()
            .await
            .map_err(|e| ChainError::Network(e.to_string()))?
            .trim()
            .to_string();
        Ok(txid)
    }
}

use std::str::FromStr;
