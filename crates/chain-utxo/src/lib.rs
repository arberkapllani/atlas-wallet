//! UTXO sibling chain providers — Litecoin, Dogecoin, Bitcoin Cash.
//!
//! All three are Bitcoin descendants but each has different address
//! encodings (bech32 "ltc", legacy base58, and CashAddr respectively),
//! so we ship them as a single crate parameterised by [`UtxoNetwork`]
//! rather than three nearly-identical sibling crates.
//!
//! Phase 2 ships watch-only:
//!
//! * Address validation (lightweight prefix + charset checks — a strict
//!   network rejects anything that slips through).
//! * Balance lookup via the public Blockchair REST API.
//! * Flat fee tiers.
//!
//! Software signing is deferred to P3.1 (Ledger HID), consistent with
//! [`atlas-chain-cosmos`] and [`atlas-chain-cardano`]. Most LTC/DOGE/BCH
//! held by Atlas's target users is hardware-signed anyway.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

use async_trait::async_trait;
use atlas_chain_traits::{
    Amount, Asset, ChainError, ChainProvider, ChainResult, FeeOption, SignedTx, TxRequest,
};
use serde::Deserialize;

/// Default Blockchair public REST root. Free tier rate-limit is fine for
/// per-address polling; users can override via the RPC settings.
pub const DEFAULT_API: &str = "https://api.blockchair.com";

/// Static metadata for one UTXO sibling network.
#[derive(Debug, Clone, Copy)]
pub struct UtxoNetwork {
    /// Atlas chain id (e.g. `"ltc"`).
    pub id: &'static str,
    /// Human-readable name.
    pub display_name: &'static str,
    /// Ticker symbol.
    pub symbol: &'static str,
    /// Decimal precision of the smallest unit.
    pub decimals: u8,
    /// Blockchair URL slug (`litecoin`, `dogecoin`, `bitcoin-cash`).
    pub blockchair_slug: &'static str,
    /// Typical fee per tx in the smallest unit, used as the "normal" tier.
    pub typical_fee_units: u128,
}

/// Litecoin (LTC).
pub const LTC: UtxoNetwork = UtxoNetwork {
    id: "ltc",
    display_name: "Litecoin",
    symbol: "LTC",
    decimals: 8,
    blockchair_slug: "litecoin",
    // 141 vbytes * ~10 lit/vB ≈ 1410 lit
    typical_fee_units: 1_500,
};

/// Dogecoin (DOGE). Higher per-tx fee because DOGE has no SegWit.
pub const DOGE: UtxoNetwork = UtxoNetwork {
    id: "doge",
    display_name: "Dogecoin",
    symbol: "DOGE",
    decimals: 8,
    blockchair_slug: "dogecoin",
    // DOGE network minimum is 1 DOGE per tx historically.
    typical_fee_units: 100_000_000,
};

/// Bitcoin Cash (BCH).
pub const BCH: UtxoNetwork = UtxoNetwork {
    id: "bch",
    display_name: "Bitcoin Cash",
    symbol: "BCH",
    decimals: 8,
    blockchair_slug: "bitcoin-cash",
    typical_fee_units: 250,
};

/// All UTXO sibling networks Atlas knows about.
pub const NETWORKS: &[UtxoNetwork] = &[LTC, DOGE, BCH];

/// Look up a network by its Atlas chain id.
pub fn network_by_id(id: &str) -> Option<&'static UtxoNetwork> {
    NETWORKS.iter().find(|n| n.id == id)
}

/// UTXO sibling chain provider.
pub struct UtxoProvider {
    http: reqwest::Client,
    api_url: String,
    network: &'static UtxoNetwork,
    asset: Asset,
}

impl UtxoProvider {
    /// Construct against [`DEFAULT_API`].
    pub fn new(network: &'static UtxoNetwork) -> Self {
        Self::with_api(network, DEFAULT_API.into())
    }

    /// Construct against a custom Blockchair-compatible base URL.
    pub fn with_api(network: &'static UtxoNetwork, api_url: String) -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent("Atlas/0.1")
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            api_url,
            network,
            asset: Asset {
                id: network.id.into(),
                symbol: network.symbol.into(),
                decimals: network.decimals,
                logo: None,
            },
        }
    }
}

