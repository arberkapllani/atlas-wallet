//! Tauri commands — the IPC surface exposed to the SvelteKit UI.
//!
//! Every secret-touching operation runs entirely inside this Rust process.
//! The frontend only sees public addresses, balances, fee quotes, and
//! signed transaction hashes.

use crate::db::TxRecord;
use crate::error::{CmdError, CmdResult};
use crate::events::{TxRecordedEvent, TxStatusChangedEvent, WalletLockedEvent};
use crate::state::AppState;
use atlas_chain_evm::networks as evm_networks;
use atlas_chain_traits::{Amount, ChainProvider, FeeOption, TxRequest};
use atlas_profile::{ProfileKind, ProfileSummary, WatchAccount};
use atlas_wallet_core::{
    derive::{derive_account, ChainKind},
    EncryptedVault, KdfParams, Mnemonic, MnemonicLength,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, State};
use tauri_specta::Event as _;
use uuid::Uuid;

// =============================================================================
// Chain catalog
// =============================================================================

#[derive(Debug, Serialize, specta::Type)]
pub struct ChainSummary {
    pub id: String,
    pub display_name: String,
    pub symbol: String,
    pub decimals: u8,
    pub family: String,
    pub enabled_by_default: bool,
}

#[tauri::command]
#[specta::specta]
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
    out.push(ChainSummary {
        id: "sol".into(),
        display_name: "Solana".into(),
        symbol: "SOL".into(),
        decimals: 9,
        family: "solana".into(),
        enabled_by_default: true,
    });
    out.push(ChainSummary {
        id: "trx".into(),
        display_name: "Tron".into(),
        symbol: "TRX".into(),
        decimals: 6,
        family: "tron".into(),
        enabled_by_default: true,
    });
    for n in atlas_chain_cosmos::NETWORKS {
        out.push(ChainSummary {
            id: n.id.into(),
            display_name: n.display_name.into(),
            symbol: n.symbol.into(),
            decimals: n.decimals,
            family: "cosmos".into(),
            // Only ATOM ships enabled-by-default; OSMO/JUNO are opt-in.
            enabled_by_default: n.id == "atom",
        });
    }
    out.push(ChainSummary {
        id: "ada".into(),
        display_name: "Cardano".into(),
        symbol: "ADA".into(),
        decimals: 6,
        family: "cardano".into(),
        // Watch-only until Ledger HID lands; off by default to avoid
        // surfacing an account the user can't actually send from yet.
        enabled_by_default: false,
    });
    for n in atlas_chain_utxo::NETWORKS {
        out.push(ChainSummary {
            id: n.id.into(),
            display_name: n.display_name.into(),
            symbol: n.symbol.into(),
            decimals: n.decimals,
            family: "utxo".into(),
            // Watch-only until P3.1 Ledger HID — off by default.
            enabled_by_default: false,
        });
    }
    Ok(out)
}

// =============================================================================
// Token registry
// =============================================================================

#[derive(Debug, Serialize, specta::Type)]
pub struct TokenSummary {
    pub id: String,
    pub symbol: String,
    pub display_name: String,
    pub chain_id: String,
    pub contract: String,
    pub decimals: u8,
    pub standard: String,
    pub enabled_by_default: bool,
}

#[tauri::command]
#[specta::specta]
pub async fn list_tokens(_state: State<'_, Arc<AppState>>) -> CmdResult<Vec<TokenSummary>> {
    Ok(atlas_token_registry::TOKENS
        .iter()
        .map(|t| TokenSummary {
            id: t.id.into(),
            symbol: t.symbol.into(),
            display_name: t.display_name.into(),
            chain_id: t.chain_id.into(),
            contract: t.contract.into(),
            decimals: t.decimals,
            standard: match t.standard {
                atlas_token_registry::TokenStandard::Erc20 => "erc20".into(),
                atlas_token_registry::TokenStandard::Trc20 => "trc20".into(),
            },
            enabled_by_default: t.enabled_by_default,
        })
        .collect())
}

/// Curated token-list URLs Atlas ships with by default. The frontend
/// uses this to populate the "Refresh from tokenlists.org" picker.
#[tauri::command]
#[specta::specta]
pub async fn token_list_default_urls() -> CmdResult<Vec<String>> {
    Ok(atlas_token_registry::DEFAULT_TOKEN_LIST_URLS
        .iter()
        .map(|s| (*s).to_string())
        .collect())
}

