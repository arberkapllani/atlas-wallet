//! Bitcoin (mainnet) chain provider.
//!
//! Uses [mempool.space](https://mempool.space) as a default REST backend.
//! All transactions are P2WPKH (BIP-84 native SegWit, `bc1q…` addresses).

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod builder;
pub mod mempool_provider;

pub use mempool_provider::BitcoinProvider;

/// Default REST endpoint.
pub const DEFAULT_BASE_URL: &str = "https://mempool.space/api";

/// Build the default Bitcoin chain provider (mempool.space, mainnet).
pub fn default_provider() -> BitcoinProvider {
    BitcoinProvider::new(DEFAULT_BASE_URL)
}

/// Returns the standard BTC asset descriptor.
pub fn btc_asset() -> exodus2_chain_traits::Asset {
    exodus2_chain_traits::Asset {
        id: "btc".into(),
        symbol: "BTC".into(),
        decimals: 8,
        logo: None,
    }
}