#[async_trait]
impl ChainProvider for UtxoProvider {
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
        validate_address_for(self.network, address)
    }

    async fn balance(&self, address: &str) -> ChainResult<Amount> {
        if !validate_address_for(self.network, address) {
            return Err(ChainError::InvalidAddress(address.into()));
        }
        let url = format!(
            "{}/{}/dashboards/address/{}",
            self.api_url, self.network.blockchair_slug, address
        );
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| ChainError::Network(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(ChainError::Rpc(format!("{} on {}", resp.status(), url)));
        }
        let body: BlockchairResp = resp
            .json()
            .await
            .map_err(|e| ChainError::Codec(e.to_string()))?;
        let bal = body
            .data
            .get(address)
            .map(|e| e.address.balance)
            .unwrap_or(0);
        Ok(Amount::new(bal as u128, self.asset.clone()))
    }

    async fn fee_options(&self) -> ChainResult<Vec<FeeOption>> {
        let base = self.network.typical_fee_units;
        let asset = self.asset.clone();
        let mk = |level: &str, mult_num: u128, mult_den: u128, eta: u32| FeeOption {
            level: level.into(),
            estimated_fee: Amount::new(base * mult_num / mult_den, asset.clone()),
            eta_seconds: eta,
            raw_hint: format!("{}/{}", mult_num, mult_den),
        };
        Ok(vec![
            mk("slow", 1, 2, 60 * 60),
            mk("normal", 1, 1, 30 * 60),
            mk("fast", 2, 1, 10 * 60),
        ])
    }

    async fn build_and_sign(
        &self,
        _request: TxRequest,
        _private_key: &[u8; 32],
    ) -> ChainResult<SignedTx> {
        Err(ChainError::Other(format!(
            "{} software signing lands in P3.1 (Ledger HID) — \
             use a watch-only profile to track balances today",
            self.network.display_name
        )))
    }

    async fn broadcast(&self, _signed: &SignedTx) -> ChainResult<String> {
        Err(ChainError::Other(format!(
            "{} broadcast lands with P3.1 (Ledger HID) signing",
            self.network.display_name
        )))
    }
}

#[derive(Debug, Deserialize)]
struct BlockchairResp {
    data: std::collections::HashMap<String, BlockchairEntry>,
}

#[derive(Debug, Deserialize)]
struct BlockchairEntry {
    address: BlockchairAddress,
}

#[derive(Debug, Deserialize)]
struct BlockchairAddress {
    #[serde(default)]
    balance: u64,
}

// ---------------------------------------------------------------------------
// Address validation
// ---------------------------------------------------------------------------

const BECH32_CHARSET: &[u8] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";
const BASE58_CHARSET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

fn validate_address_for(network: &UtxoNetwork, address: &str) -> bool {
    match network.id {
        "ltc" => is_ltc_address(address),
        "doge" => is_doge_address(address),
        "bch" => is_bch_address(address),
        _ => false,
    }
}

/// Litecoin: bech32 `ltc1…` (BIP-173) **or** legacy base58check
/// (P2PKH version 0x30 → "L"/"M" prefix; P2SH 0x32/old 0x05 → "M"/"3").
fn is_ltc_address(addr: &str) -> bool {
    if let Some(rest) = addr.strip_prefix("ltc1") {
        return is_bech32_data(rest, 6, 80);
    }
    if let Some(b) = addr.as_bytes().first() {
        if matches!(*b, b'L' | b'M' | b'3') && (26..=35).contains(&addr.len()) {
            return is_base58_charset(addr);
        }
    }
    false
}

/// Dogecoin: legacy base58check only — DOGE doesn't have SegWit on
/// mainnet, so addresses are "D…" (P2PKH version 0x1E) or "A"/"9…"
/// (P2SH version 0x16, but rare).
fn is_doge_address(addr: &str) -> bool {
    if let Some(b) = addr.as_bytes().first() {
        if matches!(*b, b'D' | b'A' | b'9') && (30..=40).contains(&addr.len()) {
            return is_base58_charset(addr);
        }
    }
    false
}

