//! Tauri commands — the IPC surface exposed to the SvelteKit UI.
//!
//! Every secret-touching operation runs entirely inside this Rust process.
//! The frontend only sees public addresses, balances, fee quotes, and
//! signed transaction hashes.

use crate::error::{CmdError, CmdResult};
use crate::state::AppState;
use exodus2_chain_evm::networks as evm_networks;
use exodus2_chain_traits::{Amount, FeeOption, TxRequest};
use exodus2_wallet_core::{
    derive::{derive_account, ChainKind},
    EncryptedVault, KdfParams, Mnemonic, MnemonicLength,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

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
pub async fn vault_exists(state: State<'_, Arc<AppState>>) -> CmdResult<bool> {
    Ok(state.vault_path.exists())
}

#[tauri::command]
pub async fn is_unlocked(state: State<'_, Arc<AppState>>) -> CmdResult<bool> {
    let m = state.mnemonic.read().await;
    Ok(m.is_some())
}

#[tauri::command]
pub async fn create_wallet(
    state: State<'_, Arc<AppState>>,
    password: String,
    word_count: u8,
) -> CmdResult<String> {
    if state.vault_path.exists() {
        return Err(CmdError::InvalidInput("vault already exists".into()));
    }
    let length = match word_count {
        12 => MnemonicLength::Words12,
        24 => MnemonicLength::Words24,
        _ => return Err(CmdError::InvalidInput("word_count must be 12 or 24".into())),
    };
    let mnemonic = Mnemonic::generate(length)?;
    let vault = EncryptedVault::encrypt(mnemonic.phrase(), &password, KdfParams::default())?;
    std::fs::write(&state.vault_path, vault.to_bytes())?;
    let phrase = mnemonic.phrase().to_string();
    *state.mnemonic.write().await = Some(Arc::new(mnemonic));
    Ok(phrase)
}

#[tauri::command]
pub async fn import_wallet(
    state: State<'_, Arc<AppState>>,
    password: String,
    phrase: String,
    passphrase: Option<String>,
) -> CmdResult<()> {
    if state.vault_path.exists() {
        return Err(CmdError::InvalidInput("vault already exists".into()));
    }
    let mnemonic = Mnemonic::from_phrase(&phrase, passphrase.as_deref().unwrap_or(""))?;
    let vault = EncryptedVault::encrypt(mnemonic.phrase(), &password, KdfParams::default())?;
    std::fs::write(&state.vault_path, vault.to_bytes())?;
    *state.mnemonic.write().await = Some(Arc::new(mnemonic));
    Ok(())
}

#[tauri::command]
pub async fn unlock_wallet(state: State<'_, Arc<AppState>>, password: String) -> CmdResult<()> {
    if !state.vault_path.exists() {
        return Err(CmdError::NotInitialized("no vault found".into()));
    }
    let bytes = std::fs::read(&state.vault_path)?;
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

#[tauri::command]
pub async fn list_chains(state: State<'_, Arc<AppState>>) -> CmdResult<Vec<ChainSummary>> {
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
    let _ = state;
    Ok(out)
}

async fn require_mnemonic(state: &AppState) -> CmdResult<Arc<Mnemonic>> {
    state.mnemonic.read().await.clone().ok_or(CmdError::Locked)
}

fn chain_kind_for(chain_id: &str) -> ChainKind {
    if chain_id == "btc" {
        ChainKind::Bitcoin
    } else {
        ChainKind::Evm
    }
}

#[tauri::command]
pub async fn get_address(state: State<'_, Arc<AppState>>, chain_id: String) -> CmdResult<String> {
    let mnemonic = require_mnemonic(&state).await?;
    let kind = chain_kind_for(&chain_id);
    let acct = derive_account(&mnemonic, kind, 0)?;
    Ok(acct.address().to_string())
}

#[tauri::command]
pub async fn get_balance(state: State<'_, Arc<AppState>>, chain_id: String) -> CmdResult<Amount> {
    let mnemonic = require_mnemonic(&state).await?;
    let kind = chain_kind_for(&chain_id);
    let acct = derive_account(&mnemonic, kind, 0)?;
    let provider = state
        .chains
        .get(&chain_id)
        .ok_or_else(|| CmdError::InvalidInput(format!("unknown chain '{chain_id}'")))?;
    let balance = provider.balance(acct.address()).await?;
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
