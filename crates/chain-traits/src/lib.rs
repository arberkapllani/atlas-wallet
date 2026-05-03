//! Cross-chain abstractions used by every concrete chain provider.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod types;
pub use types::*;

use async_trait::async_trait;

/// A unified, asynchronous façade implemented by every supported chain
/// (Bitcoin, EVM L1/L2s, Solana, Cosmos, …).
///
/// Implementations are network-bound: they fetch state, build transactions,
/// sign with a caller-supplied private key, and broadcast.
#[async_trait]
pub trait ChainProvider: Send + Sync {
    /// Stable identifier (e.g. `"btc"`, `"eth"`, `"polygon"`).
    fn id(&self) -> &'static str;

    /// Human-readable display name (e.g. `"Bitcoin"`, `"Ethereum"`).
    fn display_name(&self) -> &'static str;

    /// Native asset metadata (BTC, ETH, MATIC, …).
    fn native_asset(&self) -> &Asset;

    /// Validate that `address` is well-formed for this chain.
    fn validate_address(&self, address: &str) -> bool;

    /// Native-asset balance for the given address.
    async fn balance(&self, address: &str) -> ChainResult<Amount>;

    /// Suggested fee tiers (slow / normal / fast).
    async fn fee_options(&self) -> ChainResult<Vec<FeeOption>>;

    /// Build *and sign* a transaction. The implementation performs everything
    /// needed (UTXO selection / nonce lookup / fee estimation) and returns
    /// raw bytes ready for broadcast.
    async fn build_and_sign(
        &self,
        request: TxRequest,
        private_key: &[u8; 32],
    ) -> ChainResult<SignedTx>;

    /// Broadcast a previously-signed transaction. Returns the chain-native txid/hash.
    async fn broadcast(&self, signed: &SignedTx) -> ChainResult<String>;
}
