//! Cosmos SDK chain provider.
//!
//! Watch / read-only support landing in P2.4: address validation,
//! balance + fee preview through the Cosmos LCD REST endpoint
//! (`/cosmos/bank/v1beta1/balances/{addr}` and
//! `/cosmos/base/tendermint/v1beta1/node_info`).
//!
//! Signing is intentionally deferred to P3.1 (Ledger HID) — the
//! overwhelming majority of ATOM holders sign with a hardware
//! wallet, and shipping software-signed Cosmos txs without first
//! landing the Ledger code path would invite phishing-via-clipboard
//! UX problems we'd rather not have. `build_and_sign` returns a
//! clearly-labelled [`ChainError::Other`] until then.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

use async_trait::async_trait;
use atlas_chain_traits::{
    Amount, Asset, ChainError, ChainProvider, ChainResult, FeeOption, SignedTx, TxRequest,
};
use serde::Deserialize;

/// Default Cosmos Hub LCD endpoint (public, rate-limited).
pub const DEFAULT_LCD: &str = "https://cosmos-rest.publicnode.com";

/// Static metadata describing one Cosmos SDK chain.
#[derive(Debug, Clone, Copy)]
pub struct CosmosNetwork {
    /// Atlas chain id (`atom`, `osmo`, `juno`).
    pub id: &'static str,
    /// Human-readable name.
    pub display_name: &'static str,
    /// Chain id on the network (`cosmoshub-4`, `osmosis-1`, `juno-1`).
    pub chain_id: &'static str,
    /// Bech32 prefix.
    pub bech32_hrp: &'static str,
    /// Native denom (`uatom`, `uosmo`, `ujuno`).
    pub denom: &'static str,
    /// Display ticker.
    pub symbol: &'static str,
    /// Decimals (Cosmos micro-denoms are always 6).
    pub decimals: u8,
    /// Default LCD REST endpoint.
    pub lcd_url: &'static str,
    /// Suggested gas price (in micro-denom per gas unit, x10^4).
    /// e.g. 250 -> 0.025 uatom/gas.
    pub gas_price_x10000: u64,
}

/// Built-in Cosmos networks.
pub const NETWORKS: &[CosmosNetwork] = &[
    CosmosNetwork {
        id: "atom",
        display_name: "Cosmos Hub",
        chain_id: "cosmoshub-4",
        bech32_hrp: "cosmos",
        denom: "uatom",
        symbol: "ATOM",
        decimals: 6,
        lcd_url: DEFAULT_LCD,
        gas_price_x10000: 250, // 0.025 uatom/gas
    },
    CosmosNetwork {
        id: "osmo",
        display_name: "Osmosis",
        chain_id: "osmosis-1",
        bech32_hrp: "osmo",
        denom: "uosmo",
        symbol: "OSMO",
        decimals: 6,
        lcd_url: "https://osmosis-rest.publicnode.com",
        gas_price_x10000: 250,
    },
    CosmosNetwork {
        id: "juno",
        display_name: "Juno",
        chain_id: "juno-1",
        bech32_hrp: "juno",
        denom: "ujuno",
        symbol: "JUNO",
        decimals: 6,
        lcd_url: "https://juno-rest.publicnode.com",
        gas_price_x10000: 750,
    },
];

/// Look up a network by its Atlas id.
pub fn network_by_id(id: &str) -> Option<&'static CosmosNetwork> {
    NETWORKS.iter().find(|n| n.id == id)
}

/// Cosmos SDK provider sharing one HTTP client across calls.
pub struct CosmosProvider {
    http: reqwest::Client,
    lcd_url: String,
    network: &'static CosmosNetwork,
    asset: Asset,
}

impl CosmosProvider {
    /// Build a provider for `network` using its built-in LCD URL.
    pub fn new(network: &'static CosmosNetwork) -> Self {
        Self::with_lcd(network, network.lcd_url.to_string())
    }

