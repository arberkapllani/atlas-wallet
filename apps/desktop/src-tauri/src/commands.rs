//! Tauri commands — the IPC surface exposed to the SvelteKit UI.
//!
//! Every secret-touching operation runs entirely inside this Rust process.
//! The frontend only sees public addresses, balances, fee quotes, and
//! signed transaction hashes.

use crate::error::{CmdError, CmdResult};
use crate::state::AppState;
use exodus2_chain_evm::networks as evm_networks;
use exodus2_chain_traits::{Amount, FeeOption, TxRequest};
use exodus2_profile::{ProfileKind, ProfileSummary, WatchAccount};
use exodus2_wallet_core::{
    derive::{derive_account, ChainKind},
    EncryptedVault, KdfParams, Mnemonic, MnemonicLength,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

// =============================================================================
// Chain catalog
// =============================================================================

#[derive(Debug, Serialize)]
pub struct ChainSummary {
    pub id: String,
    pub display_name: String,
    pub symbol: String,
    pub decimals: u8,
    pub family: String,
    pub enabled_by_default: bool,
}

#[tauri::command]
pub async fn list_chains(_state: State<'_, Arc<AppState>>) -> CmdResult<Vec<ChainSummary>> {
    let mut out = Vec::new();
    out.push(ChainSummary {
        id: "btc".into(),
        display_name: "Bitcoin".into(),
        symbol: "BTC".into(),
        decimals: 8,
        family: "bitcoin".into(),
        enabled_by_default: true,
    });
    for n in evm_networks::NETWORKS {
        out.push(ChainSummary {
            id: n.id.into(),
            display_name: n.display_name.into(),
            symbol: n.symbol.into(),
            decimals: 18,
            family: "evm".into(),
            enabled_by_default: n.enabled_by_default,
        });
    }
    Ok(out)
}

// =============================================================================
// Profile management
// =============================================================================

#[tauri::command]
pub async fn list_profiles(state: State<'_, Arc<AppState>>) -> CmdResult<Vec<ProfileSummary>> {
    let reg = state.profiles.read().await;
    Ok(reg.profiles().iter().map(ProfileSummary::from).collect())
}

#[tauri::command]
pub async fn active_profile(state: State<'_, Arc<AppState>>) -> CmdResult<Option<ProfileSummary>> {
    let reg = state.profiles.read().await;
    Ok(reg.active().map(ProfileSummary::from))
}

#[tauri::command]
pub async fn switch_profile(state: State<'_, Arc<AppState>>, id: String) -> CmdResult<()> {
    let uuid = parse_uuid(&id)?;
    {
        let mut reg = state.profiles.write().await;
        reg.set_active(uuid)?;
    }
    // Switching profiles always locks the previous in-memory seed.
    *state.mnemonic.write().await = None;
    Ok(())
}

#[tauri::command]
pub async fn rename_profile(
    state: State<'_, Arc<AppState>>,
    id: String,
    new_name: String,
) -> CmdResult<()> {
    let uuid = parse_uuid(&id)?;
    let mut reg = state.profiles.write().await;
    reg.rename(uuid, &new_name)?;
    Ok(())
}

#[tauri::command]
pub async fn delete_profile(state: State<'_, Arc<AppState>>, id: String) -> CmdResult<()> {
    let uuid = parse_uuid(&id)?;
    let was_active = {
        let reg = state.profiles.read().await;
        reg.active_id() == Some(uuid)
    };
    {
        let mut reg = state.profiles.write().await;
        reg.delete(uuid)?;
    }
    if was_active {
        *state.mnemonic.write().await = None;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct CreateWatchOnlyArgs {
    pub name: String,
    pub accounts: Vec<WatchAccount>,
}

#[tauri::command]
pub async fn create_watch_only_profile(
    state: State<'_, Arc<AppState>>,
    args: CreateWatchOnlyArgs,
) -> CmdResult<ProfileSummary> {
    let mut reg = state.profiles.write().await;
    let p = reg.create_watch_only(&args.name, args.accounts)?;
    let summary = ProfileSummary::from(p);
    // Switching profiles invalidates the unlocked seed.
    drop(reg);
    *state.mnemonic.write().await = None;
    Ok(summary)
}

// =============================================================================
// Hot-wallet lifecycle (operates on the *active* profile)
// =============================================================================

/// `true` if the registry currently has at least one hot profile.
#[tauri::command]
pub async fn vault_exists(state: State<'_, Arc<AppState>>) -> CmdResult<bool> {
    let reg = state.profiles.read().await;
    Ok(reg
        .profiles()
        .iter()
        .any(|p| matches!(p.kind, ProfileKind::Hot { .. })))
}

#[tauri::command]
pub async fn is_unlocked(state: State<'_, Arc<AppState>>) -> CmdResult<bool> {
    Ok(state.mnemonic.read().await.is_some())
}

#[derive(Debug, Deserialize)]
pub struct CreateWalletArgs {
    pub password: String,
    pub word_count: u8,
    /// Optional human-friendly profile name. Defaults to `"Default"` for the
    /// first wallet, otherwise required.
    #[serde(default)]
    pub name: Option<String>,
}

#[tauri::command]
pub async fn create_wallet(
    state: State<'_, Arc<AppState>>,
    args: CreateWalletArgs,
) -> CmdResult<String> {
    let length = match args.word_count {
        12 => MnemonicLength::Words12,
        24 => MnemonicLength::Words24,
        _ => return Err(CmdError::InvalidInput("word_count must be 12 or 24".into())),
    };
    let name = derive_profile_name(&state, args.name).await?;

    let (id, vault_path) = {
        let reg = state.profiles.read().await;
        reg.reserve_hot(&name)?
    };
    let mnemonic = Mnemonic::generate(length)?;
    let vault = EncryptedVault::encrypt(mnemonic.phrase(), &args.password, KdfParams::default())?;
    if let Some(parent) = vault_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&vault_path, vault.to_bytes())?;
    {
        let mut reg = state.profiles.write().await;
        reg.commit_hot(id, &name)?;
    }
    let phrase = mnemonic.phrase().to_string();
    *state.mnemonic.write().await = Some(Arc::new(mnemonic));
    Ok(phrase)
}

#[derive(Debug, Deserialize)]
pub struct ImportWalletArgs {
    pub password: String,
    pub phrase: String,
    #[serde(default)]
    pub passphrase: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[tauri::command]
pub async fn import_wallet(
    state: State<'_, Arc<AppState>>,
    args: ImportWalletArgs,
) -> CmdResult<()> {
    let name = derive_profile_name(&state, args.name).await?;
    let (id, vault_path) = {
        let reg = state.profiles.read().await;
        reg.reserve_hot(&name)?
    };
    let mnemonic = Mnemonic::from_phrase(&args.phrase, args.passphrase.as_deref().unwrap_or(""))?;
    let vault = EncryptedVault::encrypt(mnemonic.phrase(), &args.password, KdfParams::default())?;
    if let Some(parent) = vault_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&vault_path, vault.to_bytes())?;
    {
        let mut reg = state.profiles.write().await;
        reg.commit_hot(id, &name)?;
    }
    *state.mnemonic.write().await = Some(Arc::new(mnemonic));
    Ok(())
}

#[tauri::command]
pub async fn unlock_wallet(state: State<'_, Arc<AppState>>, password: String) -> CmdResult<()> {
    let vault_path = active_hot_vault_path(&state).await?;
    let bytes = std::fs::read(&vault_path)?;
    let vault = EncryptedVault::from_bytes(&bytes)?;
    let phrase = vault.decrypt(&password)?;
    let mnemonic = Mnemonic::from_phrase(&phrase, "")?;
    *state.mnemonic.write().await = Some(Arc::new(mnemonic));
    Ok(())
}

#[tauri::command]
pub async fn lock_wallet(state: State<'_, Arc<AppState>>) -> CmdResult<()> {
    *state.mnemonic.write().await = None;
    Ok(())
}

// =============================================================================
// Address / balance / send (works for both hot and watch-only profiles)
// =============================================================================

#[tauri::command]
pub async fn get_address(state: State<'_, Arc<AppState>>, chain_id: String) -> CmdResult<String> {
    match active_address_for_chain(&state, &chain_id).await? {
        Some(addr) => Ok(addr),
        None => Err(CmdError::InvalidInput(format!(
            "active profile has no address for chain '{chain_id}'"
        ))),
    }
}

#[tauri::command]
pub async fn get_balance(state: State<'_, Arc<AppState>>, chain_id: String) -> CmdResult<Amount> {
    let address = active_address_for_chain(&state, &chain_id)
        .await?
        .ok_or_else(|| {
            CmdError::InvalidInput(format!(
                "active profile has no address for chain '{chain_id}'"
            ))
        })?;
    let provider = state
        .chains
        .get(&chain_id)
        .ok_or_else(|| CmdError::InvalidInput(format!("unknown chain '{chain_id}'")))?;
    let balance = provider.balance(&address).await?;
    Ok(balance)
}

#[tauri::command]
pub async fn get_fee_options(
    state: State<'_, Arc<AppState>>,
    chain_id: String,
) -> CmdResult<Vec<FeeOption>> {
    let provider = state
        .chains
        .get(&chain_id)
        .ok_or_else(|| CmdError::InvalidInput(format!("unknown chain '{chain_id}'")))?;
    Ok(provider.fee_options().await?)
}

#[derive(Debug, Deserialize)]
pub struct SendNativeArgs {
    pub chain_id: String,
    pub to: String,
    pub amount: String,
    pub fee_level: String,
}

#[derive(Debug, Serialize)]
pub struct SendNativeResult {
    pub txid: String,
    pub fee: Amount,
}

#[tauri::command]
pub async fn send_native(
    state: State<'_, Arc<AppState>>,
    args: SendNativeArgs,
) -> CmdResult<SendNativeResult> {
    require_signing_capable(&state).await?;
    let mnemonic = require_mnemonic(&state).await?;
    let kind = chain_kind_for(&args.chain_id);
    let acct = derive_account(&mnemonic, kind, 0)?;
    let provider = state
        .chains
        .get(&args.chain_id)
        .ok_or_else(|| CmdError::InvalidInput(format!("unknown chain '{}'", args.chain_id)))?;
    let value: u128 = args
        .amount
        .parse()
        .map_err(|_| CmdError::InvalidInput("amount must be a base-unit integer".into()))?;
    let request = TxRequest {
        from: acct.address().to_string(),
        to: args.to,
        amount: Amount::new(value, provider.native_asset().clone()),
        fee_level: args.fee_level,
        memo: None,
    };
    let signed = provider.build_and_sign(request, acct.private_key()).await?;
    let txid = provider.broadcast(&signed).await?;
    Ok(SendNativeResult {
        txid,
        fee: signed.fee,
    })
}

#[tauri::command]
pub async fn get_prices(
    state: State<'_, Arc<AppState>>,
    ids: Vec<String>,
) -> CmdResult<serde_json::Value> {
    let id_refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
    match state.prices.prices(&id_refs).await {
        Ok(map) => Ok(serde_json::to_value(map).unwrap_or_default()),
        Err(e) => Err(CmdError::Chain(e.to_string())),
    }
}

// =============================================================================
// Internal helpers
// =============================================================================

async fn require_mnemonic(state: &AppState) -> CmdResult<Arc<Mnemonic>> {
    state.mnemonic.read().await.clone().ok_or(CmdError::Locked)
}

/// Returns an error if the active profile is watch-only.
async fn require_signing_capable(state: &AppState) -> CmdResult<()> {
    let reg = state.profiles.read().await;
    let active = reg.active().ok_or_else(|| {
        CmdError::NotInitialized("no active profile — create or import a wallet first".into())
    })?;
    if !active.kind.is_signing_capable() {
        return Err(CmdError::InvalidInput(
            "active profile is watch-only".into(),
        ));
    }
    Ok(())
}

/// Returns the on-disk vault path for the active hot profile.
async fn active_hot_vault_path(state: &AppState) -> CmdResult<std::path::PathBuf> {
    let reg = state.profiles.read().await;
    let active = reg.active().ok_or_else(|| {
        CmdError::NotInitialized("no active profile — create or import a wallet first".into())
    })?;
    match &active.kind {
        ProfileKind::Hot { vault_file } => Ok(reg.absolute_path(vault_file)),
        ProfileKind::WatchOnly { .. } => Err(CmdError::InvalidInput(
            "active profile is watch-only".into(),
        )),
    }
}

/// Returns the address the active profile uses for `chain_id`, or `None` if
/// the profile has no entry for that chain.
async fn active_address_for_chain(state: &AppState, chain_id: &str) -> CmdResult<Option<String>> {
    let active_kind = {
        let reg = state.profiles.read().await;
        let active = reg.active().ok_or_else(|| {
            CmdError::NotInitialized("no active profile — create or import a wallet first".into())
        })?;
        active.kind.clone()
    };
    match active_kind {
        ProfileKind::Hot { .. } => {
            let mnemonic = require_mnemonic(state).await?;
            let kind = chain_kind_for(chain_id);
            let acct = derive_account(&mnemonic, kind, 0)?;
            Ok(Some(acct.address().to_string()))
        }
        ProfileKind::WatchOnly { accounts } => Ok(accounts
            .into_iter()
            .find(|a| a.chain_id == chain_id)
            .map(|a| a.address)),
    }
}

fn chain_kind_for(chain_id: &str) -> ChainKind {
    if chain_id == "btc" {
        ChainKind::Bitcoin
    } else {
        ChainKind::Evm
    }
}

fn parse_uuid(s: &str) -> CmdResult<Uuid> {
    Uuid::parse_str(s).map_err(|_| CmdError::InvalidInput(format!("invalid uuid: {s}")))
}

/// Pick a default profile name (`"Default"` if there are no profiles yet,
/// otherwise require the caller to supply one).
async fn derive_profile_name(
    state: &State<'_, Arc<AppState>>,
    supplied: Option<String>,
) -> CmdResult<String> {
    if let Some(n) = supplied {
        let trimmed = n.trim().to_string();
        if trimmed.is_empty() {
            return Err(CmdError::InvalidInput(
                "profile name cannot be empty".into(),
            ));
        }
        return Ok(trimmed);
    }
    let reg = state.profiles.read().await;
    if reg.profiles().is_empty() {
        Ok("Default".into())
    } else {
        Err(CmdError::InvalidInput(
            "profile name is required when adding additional wallets".into(),
        ))
    }
}