/// Bitcoin Cash: CashAddr `bitcoincash:q…`/`bitcoincash:p…` (with or
/// without the prefix), or legacy base58check ("1…"/"3…").
fn is_bch_address(addr: &str) -> bool {
    let cashaddr = addr.strip_prefix("bitcoincash:").unwrap_or(addr);
    if let Some(rest) = cashaddr
        .strip_prefix('q')
        .or_else(|| cashaddr.strip_prefix('p'))
    {
        // CashAddr payload is base32 with the bech32 charset, length
        // 41 (P2PKH) or longer for token-aware variants.
        if (40..=120).contains(&cashaddr.len()) && rest.bytes().all(|b| BECH32_CHARSET.contains(&b))
        {
            return true;
        }
    }
    // Legacy base58 BCH addresses match Bitcoin's old format.
    if let Some(b) = addr.as_bytes().first() {
        if matches!(*b, b'1' | b'3') && (26..=35).contains(&addr.len()) {
            return is_base58_charset(addr);
        }
    }
    false
}

fn is_bech32_data(payload: &str, min: usize, max: usize) -> bool {
    let bytes = payload.as_bytes();
    (min..=max).contains(&bytes.len()) && bytes.iter().all(|b| BECH32_CHARSET.contains(b))
}

fn is_base58_charset(s: &str) -> bool {
    s.bytes().all(|b| BASE58_CHARSET.contains(&b))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ltc_addresses() {
        let p = UtxoProvider::new(&LTC);
        // Real LTC bech32 (Litecoin Foundation donation address-style).
        assert!(p.validate_address("ltc1qhzjptwpym9afcdjhs7jcz6fd0jma0l0rc0e5yr"));
        // Legacy "L…" P2PKH.
        assert!(p.validate_address("LhK2kQwiaAvhjWY799cZvMyYwnQAcxkarr"));
        // BTC bech32 must be rejected (wrong HRP).
        assert!(!p.validate_address("bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq"));
        // Random junk.
        assert!(!p.validate_address(""));
        assert!(!p.validate_address("not-an-address"));
    }

    #[test]
    fn doge_addresses() {
        let p = UtxoProvider::new(&DOGE);
        // Dogecoin's classic Shibe address (length 34).
        assert!(p.validate_address("DH5yaieqoZN36fDVciNyRueRGvGLR3mr7L"));
        // BTC-style "1…" must be rejected.
        assert!(!p.validate_address("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa"));
        assert!(!p.validate_address("doge1qabcdef"));
    }

    #[test]
    fn bch_addresses() {
        let p = UtxoProvider::new(&BCH);
        // CashAddr with prefix.
        assert!(p.validate_address("bitcoincash:qpm2qsznhks23z7629mms6s4cwef74vcwvy22gdx6a"));
        // CashAddr without prefix.
        assert!(p.validate_address("qpm2qsznhks23z7629mms6s4cwef74vcwvy22gdx6a"));
        // Legacy base58 (BCH originally used these).
        assert!(p.validate_address("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa"));
        // LTC bech32 must be rejected.
        assert!(!p.validate_address("ltc1qhzjptwpym9afcdjhs7jcz6fd0jma0l0rc0e5yr"));
    }

    #[test]
    fn fee_options_have_three_tiers() {
        let p = UtxoProvider::new(&LTC);
        let fees = futures_executor_block_on(p.fee_options()).unwrap();
        assert_eq!(fees.len(), 3);
        assert_eq!(fees[0].level, "slow");
        assert_eq!(fees[1].level, "normal");
        assert_eq!(fees[2].level, "fast");
        assert!(fees[0].estimated_fee.value < fees[1].estimated_fee.value);
        assert!(fees[1].estimated_fee.value < fees[2].estimated_fee.value);
    }

    #[test]
    fn network_by_id_lookup() {
        assert_eq!(network_by_id("ltc").unwrap().symbol, "LTC");
        assert_eq!(network_by_id("doge").unwrap().symbol, "DOGE");
        assert_eq!(network_by_id("bch").unwrap().symbol, "BCH");
        assert!(network_by_id("xxx").is_none());
    }

    /// Tiny synchronous runner so we don't pull in tokio just for the
    /// fee-table assertion (matches `chain-cardano`).
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