    /// Build a provider for `network` against a user-supplied LCD URL.
    pub fn with_lcd(network: &'static CosmosNetwork, lcd_url: String) -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent("Atlas/0.1")
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            lcd_url,
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
impl ChainProvider for CosmosProvider {
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
        match bech32::decode(address) {
            Ok((hrp, data)) => hrp.as_str() == self.network.bech32_hrp && data.len() == 20,
            Err(_) => false,
        }
    }

    async fn balance(&self, address: &str) -> ChainResult<Amount> {
        if !self.validate_address(address) {
            return Err(ChainError::InvalidAddress(address.into()));
        }
        #[derive(Deserialize)]
        struct Coin {
            denom: String,
            amount: String,
        }
        #[derive(Deserialize)]
        struct Resp {
            balances: Vec<Coin>,
        }
        let url = format!(
            "{}/cosmos/bank/v1beta1/balances/{}",
            self.lcd_url.trim_end_matches('/'),
            address
        );
        let resp: Resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| ChainError::Network(e.to_string()))?
            .json()
            .await
            .map_err(|e| ChainError::Codec(e.to_string()))?;
        let value: u128 = resp
            .balances
            .into_iter()
            .find(|c| c.denom == self.network.denom)
            .map(|c| c.amount.parse::<u128>().unwrap_or(0))
            .unwrap_or(0);
        Ok(Amount::new(value, self.asset.clone()))
    }

    async fn fee_options(&self) -> ChainResult<Vec<FeeOption>> {
        // Cosmos SDK fees = gas_limit * gas_price. For a typical MsgSend the
        // network charges roughly 100k gas; we expose three tiers by scaling
        // the gas price within the safe envelope nodes accept.
        let gas: u128 = 100_000;
        let base_price = self.network.gas_price_x10000 as u128; // /10000
        let tier = |level: &str, multiplier_x100: u128, eta: u32| FeeOption {
            level: level.into(),
            estimated_fee: Amount::new(
                (gas * base_price * multiplier_x100) / 1_000_000,
                self.asset.clone(),
            ),
            eta_seconds: eta,
            raw_hint: format!("{}", (base_price * multiplier_x100) / 100),
        };
        Ok(vec![
            tier("slow", 80, 30),
            tier("normal", 100, 12),
            tier("fast", 200, 6),
        ])
    }

    async fn build_and_sign(
        &self,
        _request: TxRequest,
        _private_key: &[u8; 32],
    ) -> ChainResult<SignedTx> {
        Err(ChainError::Other(
            "Cosmos software signing lands in P3.1 (Ledger HID); use a Ledger device once \
             hardware-wallet support ships, or import a watch-only address for now"
                .into(),
        ))
    }

    async fn broadcast(&self, _signed: &SignedTx) -> ChainResult<String> {
        Err(ChainError::Other(
            "Cosmos broadcast disabled until P3.1 Ledger HID lands".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_well_known_cosmos_address() {
        let p = CosmosProvider::new(&NETWORKS[0]);
        // Cosmos Hub community pool (well-known).
        assert!(p.validate_address("cosmos1jv65s3grqf6v6jl3dp4t6c9t9rk99cd88lyufl"));
    }

    #[test]
    fn rejects_wrong_prefix() {
        let p = CosmosProvider::new(&NETWORKS[0]); // atom expects "cosmos"
        assert!(!p.validate_address("osmo1jv65s3grqf6v6jl3dp4t6c9t9rk99cd8j7gqh3"));
        assert!(!p.validate_address(""));
        assert!(!p.validate_address("0xdeadbeef"));
    }

    #[test]
    fn validates_osmo_address_on_osmo_provider() {
        // Re-encode the well-known cosmos address with the osmo HRP so we
        // exercise prefix-aware validation without baking in an arbitrary
        // mainnet address.
        let (_, data) = bech32::decode("cosmos1jv65s3grqf6v6jl3dp4t6c9t9rk99cd88lyufl").unwrap();
        let hrp = bech32::Hrp::parse("osmo").unwrap();
        let osmo_addr = bech32::encode::<bech32::Bech32>(hrp, &data).unwrap();
        let osmo = NETWORKS.iter().find(|n| n.id == "osmo").unwrap();
        let p = CosmosProvider::new(osmo);
        assert!(p.validate_address(&osmo_addr));
        // The same address rejected on the cosmoshub provider:
        let atom_p = CosmosProvider::new(&NETWORKS[0]);
        assert!(!atom_p.validate_address(&osmo_addr));
    }

    #[test]
    fn networks_are_well_formed() {
        for n in NETWORKS {
            assert!(!n.id.is_empty());
            assert_eq!(n.decimals, 6);
            assert!(n.denom.starts_with('u'));
        }
    }
}
