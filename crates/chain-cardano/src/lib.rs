//! Cardano (ADA) chain provider — watch-only / read-only.
//!
//! Cardano's signing path requires Ed25519-BIP32 ("Icarus") derivation
//! plus CBOR-encoded Mary/Babbage transaction bodies, so software
//! signing is intentionally deferred to P3.1 Ledger HID — the vast
//! majority of ADA on Daedalus / Yoroi / Eternl is hardware-signed
//! anyway. P2.6 ships:
//!
//! * Bech32 address validation (`addr1…` for mainnet payment addrs).
//! * Balance lookup via the Koios public API
//!   (`POST /api/v1/address_info`), which returns lovelace integers.
//! * A flat fee table (Cardano min fee is essentially deterministic:
//!   `a + b * tx_size`, with `a = 155381` lovelace, `b = 44`).
//!
//! `build_and_sign` and `broadcast` return a clearly-labelled
//! [`ChainError::Other`] until hardware signing arrives.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

use async_trait::async_trait;
use atlas_chain_traits::{
    Amount, Asset, ChainError, ChainProvider, ChainResult, FeeOption, SignedTx, TxRequest,
};
use serde::{Deserialize, Serialize};

/// Default Koios mainnet endpoint (public, rate-limited).
pub const DEFAULT_API: &str = "https://api.koios.rest/api/v1";

/// Lovelace per ADA.
pub const LOVELACE_PER_ADA: u128 = 1_000_000;

/// Cardano provider sharing one HTTP client across calls.
pub struct CardanoProvider {
    http: reqwest::Client,
    api_url: String,
    asset: Asset,
}

impl CardanoProvider {
    /// Construct a provider pointing at [`DEFAULT_API`].
    pub fn new() -> Self {
        Self::with_api(DEFAULT_API.into())
    }

    /// Construct a provider pointing at a custom Koios-compatible URL.
    pub fn with_api(api_url: String) -> Self {
        Self {
            http: atlas_net::http_client_builder()
                .user_agent("Atlas/0.1")
                .build()
                .unwrap_or_else(|_| atlas_net::http_client()),
            api_url,
            asset: Asset {
                id: "ada".into(),
                symbol: "ADA".into(),
                decimals: 6,
                logo: None,
            },
        }
    }
}

