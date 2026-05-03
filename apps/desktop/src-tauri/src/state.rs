//! Application state held in Tauri's managed map.
//!
//! Holds the encrypted vault path, an in-memory unlocked mnemonic (when the
//! user is signed in), and the cached price oracle. Locks via `tokio::Mutex`.

use exodus2_chain_bitcoin::BitcoinProvider;
use exodus2_chain_evm::{EvmProvider, NETWORKS};
use exodus2_chain_traits::ChainProvider;
use exodus2_price_oracle::PriceOracle;
use exodus2_wallet_core::Mnemonic;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Shared, read-only registry of every supported chain provider.
pub struct ChainRegistry {
    providers: HashMap<String, Arc<dyn ChainProvider>>,
}

impl ChainRegistry {
    pub fn new() -> Self {
        let mut providers: HashMap<String, Arc<dyn ChainProvider>> = HashMap::new();
        let btc = Arc::new(exodus2_chain_bitcoin::default_provider()) as Arc<dyn ChainProvider>;
        providers.insert(btc.id().to_string(), btc);
        for net in NETWORKS {
            let p = Arc::new(EvmProvider::new(net)) as Arc<dyn ChainProvider>;
            providers.insert(p.id().to_string(), p);
        }
        Self { providers }
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn ChainProvider>> {
        self.providers.get(id).cloned()
    }

    #[allow(dead_code)]
    pub fn ids(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }
}

/// Top-level shared state.
pub struct AppState {
    #[allow(dead_code)]
    pub data_dir: PathBuf,
    pub vault_path: PathBuf,
    pub mnemonic: RwLock<Option<Arc<Mnemonic>>>,
    pub chains: ChainRegistry,
    pub prices: PriceOracle,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Self {
        let vault_path = data_dir.join("vault.bin");
        Self {
            data_dir,
            vault_path,
            mnemonic: RwLock::new(None),
            chains: ChainRegistry::new(),
            prices: PriceOracle::new(),
        }
    }
}

impl Default for ChainRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// Stub helper for legacy bitcoin-providers we don't expose.
#[allow(dead_code)]
type _BitcoinProviderUnused = BitcoinProvider;
