//! Application state held in Tauri's managed map.
//!
//! Holds the on-disk profile registry, the currently unlocked mnemonic (if
//! any), the chain-provider registry, and the price oracle.

use atlas_chain_bitcoin::BitcoinProvider;
use atlas_chain_cardano::CardanoProvider;
use atlas_chain_cosmos::CosmosProvider;
use atlas_chain_evm::{EvmProvider, NETWORKS};
use atlas_chain_solana::SolanaProvider;
use atlas_chain_traits::ChainProvider;
use atlas_chain_tron::TronProvider;
use atlas_chain_utxo::UtxoProvider;
use atlas_price_oracle::PriceOracle;
use atlas_profile::ProfileRegistry;
use atlas_settings::Settings;
use atlas_wallet_core::Mnemonic;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock as StdRwLock};
use tokio::sync::RwLock;

use crate::db::Db;

/// Built-in default endpoint for a chain id. Used both as a fallback when no
/// override is set and to surface "this is the default" in the UI.
pub fn default_endpoint(chain_id: &str) -> Option<&'static str> {
    if chain_id == "btc" {
        return Some(atlas_chain_bitcoin::DEFAULT_BASE_URL);
    }
    if chain_id == "sol" {
        return Some(atlas_chain_solana::DEFAULT_RPC);
    }
    if chain_id == "trx" {
        return Some(atlas_chain_tron::DEFAULT_RPC);
    }
    if let Some(n) = atlas_chain_cosmos::network_by_id(chain_id) {
        return Some(n.lcd_url);
    }
    if chain_id == "ada" {
        return Some(atlas_chain_cardano::DEFAULT_API);
    }
    if atlas_chain_utxo::network_by_id(chain_id).is_some() {
        return Some(atlas_chain_utxo::DEFAULT_API);
    }
    NETWORKS
        .iter()
        .find(|n| n.id == chain_id)
        .map(|n| n.rpc_url)
}

/// Every chain id Atlas knows about, ordered for stable UI listings.
pub fn all_chain_ids() -> Vec<&'static str> {
    let mut ids = vec!["btc"];
    for n in NETWORKS {
        ids.push(n.id);
    }
    ids.push("sol");
    ids.push("trx");
    for n in atlas_chain_cosmos::NETWORKS {
        ids.push(n.id);
    }
    ids.push("ada");
    for n in atlas_chain_utxo::NETWORKS {
        ids.push(n.id);
    }
    ids
}

fn build_provider(chain_id: &str, rpc_url: &str) -> Option<Arc<dyn ChainProvider>> {
    if chain_id == "btc" {
        return Some(Arc::new(atlas_chain_bitcoin::provider_with_base_url(
            rpc_url,
        )));
    }
    if chain_id == "sol" {
        return Some(Arc::new(SolanaProvider::with_rpc(rpc_url.to_string())));
    }
    if chain_id == "trx" {
        return Some(Arc::new(TronProvider::with_rpc(rpc_url.to_string())));
    }
    if let Some(n) = atlas_chain_cosmos::network_by_id(chain_id) {
        return Some(Arc::new(CosmosProvider::with_lcd(n, rpc_url.to_string())));
    }
    if chain_id == "ada" {
        return Some(Arc::new(CardanoProvider::with_api(rpc_url.to_string())));
    }
    if let Some(n) = atlas_chain_utxo::network_by_id(chain_id) {
        return Some(Arc::new(UtxoProvider::with_api(n, rpc_url.to_string())));
    }
    NETWORKS
        .iter()
        .find(|n| n.id == chain_id)
        .map(|n| Arc::new(EvmProvider::with_rpc(n, rpc_url.to_string())) as Arc<dyn ChainProvider>)
}

/// Shared registry of every supported chain provider.
///
/// The inner map is wrapped in an `RwLock` so individual providers can be
/// rebuilt at runtime when the user changes their RPC override.
pub struct ChainRegistry {
    providers: StdRwLock<HashMap<String, Arc<dyn ChainProvider>>>,
}