impl Default for CardanoProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChainProvider for CardanoProvider {
    fn id(&self) -> &'static str {
        "ada"
    }
    fn display_name(&self) -> &'static str {
        "Cardano"
    }
    fn native_asset(&self) -> &Asset {
        &self.asset
    }

    fn validate_address(&self, address: &str) -> bool {
        // Cardano Shelley payment addresses are bech32-encoded but
        // routinely exceed the 90-char BIP-173 limit (~103 chars), so
        // the stock `bech32::decode` rejects them. We do a charset +
        // HRP-prefix check instead, which is tight enough for UI
        // validation: a malformed string fails here, and the network
        // rejects anything that survives this check but isn't a real
        // address.
        //
        // Mainnet Shelley payment addresses use HRP "addr"; legacy
        // Byron addresses are base58-encoded CBOR (rejected here).
        // Stake addresses ("stake1…") are also rejected since you
        // can't send funds to them.
        const BECH32_CHARSET: &[u8] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";
        let bytes = address.as_bytes();
        if !address.starts_with("addr1") || bytes.len() < 50 || bytes.len() > 200 {
            return false;
        }
        // Skip the "addr1" separator-included prefix and validate charset
        // (everything after the '1' separator must be in the bech32 set).
        let Some(sep) = address.rfind('1') else {
            return false;
        };
        let data = &bytes[sep + 1..];
        if data.len() < 6 {
            return false;
        }
        data.iter().all(|b| BECH32_CHARSET.contains(b))
    }

    async fn balance(&self, address: &str) -> ChainResult<Amount> {
        if !self.validate_address(address) {
            return Err(ChainError::InvalidAddress(address.into()));
        }
        #[derive(Serialize)]
        struct Req<'a> {
            #[serde(rename = "_addresses")]
            addresses: [&'a str; 1],
        }
        #[derive(Deserialize)]
        struct Row {
            balance: String,
        }
        let url = format!("{}/address_info", self.api_url.trim_end_matches('/'));
        let resp: Vec<Row> = self
            .http
            .post(&url)
            .json(&Req {
                addresses: [address],
            })
            .send()
            .await
            .map_err(|e| ChainError::Network(e.to_string()))?
            .json()
            .await
            .map_err(|e| ChainError::Codec(e.to_string()))?;
        let value: u128 = resp
            .first()
            .map(|r| r.balance.parse::<u128>().unwrap_or(0))
            .unwrap_or(0);
        Ok(Amount::new(value, self.asset.clone()))
    }

    async fn fee_options(&self) -> ChainResult<Vec<FeeOption>> {
        // Cardano min fee for a typical 1-input/2-output payment is
        // ~170-180k lovelace. We expose a single tier with a small
        // buffer; the UI can collapse the picker for ADA.
        let lovelace: u128 = 180_000;
        let opt = |level: &str, mult: u128, eta: u32| FeeOption {
            level: level.into(),
            estimated_fee: Amount::new((lovelace * mult) / 100, self.asset.clone()),
            eta_seconds: eta,
            raw_hint: ((lovelace * mult) / 100).to_string(),
        };
        Ok(vec![
            opt("slow", 100, 60),
            opt("normal", 110, 30),
            opt("fast", 130, 15),
        ])
    }

    async fn build_and_sign(
        &self,
        _request: TxRequest,
        _private_key: &[u8; 32],
    ) -> ChainResult<SignedTx> {
        Err(ChainError::Other(
            "Cardano signing lands with P3.1 Ledger HID; use a Ledger device once \
             hardware-wallet support ships, or import a watch-only address for now"
                .into(),
        ))
    }

    async fn broadcast(&self, _signed: &SignedTx) -> ChainResult<String> {
        Err(ChainError::Other(
            "Cardano broadcast disabled until P3.1 Ledger HID lands".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_mainnet_shelley_address() {
        let p = CardanoProvider::new();
        // IOG treasury / well-known mainnet payment address.
        assert!(p.validate_address(
            "addr1qx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3jcu5d8ps7zex2k2xt3uqxgjqnnj83ws8lhrn648jjxtwq2ytjqp"
        ));
    }

    #[test]
    fn rejects_stake_and_garbage() {
        let p = CardanoProvider::new();
        // Stake (reward) address — different HRP, not a payment target.
        assert!(!p.validate_address("stake1uyehkck0lajq8gr28t9uxnuvgcqrc6070x3k9r8048z8y5gh6ffgw"));
        assert!(!p.validate_address(""));
        assert!(!p.validate_address("0xdeadbeef"));
        assert!(!p.validate_address("DdzFFzCqrhsoarXqLakhRYvL7QLNNmsZbZ5GBUWxpYi1U6E1JMxdPdkqu"));
    }

    #[test]
    fn lovelace_constant() {
        assert_eq!(LOVELACE_PER_ADA, 1_000_000);
    }

    #[test]
    fn fee_options_three_tiers() {
        let p = CardanoProvider::new();
        let fees = futures_executor_block_on(p.fee_options()).unwrap();
        assert_eq!(fees.len(), 3);
        assert!(fees[2].estimated_fee.value > fees[0].estimated_fee.value);
    }

    /// Tiny synchronous test runner so we don't pull in a full async
    /// runtime just for one fee-table assertion.
    fn futures_executor_block_on<F: std::future::Future>(f: F) -> F::Output {
        use std::pin::pin;
        use std::task::{Context, Poll, Waker};
        let mut f = pin!(f);
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        loop {
            if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }
}
