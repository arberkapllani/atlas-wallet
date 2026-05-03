//! Application state held in Tauri's managed map.
//!
//! Holds the on-disk profile registry, the currently unlocked mnemonic (if
//! any), the chain-provider registry, and the price oracle.

use atlas_chain_bitcoin::BitcoinProvider;
use atlas_chain_evm::{EvmProvider, NETWORKS};
use atlas_chain_traits::ChainProvider;
use atlas_price_oracle::PriceOracle;
use atlas_profile::ProfileRegistry;
use atlas_wallet_core::Mnemonic;
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
        let btc = Arc::new(atlas_chain_bitcoin::default_provider()) as Arc<dyn ChainProvider>;
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

impl Default for ChainRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Top-level shared state.
pub struct AppState {
    #[allow(dead_code)]
    pub data_dir: PathBuf,
    /// On-disk profile registry. Wrapped in a write-lock so the IPC layer can
    /// add/remove/rename profiles while readers (UI listings) hold a snapshot.
    pub profiles: RwLock<ProfileRegistry>,
    /// Unlocked seed for the active hot profile. `None` when locked or when
    /// the active profile is watch-only.
    pub mnemonic: RwLock<Option<Arc<Mnemonic>>>,
    pub chains: ChainRegistry,
    pub prices: PriceOracle,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> std::io::Result<Self> {
        let registry = ProfileRegistry::load_or_init(&data_dir)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(Self {
            data_dir,
            profiles: RwLock::new(registry),
            mnemonic: RwLock::new(None),
            chains: ChainRegistry::new(),
            prices: PriceOracle::new(),
        })
    }
}

// Stub helper for legacy bitcoin-providers we don't expose.
#[allow(dead_code)]
type _BitcoinProviderUnused = BitcoinProvider;