impl ChainRegistry {
    /// Build the registry from `settings`. Each chain uses the user's
    /// override if one is set, otherwise the built-in default.
    pub fn from_settings(settings: &Settings) -> Self {
        let mut map: HashMap<String, Arc<dyn ChainProvider>> = HashMap::new();
        for id in all_chain_ids() {
            let url = settings
                .rpc_override(id)
                .or_else(|| default_endpoint(id).map(|s| s.to_string()));
            if let Some(url) = url {
                if let Some(p) = build_provider(id, &url) {
                    map.insert(id.to_string(), p);
                }
            }
        }
        Self {
            providers: StdRwLock::new(map),
        }
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn ChainProvider>> {
        self.providers
            .read()
            .expect("chain registry poisoned")
            .get(id)
            .cloned()
    }

    /// Replace the provider for `chain_id` (e.g. after the user changes
    /// the RPC override). Returns `false` if the id is unknown.
    pub fn replace(&self, chain_id: &str, rpc_url: &str) -> bool {
        let Some(p) = build_provider(chain_id, rpc_url) else {
            return false;
        };
        self.providers
            .write()
            .expect("chain registry poisoned")
            .insert(chain_id.to_string(), p);
        true
    }

    #[allow(dead_code)]
    pub fn ids(&self) -> Vec<String> {
        self.providers
            .read()
            .expect("chain registry poisoned")
            .keys()
            .cloned()
            .collect()
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
    pub settings: Settings,
    pub db: Db,
    /// Per-tx notes & tags. Persisted as JSON in `data_dir/txnotes.json`.
    pub txnotes: RwLock<atlas_txnotes::TxNoteStore>,
    /// Address book. Persisted as JSON in `data_dir/contacts.json`.
    #[allow(dead_code)]
    pub contacts: RwLock<atlas_contacts::ContactBook>,
    /// In-memory ring-buffer of recent privacy-redacted events.
    #[allow(dead_code)]
    pub events: RwLock<atlas_eventlog::EventLog>,
    /// Per-profile spend-limit policy + state. Persisted as JSON.
    #[allow(dead_code)]
    pub spend: RwLock<SpendStore>,
}

/// Bundle of policy + observed state for the spend-limit evaluator.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpendStore {
    pub policy: atlas_spendlimits::SpendPolicy,
    pub state: atlas_spendlimits::SpendState,
}

impl AppState {
    pub async fn new(data_dir: PathBuf) -> std::io::Result<Self> {
        let registry = ProfileRegistry::load_or_init(&data_dir)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let settings =
            Settings::load_or_init(&data_dir).map_err(|e| std::io::Error::other(e.to_string()))?;
        let chains = ChainRegistry::from_settings(&settings);
        let db = Db::open(&data_dir.join("atlas.db"))
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let txnotes = load_json_or_default::<atlas_txnotes::TxNoteStore>(&data_dir, "txnotes.json");
        let contacts =
            load_json_or_default::<atlas_contacts::ContactBook>(&data_dir, "contacts.json");
        let spend = load_json_or_default::<SpendStore>(&data_dir, "spend.json");
        Ok(Self {
            data_dir,
            profiles: RwLock::new(registry),
            mnemonic: RwLock::new(None),
            chains,
            prices: PriceOracle::new(),
            settings,
            db,
            txnotes: RwLock::new(txnotes),
            contacts: RwLock::new(contacts),
            events: RwLock::new(atlas_eventlog::EventLog::default()),
            spend: RwLock::new(spend),
        })
    }
}

/// Load `<data_dir>/<file>` as JSON into `T`, falling back to `T::default()`
/// on any I/O or parse error. The host owns persistence for these tiny
/// pure-data crates so no schema lock-in.
fn load_json_or_default<T: Default + serde::de::DeserializeOwned>(
    data_dir: &std::path::Path,
    file: &str,
) -> T {
    let path = data_dir.join(file);
    match std::fs::read_to_string(&path) {
        Ok(s) if !s.trim().is_empty() => serde_json::from_str(&s).unwrap_or_default(),
        _ => T::default(),
    }
}

/// Atomically write `value` as pretty JSON to `<data_dir>/<file>`.
pub fn save_json<T: serde::Serialize>(
    data_dir: &std::path::Path,
    file: &str,
    value: &T,
) -> std::io::Result<()> {
    let path = data_dir.join(file);
    let tmp = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

// Stub helper for legacy bitcoin-providers we don't expose.
#[allow(dead_code)]
type _BitcoinProviderUnused = BitcoinProvider;