/// Fetch a Uniswap-format token list from `url` and return the parsed
/// tokens for any chain Atlas supports. Network errors and invalid JSON
/// surface as [`CmdError::InvalidInput`].
#[tauri::command]
#[specta::specta]
pub async fn token_list_fetch(url: String) -> CmdResult<Vec<atlas_token_registry::OwnedTokenMeta>> {
    atlas_token_registry::fetch_token_list(&url)
        .await
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Add a custom user-imported token. Validates contract format (`0x` +
/// 40 hex chars for EVM, base58 for Tron) before persisting so the UI
/// can't poison the local DB with garbage.
#[tauri::command]
#[specta::specta]
pub async fn token_add_custom(
    state: State<'_, Arc<AppState>>,
    chain_id: String,
    contract: String,
    symbol: String,
    display_name: String,
    decimals: u8,
    logo_uri: Option<String>,
) -> CmdResult<()> {
    let trimmed = contract.trim().to_string();
    let standard = if chain_id == "trx" {
        if !is_valid_tron_address(&trimmed) {
            return Err(CmdError::InvalidInput(format!(
                "invalid TRC-20 contract address: {trimmed}"
            )));
        }
        "trc-20"
    } else if atlas_chain_evm::networks::by_id(&chain_id).is_some() {
        if !is_valid_evm_address(&trimmed) {
            return Err(CmdError::InvalidInput(format!(
                "invalid EVM contract address: {trimmed}"
            )));
        }
        "erc-20"
    } else {
        return Err(CmdError::InvalidInput(format!(
            "custom tokens not supported on chain {chain_id}"
        )));
    };
    if symbol.trim().is_empty() || display_name.trim().is_empty() {
        return Err(CmdError::InvalidInput(
            "symbol and display_name are required".into(),
        ));
    }
    if decimals > 38 {
        return Err(CmdError::InvalidInput("decimals must be at most 38".into()));
    }
    state
        .db
        .add_custom_token(&crate::db::CustomToken {
            chain_id,
            contract: trimmed,
            symbol: symbol.trim().into(),
            display_name: display_name.trim().into(),
            decimals,
            standard: standard.into(),
            logo_uri,
        })
        .await
        .map_err(|e| CmdError::Io(e.to_string()))?;
    Ok(())
}

/// List custom tokens for `chain_id` (or all chains when empty).
#[tauri::command]
#[specta::specta]
pub async fn token_list_custom(
    state: State<'_, Arc<AppState>>,
    chain_id: String,
) -> CmdResult<Vec<crate::db::CustomToken>> {
    state
        .db
        .list_custom_tokens(&chain_id)
        .await
        .map_err(|e| CmdError::Io(e.to_string()))
}

/// Remove one custom token. Returns `true` if a row was deleted.
#[tauri::command]
#[specta::specta]
pub async fn token_remove_custom(
    state: State<'_, Arc<AppState>>,
    chain_id: String,
    contract: String,
) -> CmdResult<bool> {
    let n = state
        .db
        .remove_custom_token(&chain_id, &contract)
        .await
        .map_err(|e| CmdError::Io(e.to_string()))?;
    Ok(n > 0)
}

fn is_valid_evm_address(addr: &str) -> bool {
    let Some(rest) = addr.strip_prefix("0x").or_else(|| addr.strip_prefix("0X")) else {
        return false;
    };
    rest.len() == 40 && rest.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_valid_tron_address(addr: &str) -> bool {
    // TRC-20 addresses are base58check-encoded with version byte 0x41
    // — the encoded form is always 34 characters starting with 'T'.
    addr.len() == 34
        && addr.starts_with('T')
        && addr.bytes().all(|b| {
            matches!(b,
                b'1'..=b'9' | b'A'..=b'H' | b'J'..=b'N' | b'P'..=b'Z' | b'a'..=b'k' | b'm'..=b'z')
        })
}

// =============================================================================
// Profile management
// =============================================================================

#[tauri::command]
#[specta::specta]
pub async fn list_profiles(state: State<'_, Arc<AppState>>) -> CmdResult<Vec<ProfileSummary>> {
    let reg = state.profiles.read().await;
    Ok(reg.profiles().iter().map(ProfileSummary::from).collect())
}

#[tauri::command]
#[specta::specta]
pub async fn active_profile(state: State<'_, Arc<AppState>>) -> CmdResult<Option<ProfileSummary>> {
    let reg = state.profiles.read().await;
    Ok(reg.active().map(ProfileSummary::from))
}

#[tauri::command]
#[specta::specta]
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
#[specta::specta]
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
#[specta::specta]
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

#[derive(Debug, Deserialize, specta::Type)]
pub struct CreateWatchOnlyArgs {
    pub name: String,
    pub accounts: Vec<WatchAccount>,
}

#[tauri::command]
#[specta::specta]
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
#[specta::specta]
pub async fn vault_exists(state: State<'_, Arc<AppState>>) -> CmdResult<bool> {
    let reg = state.profiles.read().await;
    Ok(reg
        .profiles()
        .iter()
        .any(|p| matches!(p.kind, ProfileKind::Hot { .. })))
}

#[tauri::command]
#[specta::specta]
pub async fn is_unlocked(state: State<'_, Arc<AppState>>) -> CmdResult<bool> {
    Ok(state.mnemonic.read().await.is_some())
}

#[derive(Debug, Deserialize, specta::Type)]
pub struct CreateWalletArgs {
    pub password: String,
    pub word_count: u8,
    /// Optional human-friendly profile name. Defaults to `"Default"` for the
    /// first wallet, otherwise required.
    #[serde(default)]
    pub name: Option<String>,
}

#[tauri::command]
#[specta::specta]
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

#[derive(Debug, Deserialize, specta::Type)]
pub struct ImportWalletArgs {
    pub password: String,
    pub phrase: String,
    #[serde(default)]
    pub passphrase: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[tauri::command]
#[specta::specta]
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
#[specta::specta]
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
#[specta::specta]
pub async fn lock_wallet(app: AppHandle, state: State<'_, Arc<AppState>>) -> CmdResult<()> {
    *state.mnemonic.write().await = None;
    let _ = WalletLockedEvent {
        reason: "manual".into(),
    }
    .emit(&app);
    Ok(())
}

// =============================================================================
// Address / balance / send (works for both hot and watch-only profiles)
// =============================================================================

#[tauri::command]
#[specta::specta]
pub async fn get_address(state: State<'_, Arc<AppState>>, chain_id: String) -> CmdResult<String> {
    match active_address_for_chain(&state, &chain_id).await? {
        Some(addr) => Ok(addr),
        None => Err(CmdError::InvalidInput(format!(
            "active profile has no address for chain '{chain_id}'"
        ))),
    }
}

#[tauri::command]
#[specta::specta]
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
#[specta::specta]
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

#[derive(Debug, Deserialize, specta::Type)]
pub struct SendNativeArgs {
    pub chain_id: String,
    pub to: String,
    pub amount: String,
    pub fee_level: String,
}

#[derive(Debug, Serialize, specta::Type)]
pub struct SendNativeResult {
    pub txid: String,
    pub fee: Amount,
}

#[tauri::command]
#[specta::specta]
pub async fn send_native(
    app: AppHandle,
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
        to: args.to.clone(),
        amount: Amount::new(value, provider.native_asset().clone()),
        fee_level: args.fee_level,
        memo: None,
    };
    let signed = provider.build_and_sign(request, acct.private_key()).await?;
    let txid = provider.broadcast(&signed).await?;
    record_outgoing_tx(
        &app,
        &state,
        &args.chain_id,
        &txid,
        &args.to,
        value.to_string(),
        provider.native_asset().symbol.clone(),
        signed.fee.value.to_string(),
    )
    .await;
    Ok(SendNativeResult {
        txid,
        fee: signed.fee,
    })
}

#[derive(Debug, Deserialize, specta::Type)]
pub struct SendTokenArgs {
    /// Token id from the registry (e.g. `usdt-erc20`).
    pub token_id: String,
    pub to: String,
    /// Token amount in base units (string-encoded u128).
    pub amount: String,
    pub fee_level: String,
}

#[derive(Debug, Serialize, specta::Type)]
pub struct SendTokenResult {
    pub txid: String,
    /// Network fee paid in the chain's native asset.
    pub fee: Amount,
}

/// Sign and broadcast a token transfer (ERC-20 or TRC-20).
#[tauri::command]
#[specta::specta]
pub async fn send_token(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    args: SendTokenArgs,
) -> CmdResult<SendTokenResult> {
    use atlas_token_registry::{by_id, TokenStandard};
    require_signing_capable(&state).await?;
    let token = by_id(&args.token_id)
        .ok_or_else(|| CmdError::InvalidInput(format!("unknown token '{}'", args.token_id)))?;
    let mnemonic = require_mnemonic(&state).await?;
    let kind = chain_kind_for(token.chain_id);
    let acct = derive_account(&mnemonic, kind, 0)?;
    let value: u128 = args
        .amount
        .parse()
        .map_err(|_| CmdError::InvalidInput("amount must be a base-unit integer".into()))?;

    match token.standard {
        TokenStandard::Erc20 => {
            // We need the EVM-typed provider for token transfers. ChainRegistry
            // hands us a trait object, so look up the network metadata directly
            // and build a transient provider sharing the user's RPC override.
            let network = evm_networks::NETWORKS
                .iter()
                .find(|n| n.id == token.chain_id)
                .ok_or_else(|| {
                    CmdError::InvalidInput(format!("unknown chain '{}'", token.chain_id))
                })?;
            let rpc_url = state
                .settings
                .rpc_override(token.chain_id)
                .unwrap_or_else(|| network.rpc_url.to_string());
            let provider = atlas_chain_evm::EvmProvider::with_rpc(network, rpc_url);

            let signed = provider
                .send_token(
                    acct.address(),
                    &args.to,
                    token.contract,
                    value,
                    &args.fee_level,
                    acct.private_key(),
                )
                .await?;
            let txid = provider.broadcast(&signed).await?;
            record_outgoing_tx(
                &app,
                &state,
                token.chain_id,
                &txid,
                &args.to,
                value.to_string(),
                token.symbol.into(),
                signed.fee.value.to_string(),
            )
            .await;
            Ok(SendTokenResult {
                txid,
                fee: signed.fee,
            })
        }
        TokenStandard::Trc20 => {
            let rpc_url = state
                .settings
                .rpc_override(token.chain_id)
                .unwrap_or_else(|| atlas_chain_tron::DEFAULT_RPC.to_string());
            let provider = atlas_chain_tron::TronProvider::with_rpc(rpc_url);
            let signed = provider
                .build_and_sign_trc20(
                    acct.address(),
                    &args.to,
                    token.contract,
                    value,
                    acct.private_key(),
                )
                .await?;
            // Broadcast goes through the trait-level method.
            use atlas_chain_traits::ChainProvider;
            let txid = provider.broadcast(&signed).await?;
            record_outgoing_tx(
                &app,
                &state,
                token.chain_id,
                &txid,
                &args.to,
                value.to_string(),
                token.symbol.into(),
                signed.fee.value.to_string(),
            )
            .await;
            Ok(SendTokenResult {
                txid,
                fee: signed.fee,
            })
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_prices(
    state: State<'_, Arc<AppState>>,
    ids: Vec<String>,
    currency: Option<String>,
) -> CmdResult<std::collections::HashMap<String, atlas_price_oracle::PricePoint>> {
    let id_refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
    let vs = currency
        .as_deref()
        .and_then(atlas_settings::FiatCurrency::parse)
        .unwrap_or_else(|| state.settings.fiat_currency())
        .as_str();
    state
        .prices
        .prices_in(&id_refs, vs)
        .await
        .map_err(|e| CmdError::Chain(e.to_string()))
}

/// Currently persisted display currency (`"usd"` / `"eur"` / `"gbp"`).
#[tauri::command]
#[specta::specta]
pub async fn get_fiat_currency(state: State<'_, Arc<AppState>>) -> CmdResult<String> {
    Ok(state.settings.fiat_currency().as_str().to_string())
}

/// Persist a new display currency. Accepts `"usd"`, `"eur"`, or `"gbp"`.
#[tauri::command]
#[specta::specta]
pub async fn set_fiat_currency(
    state: State<'_, Arc<AppState>>,
    currency: String,
) -> CmdResult<String> {
    let parsed = atlas_settings::FiatCurrency::parse(&currency).ok_or_else(|| {
        CmdError::InvalidInput(format!("unknown currency '{currency}' (use usd/eur/gbp)"))
    })?;
    state
        .settings
        .set_fiat_currency(parsed)
        .map_err(|e| CmdError::Io(e.to_string()))?;
    Ok(parsed.as_str().to_string())
}

/// Currently configured auto-lock timeout (minutes). `0` means disabled.
#[tauri::command]
#[specta::specta]
pub async fn get_auto_lock_minutes(state: State<'_, Arc<AppState>>) -> CmdResult<u32> {
    Ok(state.settings.auto_lock_minutes())
}

/// Persist a new auto-lock timeout. `0` disables auto-lock entirely.
/// Values above 1440 (24h) are clamped to discourage forever-unlocked sessions.
#[tauri::command]
#[specta::specta]
pub async fn set_auto_lock_minutes(
    state: State<'_, Arc<AppState>>,
    minutes: u32,
) -> CmdResult<u32> {
    let clamped = minutes.min(1440);
    state
        .settings
        .set_auto_lock_minutes(clamped)
        .map_err(|e| CmdError::Io(e.to_string()))?;
    Ok(clamped)
}

/// Probe a single chain's effective endpoint and return latency / status.
#[tauri::command]
#[specta::specta]
pub async fn network_health(
    state: State<'_, Arc<AppState>>,
    chain_id: String,
) -> CmdResult<crate::network_health::NetworkHealth> {
    Ok(crate::network_health::measure(&state, &chain_id).await)
}

/// Probe every supported chain in parallel.
#[tauri::command]
#[specta::specta]
pub async fn network_health_all(
    state: State<'_, Arc<AppState>>,
) -> CmdResult<Vec<crate::network_health::NetworkHealth>> {
    use tokio::task::JoinSet;
    let ids = crate::state::all_chain_ids();
    let mut set = JoinSet::new();
    for id in ids {
        let st = state.inner().clone();
        let chain_id = id.to_string();
        set.spawn(async move { crate::network_health::measure(&st, &chain_id).await });
    }
    let mut out = Vec::new();
    while let Some(res) = set.join_next().await {
        if let Ok(h) = res {
            out.push(h);
        }
    }
    // Stable order matching all_chain_ids() for predictable UI rendering.
    let order = crate::state::all_chain_ids();
    out.sort_by_key(|h| {
        order
            .iter()
            .position(|id| *id == h.chain_id)
            .unwrap_or(usize::MAX)
    });
    Ok(out)
}

// =============================================================================
// Exchange — 1inch v6 aggregator
// =============================================================================

#[derive(Debug, Serialize, specta::Type)]
pub struct ExchangeSettings {
    pub api_key_set: bool,
    pub base_url: String,
}

/// Inspect (without revealing) the user's 1inch configuration.
#[tauri::command]
#[specta::specta]
pub async fn get_exchange_settings(state: State<'_, Arc<AppState>>) -> CmdResult<ExchangeSettings> {
    Ok(ExchangeSettings {
        api_key_set: state.settings.oneinch_api_key().is_some(),
        base_url: state.settings.oneinch_base_url(),
    })
}

#[derive(Debug, Deserialize, specta::Type)]
pub struct ExchangeConfigArgs {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

/// Persist 1inch credentials. Pass `null` (or empty) to clear a field.
#[tauri::command]
#[specta::specta]
pub async fn set_exchange_settings(
    state: State<'_, Arc<AppState>>,
    args: ExchangeConfigArgs,
) -> CmdResult<ExchangeSettings> {
    state
        .settings
        .set_oneinch_api_key(args.api_key.as_deref())
        .map_err(|e| CmdError::Io(e.to_string()))?;
    state
        .settings
        .set_oneinch_base_url(args.base_url.as_deref())
        .map_err(|e| CmdError::Io(e.to_string()))?;
    Ok(ExchangeSettings {
        api_key_set: state.settings.oneinch_api_key().is_some(),
        base_url: state.settings.oneinch_base_url(),
    })
}

/// Quote an EVM swap via 1inch v6.
///
/// `chain_id` is the EIP-155 chain id (1 = mainnet, 137 = polygon, …).
/// Token addresses use 1inch's convention: the native asset is
/// `0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee`.
#[tauri::command]
#[specta::specta]
pub async fn exchange_quote(
    state: State<'_, Arc<AppState>>,
    chain_id: u64,
    src: String,
    dst: String,
    amount: String,
) -> CmdResult<atlas_exchange_1inch::Quote> {
    let client = atlas_exchange_1inch::OneInchClient::new(
        state.settings.oneinch_base_url(),
        state.settings.oneinch_api_key(),
    );
    client
        .quote(chain_id, &src, &dst, &amount)
        .await
        .map_err(|e| CmdError::Chain(e.to_string()))
}

/// 1inch's sentinel address for the native asset (ETH/MATIC/BNB/…).
const NATIVE_SENTINEL: &str = "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

#[derive(Debug, Serialize, specta::Type)]
pub struct ExchangeSwapResult {
    /// Final swap transaction hash.
    pub txid: String,
    /// If a pre-approve was needed (ERC-20 source), this is its tx hash.
    pub approve_txid: Option<String>,
    /// Estimated destination amount, base units, decimal string.
    pub to_amount: String,
}

#[derive(Debug, Deserialize, specta::Type)]
pub struct ExchangeSwapArgs {
    pub chain_id: u64,
    pub src: String,
    pub dst: String,
    /// Source amount, base units, decimal string.
    pub amount: String,
    /// Maximum slippage in basis points (100 = 1%).
    pub slippage_bps: u16,
    /// `slow|normal|fast` or a raw decimal wei gas-price.
    pub fee_level: String,
}

/// Execute an EVM swap via 1inch v6.
///
/// For ERC-20 source tokens this command also handles the on-chain
/// allowance: if the router's allowance is below the requested amount,
/// Atlas signs and broadcasts an `approve` transaction first, then the
/// swap. Both transactions use the wallet's pending nonce — the swap is
/// expected to land in the block immediately after the approval.
#[tauri::command]
#[specta::specta]
pub async fn exchange_swap(
    state: State<'_, Arc<AppState>>,
    args: ExchangeSwapArgs,
) -> CmdResult<ExchangeSwapResult> {
    require_signing_capable(&state).await?;

    // Resolve the EVM network from the EIP-155 chain id.
    let network = evm_networks::NETWORKS
        .iter()
        .find(|n| n.chain_id == args.chain_id)
        .ok_or_else(|| {
            CmdError::InvalidInput(format!("unsupported EVM chain id {}", args.chain_id))
        })?;

    let mnemonic = require_mnemonic(&state).await?;
    let acct = derive_account(&mnemonic, ChainKind::Evm, 0)?;
    let from_addr = acct.address().to_string();

    let amount_u128: u128 = args
        .amount
        .parse()
        .map_err(|_| CmdError::InvalidInput("amount must be a base-unit integer".into()))?;

    let rpc_url = state
        .settings
        .rpc_override(network.id)
        .unwrap_or_else(|| network.rpc_url.to_string());
    let provider = atlas_chain_evm::EvmProvider::with_rpc(network, rpc_url);

    // Fetch the swap transaction from 1inch.
    let client = atlas_exchange_1inch::OneInchClient::new(
        state.settings.oneinch_base_url(),
        state.settings.oneinch_api_key(),
    );
    let swap_tx = client
        .swap(
            args.chain_id,
            &args.src,
            &args.dst,
            &args.amount,
            &from_addr,
            args.slippage_bps,
        )
        .await
        .map_err(|e| CmdError::Chain(e.to_string()))?;

    // For ERC-20 sources, ensure the router has enough allowance.
    let mut approve_txid: Option<String> = None;
    let is_native_src = args.src.eq_ignore_ascii_case(NATIVE_SENTINEL);
    if !is_native_src {
        let current = provider
            .token_allowance(&from_addr, &swap_tx.to, &args.src)
            .await?;
        if current < amount_u128 {
            // Approve the exact amount being swapped — keeps the user's
            // attack surface small (no infinite approvals).
            let spender = parse_hex_addr_or_err(&swap_tx.to)?;
            let calldata = atlas_chain_evm::erc20::approve_calldata(&spender, amount_u128);
            let signed = provider
                .send_call(
                    &from_addr,
                    &args.src,
                    0,
                    calldata,
                    60_000,
                    &args.fee_level,
                    acct.private_key(),
                )
                .await?;
            let txid = provider.broadcast(&signed).await?;
            approve_txid = Some(txid);
        }
    }

    // Submit the swap itself.
    let value_u128: u128 = swap_tx
        .value
        .parse()
        .map_err(|_| CmdError::Chain("1inch returned a non-integer tx.value".into()))?;
    let data_bytes = decode_hex_payload(&swap_tx.data)?;
    let gas_limit = if swap_tx.gas == 0 {
        500_000
    } else {
        swap_tx.gas + swap_tx.gas / 5 // 20 % headroom over 1inch's estimate
    };

    let signed = provider
        .send_call(
            &from_addr,
            &swap_tx.to,
            value_u128,
            data_bytes,
            gas_limit,
            &args.fee_level,
            acct.private_key(),
        )
        .await?;
    let txid = provider.broadcast(&signed).await?;

    Ok(ExchangeSwapResult {
        txid,
        approve_txid,
        to_amount: swap_tx.to_amount,
    })
}

fn parse_hex_addr_or_err(addr: &str) -> CmdResult<[u8; 20]> {
    let s = addr.strip_prefix("0x").unwrap_or(addr);
    if s.len() != 40 {
        return Err(CmdError::InvalidInput(format!("bad address: {addr}")));
    }
    let mut out = [0u8; 20];
    hex::decode_to_slice(s, &mut out)
        .map_err(|_| CmdError::InvalidInput(format!("bad address: {addr}")))?;
    Ok(out)
}

fn decode_hex_payload(s: &str) -> CmdResult<Vec<u8>> {
    let stripped = s.strip_prefix("0x").unwrap_or(s);
    hex::decode(stripped).map_err(|e| CmdError::Chain(format!("bad calldata hex: {e}")))
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
            // Cardano hot-wallet derivation requires Ed25519-BIP32
            // (CIP-1852). Until P3.1 lands the Ledger HID path, we
            // surface no derived address for ADA on hot profiles —
            // users wanting to track ADA balances should add a
            // watch-only account for it.
            if chain_id == "ada" {
                return Ok(None);
            }
            // LTC/DOGE/BCH derivation also waits on P3.1 — same
            // rationale: most holders sign on hardware anyway, and
            // shipping software signing first invites phishing-via-
            // clipboard UX problems.
            if atlas_chain_utxo::network_by_id(chain_id).is_some() {
                return Ok(None);
            }
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
    match chain_id {
        "btc" => ChainKind::Bitcoin,
        "sol" => ChainKind::Solana,
        "trx" => ChainKind::Tron,
        id if atlas_chain_cosmos::network_by_id(id).is_some() => ChainKind::Cosmos,
        _ => ChainKind::Evm,
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
        // Auto-pick "Wallet N" where N is the lowest unused index >= 2.
        let existing: std::collections::HashSet<&str> =
            reg.profiles().iter().map(|p| p.name.as_str()).collect();
        for n in 2u32..=u32::MAX {
            let candidate = format!("Wallet {n}");
            if !existing.contains(candidate.as_str()) {
                return Ok(candidate);
            }
        }
        Err(CmdError::InvalidInput(
            "could not allocate a profile name".into(),
        ))
    }
}

// =============================================================================
// RPC endpoints (sovereignty: never force the user onto a third party).
// =============================================================================

#[derive(Debug, Serialize, specta::Type)]
pub struct RpcEndpoint {
    pub chain_id: String,
    /// Built-in default URL Atlas falls back to when no override is set.
    pub default_url: Option<String>,
    /// User-configured override, if any.
    pub override_url: Option<String>,
    /// The URL providers are actually dialing right now.
    pub effective_url: Option<String>,
}

fn endpoint_for(state: &AppState, chain_id: &str) -> RpcEndpoint {
    let default_url = crate::state::default_endpoint(chain_id).map(|s| s.to_string());
    let override_url = state.settings.rpc_override(chain_id);
    let effective_url = override_url.clone().or_else(|| default_url.clone());
    RpcEndpoint {
        chain_id: chain_id.to_string(),
        default_url,
        override_url,
        effective_url,
    }
}

/// List every supported chain id alongside its default and (if any) overridden RPC.
#[tauri::command]
#[specta::specta]
pub async fn list_rpc_endpoints(state: State<'_, Arc<AppState>>) -> CmdResult<Vec<RpcEndpoint>> {
    Ok(crate::state::all_chain_ids()
        .into_iter()
        .map(|id| endpoint_for(&state, id))
        .collect())
}

/// Override the RPC URL for a given chain. The new endpoint takes effect
/// immediately for every subsequent IPC call.
#[tauri::command]
#[specta::specta]
pub async fn set_rpc_endpoint(
    state: State<'_, Arc<AppState>>,
    chain_id: String,
    url: String,
) -> CmdResult<RpcEndpoint> {
    if crate::state::default_endpoint(&chain_id).is_none() {
        return Err(CmdError::InvalidInput(format!("unknown chain: {chain_id}")));
    }
    state
        .settings
        .set_rpc_override(&chain_id, &url)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))?;
    let trimmed = url.trim();
    if !state.chains.replace(&chain_id, trimmed) {
        return Err(CmdError::InvalidInput(format!(
            "chain provider for {chain_id} could not be rebuilt"
        )));
    }
    Ok(endpoint_for(&state, &chain_id))
}

/// Remove the user override for a chain, falling back to the built-in default.
#[tauri::command]
#[specta::specta]
pub async fn clear_rpc_endpoint(
    state: State<'_, Arc<AppState>>,
    chain_id: String,
) -> CmdResult<RpcEndpoint> {
    let default = crate::state::default_endpoint(&chain_id)
        .ok_or_else(|| CmdError::InvalidInput(format!("unknown chain: {chain_id}")))?;
    state
        .settings
        .clear_rpc_override(&chain_id)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))?;
    state.chains.replace(&chain_id, default);
    Ok(endpoint_for(&state, &chain_id))
}

// =============================================================================
// Local transaction history (sqlite cache)
// =============================================================================

/// Best-effort: insert an outgoing transaction into the local cache and
/// emit a `TxRecordedEvent` so the UI's history list can update without
/// repolling. Errors are logged and swallowed — the broadcast already
/// succeeded so we must never fail the user-visible flow over a cache
/// hiccup.
#[allow(clippy::too_many_arguments)]
async fn record_outgoing_tx(
    app: &AppHandle,
    state: &AppState,
    chain_id: &str,
    txid: &str,
    to: &str,
    amount: String,
    asset: String,
    fee: String,
) {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let rec = TxRecord {
        txid: txid.to_string(),
        chain_id: chain_id.to_string(),
        direction: "send".into(),
        counterparty: to.to_string(),
        amount,
        asset,
        fee,
        timestamp,
        status: "pending".into(),
        memo: None,
    };
    if let Err(e) = state.db.record_tx(&rec).await {
        tracing::warn!(target: "Atlas", "tx_history insert failed: {}", e);
        return;
    }
    let _ = TxRecordedEvent { record: rec }.emit(app);
}

/// Return the most-recent cached transactions. Pass chain_id = "" to query
/// across every chain.
#[tauri::command]
#[specta::specta]
pub async fn tx_history_list(
    state: State<'_, Arc<AppState>>,
    chain_id: String,
    limit: u32,
) -> CmdResult<Vec<TxRecord>> {
    let limit = limit.clamp(1, 500) as i64;
    state
        .db
        .list_history(&chain_id, limit)
        .await
        .map_err(|e| CmdError::Io(e.to_string()))
}

/// Update the on-disk status of a single tx (pending ? confirmed/failed).
/// The frontend can call this once a confirmation watcher resolves.
#[tauri::command]
#[specta::specta]
pub async fn tx_history_set_status(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    chain_id: String,
    txid: String,
    status: String,
) -> CmdResult<()> {
    state
        .db
        .update_status(&chain_id, &txid, &status)
        .await
        .map_err(|e| CmdError::Io(e.to_string()))?;
    let _ = TxStatusChangedEvent {
        chain_id,
        txid,
        status,
    }
    .emit(&app);
    Ok(())
}

/// Allow the UI to record a tx that didn't go through Atlas's send flow
/// (e.g. an external broadcast the user wants to track).
#[tauri::command]
#[specta::specta]
pub async fn tx_history_record(state: State<'_, Arc<AppState>>, record: TxRecord) -> CmdResult<()> {
    state
        .db
        .record_tx(&record)
        .await
        .map_err(|e| CmdError::Io(e.to_string()))
}
