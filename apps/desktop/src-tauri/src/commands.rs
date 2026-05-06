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
use atlas_profile::{HardwareVendor, HwAccount, ProfileKind, ProfileSummary, WatchAccount};
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

/// How the active profile signs transactions. Used by the Send
/// flow to decide whether to show the password prompt, the
/// hardware-device prompt, or refuse outright (watch-only).
#[derive(Debug, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SigningCapability {
    /// No active profile yet.
    None,
    /// Hot wallet — sign in-process after password unlock.
    Hot,
    /// Hardware-backed — caller must drive the device.
    Hardware {
        /// `"ledger"` or `"trezor"`.
        vendor: String,
    },
    /// Watch-only — cannot sign at all.
    WatchOnly,
}

/// Returns how the active profile expects to sign transactions.
/// Frontend Send / Swap flows poll this on mount so they can
/// switch the confirm button between "Unlock to send",
/// "Confirm on device", and "Watch-only — cannot send".
#[tauri::command]
#[specta::specta]
pub async fn active_signing_capability(
    state: State<'_, Arc<AppState>>,
) -> CmdResult<SigningCapability> {
    let reg = state.profiles.read().await;
    Ok(match reg.active() {
        None => SigningCapability::None,
        Some(p) => match &p.kind {
            ProfileKind::Hot { .. } => SigningCapability::Hot,
            ProfileKind::Hardware { vendor, .. } => SigningCapability::Hardware {
                vendor: match vendor {
                    atlas_profile::HardwareVendor::Ledger => "ledger".into(),
                    atlas_profile::HardwareVendor::Trezor => "trezor".into(),
                },
            },
            ProfileKind::WatchOnly { .. } => SigningCapability::WatchOnly,
        },
    })
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

#[derive(Debug, Deserialize, specta::Type)]
pub struct CreateHardwareArgs {
    pub name: String,
    pub vendor: HardwareVendor,
    pub accounts: Vec<HwAccount>,
}

/// Create a hardware-backed profile. Accounts are expected to have
/// already been derived from the device by the caller (via the
/// hardware-ledger / hardware-trezor crate); this command is the
/// pure persistence half.
#[tauri::command]
#[specta::specta]
pub async fn create_hardware_profile(
    state: State<'_, Arc<AppState>>,
    args: CreateHardwareArgs,
) -> CmdResult<ProfileSummary> {
    let mut reg = state.profiles.write().await;
    let p = reg.create_hardware(&args.name, args.vendor, args.accounts)?;
    let summary = ProfileSummary::from(p);
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

/// Currently configured anti-phishing phrase. Surfaced on every
/// unlock screen so users can tell a phishing UI from the real
/// Atlas.
#[tauri::command]
#[specta::specta]
pub async fn get_anti_phishing_phrase(
    state: State<'_, Arc<AppState>>,
) -> CmdResult<Option<String>> {
    Ok(state.settings.anti_phishing_phrase())
}

/// Set or clear the anti-phishing phrase. Pass `None` / empty
/// string to disable the feature.
#[tauri::command]
#[specta::specta]
pub async fn set_anti_phishing_phrase(
    state: State<'_, Arc<AppState>>,
    phrase: Option<String>,
) -> CmdResult<Option<String>> {
    state
        .settings
        .set_anti_phishing_phrase(phrase.as_deref())
        .map_err(|e| CmdError::Io(e.to_string()))?;
    Ok(state.settings.anti_phishing_phrase())
}

/// Snapshot of biometric capabilities + user preference. Used by
/// the unlock screen to decide whether to show the Hello / Touch
/// ID prompt button.
#[derive(Debug, serde::Serialize, specta::Type)]
pub struct BiometricStatus {
    /// `true` if the OS reports an enrolled sensor reachable.
    pub available: bool,
    /// `true` if the user has opted in to biometric unlock.
    pub enabled: bool,
}

/// Combined biometric availability + user preference.
#[tauri::command]
#[specta::specta]
pub async fn biometric_status(state: State<'_, Arc<AppState>>) -> CmdResult<BiometricStatus> {
    let provider = atlas_biometric::default_provider();
    let available = provider.is_available().await;
    Ok(BiometricStatus {
        available,
        enabled: state.settings.biometric_unlock_enabled(),
    })
}

/// Persist the user's biometric-unlock preference. The actual
/// platform check happens at unlock time; this command only
/// records the preference.
#[tauri::command]
#[specta::specta]
pub async fn set_biometric_unlock_enabled(
    state: State<'_, Arc<AppState>>,
    enabled: bool,
) -> CmdResult<bool> {
    state
        .settings
        .set_biometric_unlock_enabled(enabled)
        .map_err(|e| CmdError::Io(e.to_string()))?;
    Ok(state.settings.biometric_unlock_enabled())
}

/// Snapshot of the recovery-drill reminder state. Frontend uses
/// this to decide whether to show the "verify your seed" banner.
#[derive(Debug, serde::Serialize, specta::Type)]
pub struct RecoveryDrillStatus {
    /// `true` if a drill is currently due.
    pub due: bool,
    /// Reminder interval in days. `0` = disabled.
    pub interval_days: u32,
    /// Unix seconds of the last completed drill, or `None`.
    pub last_at: Option<i64>,
}

/// Current recovery-drill status (due / interval / last).
#[tauri::command]
#[specta::specta]
pub async fn recovery_drill_status(
    state: State<'_, Arc<AppState>>,
) -> CmdResult<RecoveryDrillStatus> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Ok(RecoveryDrillStatus {
        due: state.settings.recovery_drill_is_due(now),
        interval_days: state.settings.recovery_drill_interval_days(),
        last_at: state.settings.last_recovery_drill_at(),
    })
}

/// Update the recovery-drill reminder interval. Pass `0` to
/// disable the reminder. Values above 365 days are clamped.
#[tauri::command]
#[specta::specta]
pub async fn set_recovery_drill_interval_days(
    state: State<'_, Arc<AppState>>,
    days: u32,
) -> CmdResult<u32> {
    state
        .settings
        .set_recovery_drill_interval_days(days)
        .map_err(|e| CmdError::Io(e.to_string()))?;
    Ok(state.settings.recovery_drill_interval_days())
}

/// Record that the user just successfully verified their seed
/// phrase. The frontend is responsible for actually performing
/// the verification (showing a few random words and checking the
/// user types them back).
#[tauri::command]
#[specta::specta]
pub async fn record_recovery_drill_completed(state: State<'_, Arc<AppState>>) -> CmdResult<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    state
        .settings
        .mark_recovery_drill_completed(now)
        .map_err(|e| CmdError::Io(e.to_string()))?;
    Ok(())
}

/// Atlas-wide swap-fee configuration. Surfaced to the UI so
/// users can see exactly what cut Atlas takes (currently 0.25 %
/// by default, capped at 1 %).
#[derive(Debug, serde::Serialize, specta::Type)]
pub struct SwapFeeConfig {
    /// Atlas's default fee in basis points (25 = 0.25 %).
    pub fee_bps: u32,
    /// THORChain affiliate THORName, when configured.
    pub thorchain_affiliate: Option<String>,
}

/// Read the current swap-fee configuration.
#[tauri::command]
#[specta::specta]
pub async fn get_swap_fee_config(state: State<'_, Arc<AppState>>) -> CmdResult<SwapFeeConfig> {
    Ok(SwapFeeConfig {
        fee_bps: state.settings.swap_fee_bps(),
        thorchain_affiliate: state.settings.thorchain_affiliate(),
    })
}

/// Update Atlas's swap-fee bps. Clamped to `[0, 100]` (1 %).
#[tauri::command]
#[specta::specta]
pub async fn set_swap_fee_bps(state: State<'_, Arc<AppState>>, bps: u32) -> CmdResult<u32> {
    state
        .settings
        .set_swap_fee_bps(bps)
        .map_err(|e| CmdError::Io(e.to_string()))?;
    Ok(state.settings.swap_fee_bps())
}

/// Persist (or clear) the THORChain affiliate THORName.
#[tauri::command]
#[specta::specta]
pub async fn set_thorchain_affiliate(
    state: State<'_, Arc<AppState>>,
    name: Option<String>,
) -> CmdResult<Option<String>> {
    state
        .settings
        .set_thorchain_affiliate(name.as_deref())
        .map_err(|e| CmdError::Io(e.to_string()))?;
    Ok(state.settings.thorchain_affiliate())
}

/// Default Flashbots Protect endpoint. Routes Ethereum mainnet
/// transactions through a private MEV-aware mempool instead of
/// the public one, blocking sandwich and front-run attacks.
pub const FLASHBOTS_PROTECT_URL: &str = "https://rpc.flashbots.net";

/// Whether the Ethereum-mainnet RPC currently points at the
/// Flashbots Protect endpoint.
#[tauri::command]
#[specta::specta]
pub async fn flashbots_protect_enabled(state: State<'_, Arc<AppState>>) -> CmdResult<bool> {
    Ok(state
        .settings
        .rpc_override("ethereum")
        .as_deref()
        .map(|u| u.starts_with(FLASHBOTS_PROTECT_URL))
        .unwrap_or(false))
}

/// Toggle Flashbots Protect for Ethereum mainnet. When enabled,
/// the wallet sets the `ethereum` RPC override to the Flashbots
/// Protect URL; when disabled, the override is cleared so Atlas
/// falls back to its default mainnet RPC. Other chains are
/// unaffected.
#[tauri::command]
#[specta::specta]
pub async fn set_flashbots_protect(
    state: State<'_, Arc<AppState>>,
    enabled: bool,
) -> CmdResult<bool> {
    if enabled {
        state
            .settings
            .set_rpc_override("ethereum", FLASHBOTS_PROTECT_URL)
            .map_err(|e| CmdError::Io(e.to_string()))?;
    } else {
        // Only clear the override if we set it. Don't stomp on a
        // user's custom RPC.
        let cur = state.settings.rpc_override("ethereum");
        if cur
            .as_deref()
            .map(|u| u.starts_with(FLASHBOTS_PROTECT_URL))
            .unwrap_or(false)
        {
            state
                .settings
                .clear_rpc_override("ethereum")
                .map_err(|e| CmdError::Io(e.to_string()))?;
        }
    }
    Ok(state
        .settings
        .rpc_override("ethereum")
        .as_deref()
        .map(|u| u.starts_with(FLASHBOTS_PROTECT_URL))
        .unwrap_or(false))
}

// =============================================================================
// Fiat on-ramp (MoonPay)
// =============================================================================

/// MoonPay configuration surfaced to the UI. The secret key, when set,
/// is reported only as a boolean — it never crosses the IPC boundary.
#[derive(Debug, Serialize, specta::Type)]
pub struct MoonPayConfig {
    pub api_key: Option<String>,
    pub secret_key_set: bool,
    pub production: bool,
}

#[derive(Debug, Deserialize, specta::Type)]
pub struct MoonPayConfigInput {
    pub api_key: Option<String>,
    /// `Some("")` explicitly clears the stored secret. `None` leaves it as-is.
    pub secret_key: Option<String>,
    pub production: bool,
}

#[tauri::command]
#[specta::specta]
pub async fn get_moonpay_config(state: State<'_, Arc<AppState>>) -> CmdResult<MoonPayConfig> {
    Ok(MoonPayConfig {
        api_key: state.settings.moonpay_api_key(),
        secret_key_set: state.settings.moonpay_secret_key().is_some(),
        production: state.settings.moonpay_production(),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn set_moonpay_config(
    state: State<'_, Arc<AppState>>,
    args: MoonPayConfigInput,
) -> CmdResult<MoonPayConfig> {
    state
        .settings
        .set_moonpay_api_key(args.api_key.as_deref())
        .map_err(|e| CmdError::Io(e.to_string()))?;
    if let Some(secret) = &args.secret_key {
        state
            .settings
            .set_moonpay_secret_key(if secret.is_empty() {
                None
            } else {
                Some(secret.as_str())
            })
            .map_err(|e| CmdError::Io(e.to_string()))?;
    }
    state
        .settings
        .set_moonpay_production(args.production)
        .map_err(|e| CmdError::Io(e.to_string()))?;
    Ok(MoonPayConfig {
        api_key: state.settings.moonpay_api_key(),
        secret_key_set: state.settings.moonpay_secret_key().is_some(),
        production: state.settings.moonpay_production(),
    })
}

#[derive(Debug, Deserialize, specta::Type)]
pub struct MoonPayBuyArgs {
    pub currency_code: String,
    pub wallet_address: String,
    pub base_currency_amount: Option<String>,
    pub base_currency_code: Option<String>,
    pub redirect_url: Option<String>,
}

/// Build a (possibly signed) MoonPay Buy widget URL using the persisted
/// configuration. The returned URL is intended to be opened in the system
/// browser.
#[tauri::command]
#[specta::specta]
pub async fn build_moonpay_buy_url(
    state: State<'_, Arc<AppState>>,
    args: MoonPayBuyArgs,
) -> CmdResult<String> {
    let api_key = state
        .settings
        .moonpay_api_key()
        .ok_or_else(|| CmdError::InvalidInput("MoonPay API key not configured".into()))?;
    let environment = if state.settings.moonpay_production() {
        atlas_onramp::Environment::Production
    } else {
        atlas_onramp::Environment::Sandbox
    };
    let params = atlas_onramp::MoonPayBuyParams {
        api_key,
        currency_code: args.currency_code,
        wallet_address: args.wallet_address,
        base_currency_amount: args.base_currency_amount,
        base_currency_code: args.base_currency_code,
        environment,
        redirect_url: args.redirect_url,
    };
    let url =
        atlas_onramp::build_buy_url(&params).map_err(|e| CmdError::InvalidInput(e.to_string()))?;
    if let Some(secret) = state.settings.moonpay_secret_key() {
        atlas_onramp::sign_url(&url, &secret).map_err(|e| CmdError::InvalidInput(e.to_string()))
    } else {
        Ok(url)
    }
}

#[derive(Debug, Deserialize, specta::Type)]
pub struct MoonPaySellArgs {
    pub base_currency_code: String,
    pub refund_wallet_address: String,
    pub base_currency_amount: Option<String>,
    pub quote_currency_code: Option<String>,
    pub redirect_url: Option<String>,
}

/// Build a (possibly signed) MoonPay Sell widget URL using the persisted
/// configuration. The user lands on MoonPay, sells crypto for fiat, and
/// MoonPay handles KYC + bank/card payout.
#[tauri::command]
#[specta::specta]
pub async fn build_moonpay_sell_url(
    state: State<'_, Arc<AppState>>,
    args: MoonPaySellArgs,
) -> CmdResult<String> {
    let api_key = state
        .settings
        .moonpay_api_key()
        .ok_or_else(|| CmdError::InvalidInput("MoonPay API key not configured".into()))?;
    let environment = if state.settings.moonpay_production() {
        atlas_onramp::Environment::Production
    } else {
        atlas_onramp::Environment::Sandbox
    };
    let params = atlas_onramp::MoonPaySellParams {
        api_key,
        base_currency_code: args.base_currency_code,
        refund_wallet_address: args.refund_wallet_address,
        base_currency_amount: args.base_currency_amount,
        quote_currency_code: args.quote_currency_code,
        environment,
        redirect_url: args.redirect_url,
    };
    let url =
        atlas_onramp::build_sell_url(&params).map_err(|e| CmdError::InvalidInput(e.to_string()))?;
    if let Some(secret) = state.settings.moonpay_secret_key() {
        atlas_onramp::sign_url(&url, &secret).map_err(|e| CmdError::InvalidInput(e.to_string()))
    } else {
        Ok(url)
    }
}

// ----- Multisig BTC ----------------------------------------------------------

/// Build an N-of-M `wsh(sortedmulti)` descriptor wallet from a policy.
#[tauri::command]
#[specta::specta]
pub async fn multisig_btc_build_descriptor(
    policy: atlas_multisig_btc::MultisigPolicy,
) -> CmdResult<atlas_multisig_btc::MultisigWallet> {
    atlas_multisig_btc::build_descriptor(&policy).map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Derive the receive (`change=false`) or change (`change=true`) address
/// at `index` for a previously-built multisig wallet.
#[tauri::command]
#[specta::specta]
pub async fn multisig_btc_derive_address(
    wallet: atlas_multisig_btc::MultisigWallet,
    change: bool,
    index: u32,
) -> CmdResult<String> {
    atlas_multisig_btc::derive_address(&wallet, change, index)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Decode a base64 PSBT and return a UI-friendly summary.
#[tauri::command]
#[specta::specta]
pub async fn multisig_btc_psbt_summary(
    psbt_b64: String,
) -> CmdResult<atlas_multisig_btc::psbt::PsbtSummary> {
    atlas_multisig_btc::psbt::summarise_psbt(&psbt_b64)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Combine partially-signed PSBTs (one from each co-signer) into a
/// single PSBT carrying every collected signature.
#[tauri::command]
#[specta::specta]
pub async fn multisig_btc_psbt_combine(parts: Vec<String>) -> CmdResult<String> {
    atlas_multisig_btc::psbt::combine_psbts(&parts)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Finalise a fully-signed PSBT and return the raw broadcast-ready
/// transaction hex.
#[tauri::command]
#[specta::specta]
pub async fn multisig_btc_psbt_finalize(psbt_b64: String) -> CmdResult<String> {
    atlas_multisig_btc::psbt::finalize_psbt(&psbt_b64)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Build a shareable `atlas-msig://signer?...` invite URI for a
/// co-signer. The receiving wallet decodes it and gets a ready-to-use
/// `MultisigSigner` (xpub + origin + fingerprint + optional label).
#[tauri::command]
#[specta::specta]
pub async fn multisig_btc_build_invite(
    invite: atlas_multisig_btc::invite::CoSignerInvite,
) -> CmdResult<String> {
    atlas_multisig_btc::invite::build_invite_uri(&invite)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Parse a co-signer invite URI received from a partner. Rejects
/// invites whose embedded xpub doesn't decode (cheap tamper check).
#[tauri::command]
#[specta::specta]
pub async fn multisig_btc_parse_invite(
    uri: String,
) -> CmdResult<atlas_multisig_btc::invite::CoSignerInvite> {
    atlas_multisig_btc::invite::parse_invite_uri(&uri)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

// ----- Multisig EVM (Safe) ---------------------------------------------------

/// Compute the canonical EIP-712 `safeTxHash` and `domainSeparator` for
/// a Safe transaction. Co-signers verify these before signing so they
/// know exactly which chain + Safe + payload their signature commits to.
#[tauri::command]
#[specta::specta]
pub async fn multisig_evm_safe_tx_hash(
    safe_address: String,
    chain_id: u64,
    tx: atlas_multisig_evm::SafeTx,
) -> CmdResult<atlas_multisig_evm::SafeTxHashes> {
    atlas_multisig_evm::safe_tx_hash(&safe_address, chain_id, &tx)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Pack collected `(signer, signature)` pairs into the contiguous bytes
/// blob `Safe.execTransaction` expects. Signatures are sorted by signer
/// address ascending — the order Safe v1.3+ requires.
#[tauri::command]
#[specta::specta]
pub async fn multisig_evm_pack_signatures(sigs: Vec<(String, String)>) -> CmdResult<String> {
    let parsed: Result<Vec<_>, _> = sigs
        .into_iter()
        .map(|(addr, hex_sig)| {
            let s = hex_sig.trim().trim_start_matches("0x");
            hex::decode(s)
                .map(|bytes| (addr, bytes))
                .map_err(|e| CmdError::InvalidInput(format!("signature hex: {e}")))
        })
        .collect();
    let parsed = parsed?;
    atlas_multisig_evm::pack_signatures(parsed).map_err(|e| CmdError::InvalidInput(e.to_string()))
}

// ----- ERC-4337 Account Abstraction ------------------------------------------

/// Compute the canonical EntryPoint v0.6 `userOpHash` a smart-account
/// owner must sign before submitting to a bundler.
#[tauri::command]
#[specta::specta]
pub async fn aa_user_op_hash(
    entry_point: String,
    chain_id: u64,
    op: atlas_aa_erc4337::UserOperation,
) -> CmdResult<atlas_aa_erc4337::UserOpHashes> {
    atlas_aa_erc4337::user_op_hash(&entry_point, chain_id, &op)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Encode `(target, value, data)` as ABI calldata for a SimpleAccount's
/// `execute(address,uint256,bytes)` entry — the canonical inner-call
/// dispatcher used by virtually every ERC-4337 account implementation.
#[tauri::command]
#[specta::specta]
pub async fn aa_encode_execute_calldata(
    target: String,
    value: String,
    data: String,
) -> CmdResult<String> {
    let value_u128: u128 = value
        .parse()
        .map_err(|e| CmdError::InvalidInput(format!("value: {e}")))?;
    atlas_aa_erc4337::encode_execute_calldata(&target, value_u128, &data)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

// ---- Shamir Secret Sharing (SLIP-39 GF(256) core) -------------------

/// Split a hex-encoded secret into `total` Shamir shares with the
/// given recovery `threshold`. Uses OS RNG for the polynomial
/// coefficients so the same secret produces fresh shares every call.
#[tauri::command]
#[specta::specta]
pub async fn shamir_split(
    secret_hex: String,
    threshold: u8,
    total: u8,
) -> CmdResult<Vec<atlas_shamir::Share>> {
    let secret =
        hex::decode(secret_hex).map_err(|e| CmdError::InvalidInput(format!("secret_hex: {e}")))?;
    let mut rng = rand::thread_rng();
    atlas_shamir::split(&secret, threshold, total, &mut rng)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Recover a secret (hex-encoded) from at least `threshold` Shamir
/// shares previously produced by `shamir_split`.
#[tauri::command]
#[specta::specta]
pub async fn shamir_combine(shares: Vec<atlas_shamir::Share>) -> CmdResult<String> {
    let bytes =
        atlas_shamir::combine(&shares).map_err(|e| CmdError::InvalidInput(e.to_string()))?;
    Ok(hex::encode(bytes))
}

// ---- WalletConnect v2 -------------------------------------------------

/// Parse a `wc:` v2 pairing URI into its components.
#[tauri::command]
#[specta::specta]
pub async fn wc_parse_uri(uri: String) -> CmdResult<atlas_walletconnect::WcUri> {
    atlas_walletconnect::parse_wc_uri(&uri).map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Re-emit a `wc:` v2 URI from a parsed `WcUri`.
#[tauri::command]
#[specta::specta]
pub async fn wc_build_uri(uri: atlas_walletconnect::WcUri) -> CmdResult<String> {
    atlas_walletconnect::build_wc_uri(&uri).map_err(|e| CmdError::InvalidInput(e.to_string()))
}

// ---- In-app dApp browser registry -----------------------------------

/// Evaluate a candidate URL against the curated dApp registry. Used by
/// the in-app browser before rendering a third-party origin.
#[tauri::command]
#[specta::specta]
pub async fn dapp_assess_origin(url: String) -> CmdResult<atlas_dapp_registry::DappAssessment> {
    let reg = atlas_dapp_registry::DappRegistry::with_defaults();
    reg.assess(&url)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Return the built-in curated dApp list (UI directory / search).
#[tauri::command]
#[specta::specta]
pub async fn dapp_list_curated() -> CmdResult<Vec<atlas_dapp_registry::DappEntry>> {
    Ok(atlas_dapp_registry::DappRegistry::with_defaults().entries)
}

// ---- Phishing-domain detector ---------------------------------------

/// Analyse an arbitrary origin URL using the built-in defence config
/// (curated DEX/NFT/wallet brand list) plus typosquat / homoglyph /
/// Punycode heuristics. The dApp registry handles the curated good
/// origins; this command catches the long tail.
#[tauri::command]
#[specta::specta]
pub async fn phishing_analyze_default(origin: String) -> CmdResult<atlas_phishing::PhishingReport> {
    atlas_phishing::analyze(&origin, &atlas_phishing::default_config())
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

// ---- NFT gallery aggregation ----------------------------------------

/// Aggregate a flat list of owned NFTs into the gallery view used by
/// the UI (collection grouping, value totals, optional spam filter).
#[tauri::command]
#[specta::specta]
pub async fn nft_gallery_view(
    items: Vec<atlas_nft_registry::OwnedNft>,
    filter: atlas_nft_gallery::GalleryFilter,
) -> CmdResult<atlas_nft_gallery::GalleryView> {
    Ok(atlas_nft_gallery::build_gallery(&items, &filter))
}

// ---- Staking aggregation --------------------------------------------

/// Aggregate a user's delegations on one chain into a `ChainPosition`
/// (totals + stake-weighted net APR). Validators are needed so we can
/// resolve each delegation's APR.
#[tauri::command]
#[specta::specta]
pub async fn staking_aggregate_chain(
    chain: atlas_staking::StakingChain,
    delegations: Vec<atlas_staking::Delegation>,
    validators: Vec<atlas_staking::Validator>,
) -> CmdResult<atlas_staking::ChainPosition> {
    atlas_staking::aggregate_chain(chain, &delegations, &validators)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Pick a sensible validator from a candidate list (active +
/// reasonable commission + decentralisation-friendly tiebreakers).
#[tauri::command]
#[specta::specta]
pub async fn staking_recommend_validator(
    validators: Vec<atlas_staking::Validator>,
) -> CmdResult<Option<atlas_staking::Validator>> {
    Ok(atlas_staking::recommend_validator(&validators).cloned())
}

/// Convert a nominal APR to APY using the chain's typical compounding
/// frequency (saves the UI from hard-coding magic numbers).
#[tauri::command]
#[specta::specta]
pub async fn staking_apr_to_apy(chain: atlas_staking::StakingChain, apr: f64) -> CmdResult<f64> {
    Ok(atlas_staking::apr_to_apy(
        apr,
        chain.default_compounds_per_year(),
    ))
}

// ---- Rich tx history ------------------------------------------------

/// Apply UI filters (chain / direction / status / date / asset /
/// query) and return rows newest-first.
#[tauri::command]
#[specta::specta]
pub async fn tx_history_filter(
    rows: Vec<atlas_tx_history::Tx>,
    filter: atlas_tx_history::HistoryFilter,
) -> CmdResult<Vec<atlas_tx_history::Tx>> {
    Ok(atlas_tx_history::filter_history(&rows, &filter))
}

/// Aggregate a (typically already-filtered) row set into per-asset
/// and per-chain totals plus first/last timestamps.
#[tauri::command]
#[specta::specta]
pub async fn tx_history_summarise(
    rows: Vec<atlas_tx_history::Tx>,
) -> CmdResult<atlas_tx_history::HistorySummary> {
    Ok(atlas_tx_history::summarise(&rows))
}

/// Render a row set as RFC-4180-style CSV, ready for download.
#[tauri::command]
#[specta::specta]
pub async fn tx_history_export_csv(rows: Vec<atlas_tx_history::Tx>) -> CmdResult<String> {
    Ok(atlas_tx_history::export_csv(&rows))
}

// ---- Multi-source price oracle --------------------------------------

/// Aggregate independent price feeds into a single median quote with
/// outlier rejection. Quotes are pre-fetched by the frontend (or by
/// other Tauri commands) so this command itself does no I/O.
#[tauri::command]
#[specta::specta]
pub async fn price_oracle_aggregate(
    quotes: Vec<atlas_price_oracle::SourceQuote>,
    max_deviation_bps: u32,
) -> CmdResult<atlas_price_oracle::AggregatedPrice> {
    atlas_price_oracle::aggregate_prices(&quotes, max_deviation_bps)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

// ---- Token approval risk dashboard ----------------------------------

/// Score every approval against the (curated) config and return rows
/// sorted Critical first.
#[tauri::command]
#[specta::specta]
pub async fn approvals_analyze(
    approvals: Vec<atlas_approvals::Approval>,
    config: atlas_approvals::ApprovalConfig,
    now: i64,
) -> CmdResult<Vec<atlas_approvals::ApprovalRisk>> {
    Ok(atlas_approvals::analyze(&approvals, &config, now))
}

/// Aggregate per-chain risk counts for the dashboard header.
#[tauri::command]
#[specta::specta]
pub async fn approvals_summarise(
    rows: Vec<atlas_approvals::ApprovalRisk>,
) -> CmdResult<Vec<atlas_approvals::ApprovalSummary>> {
    Ok(atlas_approvals::summarise(&rows))
}

// ---- EVM calldata decoder -------------------------------------------

/// Decode raw EVM calldata into a structured action description.
#[tauri::command]
#[specta::specta]
pub async fn calldata_decode(data: String) -> CmdResult<atlas_calldata::DecodedCall> {
    atlas_calldata::decode(&data).map_err(|e| CmdError::InvalidInput(e.to_string()))
}

// ---- Phishing-domain check ------------------------------------------

/// Analyse a dApp origin URL for phishing indicators.
#[tauri::command]
#[specta::specta]
pub async fn phishing_analyze(
    origin: String,
    config: atlas_phishing::PhishingConfig,
) -> CmdResult<atlas_phishing::PhishingReport> {
    atlas_phishing::analyze(&origin, &config).map_err(|e| CmdError::InvalidInput(e.to_string()))
}

// ---- Address-poisoning detector -------------------------------------

/// Flag incoming transfers from addresses that visually mimic
/// addresses the user has previously sent funds to.
#[tauri::command]
#[specta::specta]
pub async fn poisoning_detect(
    transfers: Vec<atlas_address_poisoning::IncomingTransfer>,
    trusted: Vec<atlas_address_poisoning::TrustedCounterparty>,
    config: atlas_address_poisoning::PoisonConfig,
) -> CmdResult<Vec<atlas_address_poisoning::PoisonAlert>> {
    atlas_address_poisoning::detect(&transfers, &trusted, &config)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Live-input check for the Send page: compare a candidate recipient
/// against every address in the user's contact book and return any
/// look-alikes whose hex prefix/suffix overlap exceeds the defaults.
#[tauri::command]
#[specta::specta]
pub async fn poisoning_check_candidate(
    state: State<'_, Arc<AppState>>,
    candidate: String,
) -> CmdResult<Vec<atlas_address_poisoning::MimicMatch>> {
    let book = state.contacts.read().await;
    let trusted: Vec<String> = book
        .list()
        .into_iter()
        .flat_map(|c| c.addresses.into_iter().map(|a| a.address))
        .collect();
    drop(book);
    let refs: Vec<&str> = trusted.iter().map(|s| s.as_str()).collect();
    Ok(atlas_address_poisoning::compare_to_trusted(
        &candidate, &refs, 0, 0,
    ))
}

// ---- EIP-712 typed-data inspector -----------------------------------

/// Classify an EIP-712 typed-data signing request before the user signs.
#[tauri::command]
#[specta::specta]
pub async fn eip712_classify(json: String) -> CmdResult<atlas_eip712::Eip712Report> {
    atlas_eip712::classify(&json).map_err(|e| CmdError::InvalidInput(e.to_string()))
}

// ---- Malicious-address blocklist -----------------------------------

/// Check an address against the user's blocklist. Always returns a
/// verdict; an unparseable address is reported as `Clean` so we never
/// throw on user input.
#[tauri::command]
#[specta::specta]
pub async fn blocklist_check(
    state: State<'_, Arc<AppState>>,
    address: String,
) -> CmdResult<atlas_blocklist::BlocklistVerdict> {
    Ok(state.blocklist.read().await.check(&address))
}

/// Sorted snapshot of every entry currently in the blocklist.
#[tauri::command]
#[specta::specta]
pub async fn blocklist_list(
    state: State<'_, Arc<AppState>>,
) -> CmdResult<Vec<atlas_blocklist::BlocklistEntry>> {
    Ok(state.blocklist.read().await.sorted())
}

/// Add (or overwrite) one entry. Returns the normalised key.
#[tauri::command]
#[specta::specta]
pub async fn blocklist_add(
    state: State<'_, Arc<AppState>>,
    entry: atlas_blocklist::BlocklistEntry,
) -> CmdResult<String> {
    let key = state
        .blocklist
        .write()
        .await
        .add(entry)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))?;
    persist_blocklist(&state).await?;
    Ok(key)
}

/// Remove one entry. Returns true if it was present.
#[tauri::command]
#[specta::specta]
pub async fn blocklist_remove(state: State<'_, Arc<AppState>>, address: String) -> CmdResult<bool> {
    let removed = state.blocklist.write().await.remove(&address);
    if removed {
        persist_blocklist(&state).await?;
    }
    Ok(removed)
}

/// Bulk-merge a JSON array of entries (e.g. from a curated feed).
/// Returns the number of entries written.
#[tauri::command]
#[specta::specta]
pub async fn blocklist_import_json(
    state: State<'_, Arc<AppState>>,
    json: String,
) -> CmdResult<u32> {
    let added = state
        .blocklist
        .write()
        .await
        .merge_json(&json)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))?;
    persist_blocklist(&state).await?;
    Ok(added as u32)
}

async fn persist_blocklist(state: &Arc<AppState>) -> CmdResult<()> {
    let snapshot = state.blocklist.read().await.clone();
    crate::state::save_json(&state.data_dir, "blocklist.json", &snapshot)
        .map_err(|e| CmdError::InvalidInput(format!("persist blocklist: {e}")))
}

// ---- Portfolio P&L ---------------------------------------------------

/// Compute realized + unrealized P&L over a list of trades.
#[tauri::command]
#[specta::specta]
pub async fn pnl_compute(
    trades: Vec<atlas_pnl::Trade>,
    prices: std::collections::HashMap<String, f64>,
    method: atlas_pnl::AccountingMethod,
) -> CmdResult<atlas_pnl::PortfolioReport> {
    atlas_pnl::compute(&trades, &prices, method).map_err(|e| CmdError::InvalidInput(e.to_string()))
}

// ---- Persisted trade history (P&L source) ----------------------------

/// List every recorded trade, oldest-first.
#[tauri::command]
#[specta::specta]
pub async fn trades_list(state: State<'_, Arc<AppState>>) -> CmdResult<Vec<atlas_pnl::Trade>> {
    let mut rows = state.trades.read().await.clone();
    rows.sort_by_key(|a| a.ts);
    Ok(rows)
}

/// Append a trade to the local history and persist to disk.
#[tauri::command]
#[specta::specta]
pub async fn trades_add(
    state: State<'_, Arc<AppState>>,
    trade: atlas_pnl::Trade,
) -> CmdResult<u32> {
    {
        let mut store = state.trades.write().await;
        store.push(trade);
    }
    persist_trades(&state).await?;
    Ok(state.trades.read().await.len() as u32)
}

/// Remove the trade at `index` (0-based, oldest-first ordering).
/// Returns whether a trade was removed.
#[tauri::command]
#[specta::specta]
pub async fn trades_remove(state: State<'_, Arc<AppState>>, index: u32) -> CmdResult<bool> {
    let removed;
    {
        let mut store = state.trades.write().await;
        let mut indexed: Vec<(usize, &atlas_pnl::Trade)> = store.iter().enumerate().collect();
        indexed.sort_by_key(|a| a.1.ts);
        let target = indexed.get(index as usize).map(|(i, _)| *i);
        if let Some(i) = target {
            store.remove(i);
            removed = true;
        } else {
            removed = false;
        }
    }
    if removed {
        persist_trades(&state).await?;
    }
    Ok(removed)
}

/// Clear the entire trade history.
#[tauri::command]
#[specta::specta]
pub async fn trades_clear(state: State<'_, Arc<AppState>>) -> CmdResult<()> {
    {
        let mut store = state.trades.write().await;
        store.clear();
    }
    persist_trades(&state).await
}

/// Bulk-import a JSON array of `Trade` objects. Returns the number of
/// rows added.
#[tauri::command]
#[specta::specta]
pub async fn trades_import_json(state: State<'_, Arc<AppState>>, json: String) -> CmdResult<u32> {
    let rows: Vec<atlas_pnl::Trade> = serde_json::from_str(&json)
        .map_err(|e| CmdError::InvalidInput(format!("invalid trades json: {e}")))?;
    let added = rows.len();
    {
        let mut store = state.trades.write().await;
        store.extend(rows);
    }
    persist_trades(&state).await?;
    Ok(added as u32)
}

/// Compute P&L over the persisted trade history.
#[tauri::command]
#[specta::specta]
pub async fn trades_compute_pnl(
    state: State<'_, Arc<AppState>>,
    prices: std::collections::HashMap<String, f64>,
    method: atlas_pnl::AccountingMethod,
) -> CmdResult<atlas_pnl::PortfolioReport> {
    let trades = state.trades.read().await.clone();
    atlas_pnl::compute(&trades, &prices, method).map_err(|e| CmdError::InvalidInput(e.to_string()))
}

async fn persist_trades(state: &Arc<AppState>) -> CmdResult<()> {
    let snapshot = state.trades.read().await.clone();
    crate::state::save_json(&state.data_dir, "trades.json", &snapshot)
        .map_err(|e| CmdError::InvalidInput(format!("persist trades: {e}")))
}

// ---- Payment URI parser/builder ------------------------------------

/// Parse a BIP-21 / EIP-681 payment URI scanned from a QR code.
#[tauri::command]
#[specta::specta]
pub async fn payuri_parse(input: String) -> CmdResult<atlas_payuri::PaymentIntent> {
    atlas_payuri::parse(&input).map_err(|e| CmdError::InvalidInput(e.to_string()))
}

// ---- Fee estimator -------------------------------------------------

/// Suggest Slow / Standard / Fast EIP-1559 fee tiers from a recent
/// base-fee and a priority-fee history.
#[tauri::command]
#[specta::specta]
pub async fn fees_eip1559_suggest(
    base_fee_per_gas: String,
    recent_priority_fees: Vec<String>,
) -> CmdResult<atlas_fees::Eip1559Suggestions> {
    let base = base_fee_per_gas
        .parse::<u128>()
        .map_err(|e| CmdError::InvalidInput(format!("base_fee: {e}")))?;
    let history: Vec<u128> = recent_priority_fees
        .iter()
        .map(|s| s.parse::<u128>())
        .collect::<Result<_, _>>()
        .map_err(|e| CmdError::InvalidInput(format!("priority_fees: {e}")))?;
    atlas_fees::eip1559_suggest(base, &history).map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Bump an EIP-1559 suggestion by >=10% for replacing a stuck tx.
#[tauri::command]
#[specta::specta]
pub async fn fees_bump_for_replacement(
    suggestion: atlas_fees::Eip1559Suggestion,
) -> CmdResult<atlas_fees::Eip1559Suggestion> {
    Ok(atlas_fees::bump_for_replacement(&suggestion))
}

/// Total UTXO fee in sats given a sat/vB rate and a vsize estimate.
#[tauri::command]
#[specta::specta]
pub async fn fees_utxo_sats(sat_per_vbyte: u64, vsize: u64) -> CmdResult<u64> {
    atlas_fees::utxo_fee_sats(sat_per_vbyte, vsize)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

// ---- ENS namehash --------------------------------------------------

/// Cheap heuristic: does this string look like an ENS name?
#[tauri::command]
#[specta::specta]
pub async fn ens_looks_like_ens(name: String) -> CmdResult<bool> {
    Ok(atlas_ens::looks_like_ens(&name))
}

/// UTS-46 normalise an ENS name (lowercase + label validation).
#[tauri::command]
#[specta::specta]
pub async fn ens_normalise(name: String) -> CmdResult<String> {
    atlas_ens::normalise(&name).map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Compute the EIP-137 namehash of an ENS name as a 0x-prefixed hex string.
#[tauri::command]
#[specta::specta]
pub async fn ens_namehash(name: String) -> CmdResult<String> {
    atlas_ens::namehash_hex(&name).map_err(|e| CmdError::InvalidInput(e.to_string()))
}

// ---- gas-cost USD estimator ---------------------------------------

/// USD-denominated gas cost for an EVM tx.
///
/// `effective_gas_price_wei` is passed as a decimal string because
/// Tauri's specta layer can't round-trip u128 directly.
/// `native_price_usd_micro` is the native-token price scaled by 1e6
/// (e.g. $1234.567890 → 1_234_567_890).
#[tauri::command]
#[specta::specta]
pub async fn gascost_estimate(
    gas_used: u64,
    effective_gas_price_wei: String,
    native_price_usd_micro: u64,
) -> CmdResult<atlas_gascost::GasCost> {
    let price: u128 = effective_gas_price_wei
        .parse()
        .map_err(|_| CmdError::InvalidInput("effective_gas_price_wei not a u128".into()))?;
    atlas_gascost::estimate(gas_used, price, native_price_usd_micro)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Format a wei amount as a fixed-decimal native-token string.
#[tauri::command]
#[specta::specta]
pub async fn gascost_format_eth(wei: String, decimals: u32) -> CmdResult<String> {
    let wei: u128 = wei
        .parse()
        .map_err(|_| CmdError::InvalidInput("wei not a u128".into()))?;
    Ok(atlas_gascost::format_eth(wei, decimals))
}

// ---- portfolio diversification ------------------------------------

/// Score a portfolio's concentration and return per-position weights.
#[tauri::command]
#[specta::specta]
pub async fn diversification_analyse(
    holdings: Vec<atlas_diversification::Holding>,
) -> CmdResult<atlas_diversification::DiversificationReport> {
    atlas_diversification::analyse(&holdings).map_err(|e| CmdError::InvalidInput(e.to_string()))
}

// ---- swap slippage / deadline math --------------------------------

/// Validate slippage bps + deadline seconds. Returns the canonicalised pair.
#[tauri::command]
#[specta::specta]
pub async fn slippage_validate(
    slippage_bps: u32,
    deadline_secs: u64,
) -> CmdResult<atlas_slippage::SwapSettings> {
    atlas_slippage::SwapSettings::new(slippage_bps, deadline_secs)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Bucket a slippage value into Low / Normal / High / Reckless.
#[tauri::command]
#[specta::specta]
pub async fn slippage_band(slippage_bps: u32) -> CmdResult<atlas_slippage::SlippageBand> {
    let s = atlas_slippage::SwapSettings::new(slippage_bps, atlas_slippage::MIN_DEADLINE_SECS)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))?;
    Ok(s.band())
}

/// Floor `amountOutMin` from a quote and slippage tolerance.
///
/// `amount_out_quote` is a u128 passed as a decimal string so it
/// survives the JS bridge.
#[tauri::command]
#[specta::specta]
pub async fn slippage_min_out(amount_out_quote: String, slippage_bps: u32) -> CmdResult<String> {
    let q: u128 = amount_out_quote
        .parse()
        .map_err(|_| CmdError::InvalidInput("amount_out_quote not a u128".into()))?;
    atlas_slippage::min_out(q, slippage_bps)
        .map(|v| v.to_string())
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Ceiling `amountInMax` from a quote and slippage tolerance.
#[tauri::command]
#[specta::specta]
pub async fn slippage_max_in(amount_in_quote: String, slippage_bps: u32) -> CmdResult<String> {
    let q: u128 = amount_in_quote
        .parse()
        .map_err(|_| CmdError::InvalidInput("amount_in_quote not a u128".into()))?;
    atlas_slippage::max_in(q, slippage_bps)
        .map(|v| v.to_string())
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Compute the absolute deadline timestamp (unix seconds).
#[tauri::command]
#[specta::specta]
pub async fn slippage_deadline_unix(now_unix: u64, deadline_secs: u64) -> CmdResult<u64> {
    atlas_slippage::deadline_unix(now_unix, deadline_secs)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

// ---- presentation formatters --------------------------------------

/// Locale-aware currency formatter (e.g. "$1,234.56").
#[tauri::command]
#[specta::specta]
pub async fn fmt_currency(
    amount: f64,
    code: String,
    locale_tag: String,
    decimals: u32,
) -> CmdResult<String> {
    Ok(atlas_fmt::format_currency(
        amount,
        &code,
        &locale_tag,
        decimals,
    ))
}

/// Compact number formatter (e.g. 1234567 -> "1.23M").
#[tauri::command]
#[specta::specta]
pub async fn fmt_compact(value: f64) -> CmdResult<String> {
    Ok(atlas_fmt::format_compact(value))
}

/// Format a base-unit decimal string into a human token amount.
#[tauri::command]
#[specta::specta]
pub async fn fmt_token_amount(
    base_units: String,
    decimals: u32,
    max_significant: u32,
) -> CmdResult<String> {
    atlas_fmt::format_token_amount(&base_units, decimals, max_significant)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Shorten a 0x address to "0x1234\u{2026}abcd" form.
#[tauri::command]
#[specta::specta]
pub async fn fmt_truncate_address(addr: String) -> CmdResult<String> {
    Ok(atlas_fmt::truncate_address(&addr))
}

// ---- per-tx notes & tags ------------------------------------------

/// List all stored tx-notes.
#[tauri::command]
#[specta::specta]
pub async fn txnotes_list(
    state: State<'_, Arc<AppState>>,
) -> CmdResult<Vec<atlas_txnotes::TxNote>> {
    Ok(state.txnotes.read().await.list())
}

/// Get a single note by chain + txid (case-insensitive).
#[tauri::command]
#[specta::specta]
pub async fn txnotes_get(
    state: State<'_, Arc<AppState>>,
    chain: atlas_txnotes::Chain,
    txid: String,
) -> CmdResult<Option<atlas_txnotes::TxNote>> {
    Ok(state.txnotes.read().await.get(chain, &txid).cloned())
}

/// Insert or replace a tx-note. Empty note + empty tags removes it.
#[tauri::command]
#[specta::specta]
pub async fn txnotes_upsert(
    state: State<'_, Arc<AppState>>,
    chain: atlas_txnotes::Chain,
    txid: String,
    note: String,
    tags: Vec<String>,
) -> CmdResult<()> {
    {
        let mut store = state.txnotes.write().await;
        store
            .upsert(chain, &txid, &note, &tags)
            .map_err(|e| CmdError::InvalidInput(e.to_string()))?;
    }
    persist_txnotes(&state).await
}

/// Remove a tx-note. Returns whether one existed.
#[tauri::command]
#[specta::specta]
pub async fn txnotes_remove(
    state: State<'_, Arc<AppState>>,
    chain: atlas_txnotes::Chain,
    txid: String,
) -> CmdResult<bool> {
    let removed = {
        let mut store = state.txnotes.write().await;
        store.remove(chain, &txid)
    };
    if removed {
        persist_txnotes(&state).await?;
    }
    Ok(removed)
}

/// List notes that carry a given tag (case-insensitive).
#[tauri::command]
#[specta::specta]
pub async fn txnotes_list_by_tag(
    state: State<'_, Arc<AppState>>,
    tag: String,
) -> CmdResult<Vec<atlas_txnotes::TxNote>> {
    Ok(state.txnotes.read().await.list_by_tag(&tag))
}

/// Distinct, sorted list of every tag used across all notes.
#[tauri::command]
#[specta::specta]
pub async fn txnotes_all_tags(state: State<'_, Arc<AppState>>) -> CmdResult<Vec<String>> {
    Ok(state.txnotes.read().await.all_tags())
}

async fn persist_txnotes(state: &Arc<AppState>) -> CmdResult<()> {
    let snapshot = state.txnotes.read().await.clone();
    crate::state::save_json(&state.data_dir, "txnotes.json", &snapshot)
        .map_err(|e| CmdError::InvalidInput(format!("persist txnotes: {e}")))
}

// ---- contacts (address book) --------------------------------------

/// All contacts, ordered by name.
#[tauri::command]
#[specta::specta]
pub async fn contacts_list(
    state: State<'_, Arc<AppState>>,
) -> CmdResult<Vec<atlas_contacts::Contact>> {
    Ok(state.contacts.read().await.list())
}

/// Get one contact by uuid.
#[tauri::command]
#[specta::specta]
pub async fn contacts_get(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> CmdResult<Option<atlas_contacts::Contact>> {
    Ok(state.contacts.read().await.get(&id).cloned())
}

/// Add a new contact and return the assigned uuid.
#[tauri::command]
#[specta::specta]
pub async fn contacts_add(
    state: State<'_, Arc<AppState>>,
    name: String,
    note: String,
    addresses: Vec<atlas_contacts::ContactAddress>,
) -> CmdResult<String> {
    let id = {
        let mut book = state.contacts.write().await;
        book.add(&name, &note, &addresses)
            .map_err(|e| CmdError::InvalidInput(e.to_string()))?
    };
    persist_contacts(&state).await?;
    Ok(id)
}

/// Update an existing contact.
#[tauri::command]
#[specta::specta]
pub async fn contacts_update(
    state: State<'_, Arc<AppState>>,
    id: String,
    name: String,
    note: String,
    addresses: Vec<atlas_contacts::ContactAddress>,
) -> CmdResult<()> {
    {
        let mut book = state.contacts.write().await;
        book.update(&id, &name, &note, &addresses)
            .map_err(|e| CmdError::InvalidInput(e.to_string()))?;
    }
    persist_contacts(&state).await
}

/// Remove a contact. Returns whether one existed.
#[tauri::command]
#[specta::specta]
pub async fn contacts_remove(state: State<'_, Arc<AppState>>, id: String) -> CmdResult<bool> {
    let removed = {
        let mut book = state.contacts.write().await;
        book.remove(&id)
    };
    if removed {
        persist_contacts(&state).await?;
    }
    Ok(removed)
}

/// Find the contact (if any) that owns this `chain`+`address`.
#[tauri::command]
#[specta::specta]
pub async fn contacts_find_by_address(
    state: State<'_, Arc<AppState>>,
    chain: atlas_contacts::ContactChain,
    address: String,
) -> CmdResult<Option<atlas_contacts::Contact>> {
    Ok(state
        .contacts
        .read()
        .await
        .find_by_address(chain, &address)
        .cloned())
}

/// Case-insensitive substring search across name + note.
#[tauri::command]
#[specta::specta]
pub async fn contacts_search(
    state: State<'_, Arc<AppState>>,
    query: String,
) -> CmdResult<Vec<atlas_contacts::Contact>> {
    Ok(state.contacts.read().await.search(&query))
}

async fn persist_contacts(state: &Arc<AppState>) -> CmdResult<()> {
    let snapshot = state.contacts.read().await.clone();
    crate::state::save_json(&state.data_dir, "contacts.json", &snapshot)
        .map_err(|e| CmdError::InvalidInput(format!("persist contacts: {e}")))
}

// ---- in-memory event log ------------------------------------------

/// Append a redactable event to the in-memory ring buffer.
#[tauri::command]
#[specta::specta]
pub async fn events_record(
    state: State<'_, Arc<AppState>>,
    timestamp_unix_ms: u64,
    level: atlas_eventlog::EventLevel,
    category: atlas_eventlog::EventCategory,
    message: String,
) -> CmdResult<()> {
    state
        .events
        .write()
        .await
        .record(timestamp_unix_ms, level, category, &message)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Most recent events (newest first).
#[tauri::command]
#[specta::specta]
pub async fn events_recent(
    state: State<'_, Arc<AppState>>,
    limit: u32,
) -> CmdResult<Vec<atlas_eventlog::EventRecord>> {
    Ok(state.events.read().await.recent(limit as usize))
}

/// Filter by min-level + optional category, newest first.
#[tauri::command]
#[specta::specta]
pub async fn events_filter(
    state: State<'_, Arc<AppState>>,
    min_level: atlas_eventlog::EventLevel,
    category: Option<atlas_eventlog::EventCategory>,
    limit: u32,
) -> CmdResult<Vec<atlas_eventlog::EventRecord>> {
    Ok(state
        .events
        .read()
        .await
        .filter(min_level, category, limit as usize))
}

/// Drop the entire event buffer.
#[tauri::command]
#[specta::specta]
pub async fn events_clear(state: State<'_, Arc<AppState>>) -> CmdResult<()> {
    state.events.write().await.clear();
    Ok(())
}

/// Privacy-redacted snapshot for "Report a problem".
#[tauri::command]
#[specta::specta]
pub async fn events_export_redacted(
    state: State<'_, Arc<AppState>>,
) -> CmdResult<Vec<atlas_eventlog::EventRecord>> {
    Ok(state.events.read().await.export_redacted())
}

// ---- spend limits -------------------------------------------------

/// Read the active spend-limit policy.
#[tauri::command]
#[specta::specta]
pub async fn spend_get_policy(
    state: State<'_, Arc<AppState>>,
) -> CmdResult<atlas_spendlimits::SpendPolicy> {
    Ok(state.spend.read().await.policy)
}

/// Replace the spend-limit policy. Validates before persisting.
#[tauri::command]
#[specta::specta]
pub async fn spend_set_policy(
    state: State<'_, Arc<AppState>>,
    policy: atlas_spendlimits::SpendPolicy,
) -> CmdResult<()> {
    policy
        .validate()
        .map_err(|e| CmdError::InvalidInput(e.to_string()))?;
    {
        let mut s = state.spend.write().await;
        s.policy = policy;
    }
    persist_spend(&state).await
}

/// Read the rolling daily-spend state.
#[tauri::command]
#[specta::specta]
pub async fn spend_get_state(
    state: State<'_, Arc<AppState>>,
) -> CmdResult<atlas_spendlimits::SpendState> {
    Ok(state.spend.read().await.state)
}

/// Evaluate a prospective tx against the active policy/state.
///
/// Read-only: the returned `next_state` is what the host should
/// pass to `spend_commit` only after the tx is actually broadcast.
#[tauri::command]
#[specta::specta]
pub async fn spend_evaluate(
    state: State<'_, Arc<AppState>>,
    now_unix: u64,
    attempt_usd: u64,
) -> CmdResult<atlas_spendlimits::LimitEvaluation> {
    let store = state.spend.read().await;
    atlas_spendlimits::evaluate(&store.policy, &store.state, now_unix, attempt_usd)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Persist the `next_state` produced by `spend_evaluate` after a
/// successful broadcast.
#[tauri::command]
#[specta::specta]
pub async fn spend_commit(
    state: State<'_, Arc<AppState>>,
    next_state: atlas_spendlimits::SpendState,
) -> CmdResult<()> {
    {
        let mut s = state.spend.write().await;
        s.state = next_state;
    }
    persist_spend(&state).await
}

/// Reset the rolling spend state to zero (keeps the policy).
#[tauri::command]
#[specta::specta]
pub async fn spend_reset(state: State<'_, Arc<AppState>>) -> CmdResult<()> {
    {
        let mut s = state.spend.write().await;
        s.state = atlas_spendlimits::SpendState::default();
    }
    persist_spend(&state).await
}

async fn persist_spend(state: &Arc<AppState>) -> CmdResult<()> {
    let snapshot = state.spend.read().await.clone();
    crate::state::save_json(&state.data_dir, "spend.json", &snapshot)
        .map_err(|e| CmdError::InvalidInput(format!("persist spend: {e}")))
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
// Jupiter (Solana) — read-only quote + unsigned swap transaction.
// =============================================================================
//
// Atlas signs Solana transactions itself, so the swap path is:
//   1. Frontend asks for a quote.
//   2. Frontend asks for a swap-tx.
//   3. Atlas decodes the base-64 versioned transaction, signs
//      with the active hot wallet's Solana key, broadcasts via
//      the configured Solana RPC.
//
// Step 3 lives behind a follow-up task because it needs Solana
// VersionedTransaction parsing in atlas-chain-solana. This commit
// ships the read-only halves so the UI can show quotes and let
// the user pick a route.

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct JupiterQuoteArgs {
    /// SPL mint address of the input token (use the wrapped-SOL
    /// mint for native SOL).
    pub input_mint: String,
    /// SPL mint address of the output token.
    pub output_mint: String,
    /// Amount in input-mint base units (string-encoded u64).
    pub amount: String,
    /// Slippage in basis points (100 = 1%). Capped at 5000.
    pub slippage_bps: u32,
    /// Optional Atlas platform fee in basis points (max 50).
    pub platform_fee_bps: Option<u32>,
}

/// Fetch a Jupiter v6 quote for a Solana swap.
#[tauri::command]
#[specta::specta]
pub async fn jupiter_quote(
    args: JupiterQuoteArgs,
) -> CmdResult<atlas_exchange_jupiter::JupiterQuote> {
    let client = atlas_exchange_jupiter::JupiterClient::new();
    let req = atlas_exchange_jupiter::QuoteRequest {
        input_mint: args.input_mint,
        output_mint: args.output_mint,
        amount: args.amount,
        slippage_bps: args.slippage_bps,
        platform_fee_bps: args.platform_fee_bps,
    };
    client
        .quote(&req)
        .await
        .map_err(|e| CmdError::Chain(e.to_string()))
}

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct JupiterSwapArgs {
    /// Quote returned by `jupiter_quote`.
    pub quote: atlas_exchange_jupiter::JupiterQuote,
    /// Solana base-58 public key of the wallet paying for the
    /// swap. The frontend pulls this from `get_address`.
    pub user_public_key: String,
    /// Wrap / unwrap SOL automatically when the input or output
    /// is the wrapped-SOL mint. Almost always `true` for end
    /// users.
    pub wrap_and_unwrap_sol: bool,
    /// Optional Atlas referral fee account (SPL token account
    /// of Atlas's referral PDA on the output mint).
    pub fee_account: Option<String>,
}

/// Build an unsigned Jupiter swap transaction for the supplied
/// quote. Returns a base-64 encoded versioned transaction the
/// caller must sign with their Solana key and broadcast through
/// `atlas-chain-solana`.
#[tauri::command]
#[specta::specta]
pub async fn jupiter_swap(
    args: JupiterSwapArgs,
) -> CmdResult<atlas_exchange_jupiter::SwapTransaction> {
    let client = atlas_exchange_jupiter::JupiterClient::new();
    client
        .swap_with_fee(
            &args.quote,
            &args.user_public_key,
            args.wrap_and_unwrap_sol,
            args.fee_account,
        )
        .await
        .map_err(|e| CmdError::Chain(e.to_string()))
}

// =============================================================================
// THORChain (cross-chain native swaps) — read-only quote.
// =============================================================================
//
// THORChain doesn't return an unsigned transaction; the wallet
// builds and signs the deposit on the source chain itself, using
// the inbound_address + memo from the quote. The signing path
// reuses atlas-chain-bitcoin / atlas-chain-evm / etc. directly.

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct ThorchainQuoteArgs {
    /// Source asset in THORChain notation (e.g. "BTC.BTC",
    /// "ETH.USDC-0xa0b8...eb48").
    pub from_asset: String,
    /// Destination asset.
    pub to_asset: String,
    /// Amount in 1e8-fixed-point base units (string-encoded).
    pub amount: String,
    /// Recipient address on the destination chain.
    pub destination: String,
    /// Optional affiliate THORName.
    pub affiliate: Option<String>,
    /// Optional affiliate basis points (0..=1000).
    pub affiliate_bps: Option<u32>,
    /// Optional minimum acceptable output (slippage protection).
    pub min_amount_out: Option<String>,
}

/// Fetch a THORChain swap quote. The returned `inbound_address`
/// + `memo` are what the wallet then sends to on the source
/// chain.
#[tauri::command]
#[specta::specta]
pub async fn thorchain_quote(
    args: ThorchainQuoteArgs,
) -> CmdResult<atlas_exchange_thorchain::ThorchainQuote> {
    let client = atlas_exchange_thorchain::ThorchainClient::new();
    let req = atlas_exchange_thorchain::ThorchainQuoteRequest {
        from_asset: args.from_asset,
        to_asset: args.to_asset,
        amount: args.amount,
        destination: args.destination,
        affiliate: args.affiliate,
        affiliate_bps: args.affiliate_bps,
        min_amount_out: args.min_amount_out,
    };
    client
        .quote(&req)
        .await
        .map_err(|e| CmdError::Chain(e.to_string()))
}

// =============================================================================
// ChangeNOW (non-custodial multi-chain swap aggregator).
// =============================================================================
//
// Two-call flow:
//   1. estimate     — open, no API key required.
//   2. create       — requires an API key (partner program).
// The created exchange returns a deposit address; the wallet
// signs and broadcasts the deposit using the existing per-chain
// code paths.

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct ChangeNowEstimateArgs {
    /// Source ticker, lowercase (e.g. "btc").
    pub from_currency: String,
    /// Destination ticker.
    pub to_currency: String,
    /// Optional source-network override (USDT etc.).
    pub from_network: Option<String>,
    /// Optional destination-network override.
    pub to_network: Option<String>,
    /// Source amount as a decimal string in display units.
    pub from_amount: String,
    /// "standard" (floating rate) or "fixed-rate".
    pub flow: String,
}

/// Read-only ChangeNOW estimate.
#[tauri::command]
#[specta::specta]
pub async fn changenow_estimate(
    args: ChangeNowEstimateArgs,
) -> CmdResult<atlas_exchange_changenow::Estimate> {
    let client = atlas_exchange_changenow::ChangeNowClient::new();
    let req = atlas_exchange_changenow::EstimateRequest {
        from_currency: args.from_currency,
        to_currency: args.to_currency,
        from_network: args.from_network,
        to_network: args.to_network,
        from_amount: args.from_amount,
        flow: args.flow,
    };
    client
        .estimate(&req)
        .await
        .map_err(|e| CmdError::Chain(e.to_string()))
}

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct ChangeNowCreateArgs {
    /// Source ticker.
    pub from_currency: String,
    /// Destination ticker.
    pub to_currency: String,
    /// Optional source-network override.
    pub from_network: Option<String>,
    /// Optional destination-network override.
    pub to_network: Option<String>,
    /// Source amount (decimal string, display unit).
    pub from_amount: String,
    /// User's destination address.
    pub address: String,
    /// Optional memo / destination tag (XRP, XLM, ...).
    pub extra_id: Option<String>,
    /// Optional refund address.
    pub refund_address: Option<String>,
    /// "standard" or "fixed-rate".
    pub flow: String,
    /// rateId from a fixed-rate estimate. Required for fixed-rate.
    pub rate_id: Option<String>,
    /// ChangeNOW partner-program API key. Required.
    pub api_key: String,
}

/// Create a ChangeNOW exchange transaction. Returns the deposit
/// address the wallet must send the source asset to.
#[tauri::command]
#[specta::specta]
pub async fn changenow_create(
    args: ChangeNowCreateArgs,
) -> CmdResult<atlas_exchange_changenow::CreatedExchange> {
    if args.api_key.is_empty() {
        return Err(CmdError::InvalidInput(
            "ChangeNOW api_key is required".into(),
        ));
    }
    let client = atlas_exchange_changenow::ChangeNowClient::with_api_key(args.api_key);
    let req = atlas_exchange_changenow::CreateExchangeRequest {
        from_currency: args.from_currency,
        to_currency: args.to_currency,
        from_network: args.from_network,
        to_network: args.to_network,
        from_amount: args.from_amount,
        address: args.address,
        extra_id: args.extra_id,
        refund_address: args.refund_address,
        flow: args.flow,
        rate_id: args.rate_id,
    };
    client
        .create_exchange(&req)
        .await
        .map_err(|e| CmdError::Chain(e.to_string()))
}

// =============================================================================
// Multi-source quote routing.
// =============================================================================
//
// Dispatches a single normalized swap intent to every applicable
// provider in parallel and returns the ranked list. The frontend
// picks one route and then calls the provider-specific
// build/sign endpoint with the route's `raw` payload.

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct RouteQuotesArgs {
    /// Routing request — see atlas_exchange_router::RoutingRequest.
    pub request: atlas_exchange_router::RoutingRequest,
}

/// Return ranked quotes from every applicable provider. Failures
/// are surfaced as entries with `out_amount = None` and an
/// `error` message; they sort to the bottom.
#[tauri::command]
#[specta::specta]
pub async fn route_quotes(
    state: State<'_, Arc<AppState>>,
    args: RouteQuotesArgs,
) -> CmdResult<Vec<atlas_exchange_router::RoutedQuote>> {
    let mut req = args.request;
    // If the caller didn't supply an explicit fee config, inject
    // Atlas's defaults from settings so every routed quote
    // already includes Atlas's affiliate / platform-fee share.
    if req.fee.is_none() {
        let bps = state.settings.swap_fee_bps();
        let thor_aff = state.settings.thorchain_affiliate();
        if bps > 0 || thor_aff.is_some() {
            req.fee = Some(atlas_exchange_router::SwapFee {
                jupiter_platform_fee_bps: if bps > 0 { Some(bps.min(50)) } else { None },
                thorchain_affiliate: thor_aff,
                thorchain_affiliate_bps: if bps > 0 { Some(bps.min(1_000)) } else { None },
            });
        }
    }
    Ok(atlas_exchange_router::route(&req).await)
}

// =============================================================================
// Internal helpers
// =============================================================================

async fn require_mnemonic(state: &AppState) -> CmdResult<Arc<Mnemonic>> {
    // If the active profile is hardware-backed, surface a distinct
    // error so the frontend routes signing through the device
    // instead of prompting for a password.
    {
        let reg = state.profiles.read().await;
        if let Some(active) = reg.active() {
            if let ProfileKind::Hardware { vendor, .. } = &active.kind {
                let v = match vendor {
                    atlas_profile::HardwareVendor::Ledger => "ledger",
                    atlas_profile::HardwareVendor::Trezor => "trezor",
                };
                return Err(CmdError::HardwareSignatureRequired(v.into()));
            }
        }
    }
    state.mnemonic.read().await.clone().ok_or(CmdError::Locked)
}

/// Returns an error if the active profile cannot sign at all
/// (watch-only) or requires hardware (handed back as
/// [`CmdError::HardwareSignatureRequired`] so the UI can route
/// to the device flow).
async fn require_signing_capable(state: &AppState) -> CmdResult<()> {
    let reg = state.profiles.read().await;
    let active = reg.active().ok_or_else(|| {
        CmdError::NotInitialized("no active profile — create or import a wallet first".into())
    })?;
    match &active.kind {
        ProfileKind::Hot { .. } => Ok(()),
        ProfileKind::Hardware { vendor, .. } => {
            let v = match vendor {
                atlas_profile::HardwareVendor::Ledger => "ledger",
                atlas_profile::HardwareVendor::Trezor => "trezor",
            };
            Err(CmdError::HardwareSignatureRequired(v.into()))
        }
        ProfileKind::WatchOnly { .. } => Err(CmdError::InvalidInput(
            "active profile is watch-only".into(),
        )),
    }
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
        ProfileKind::Hardware { .. } => Err(CmdError::InvalidInput(
            "active profile is hardware-backed; signing requires the device".into(),
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
        ProfileKind::Hardware { accounts, .. } => Ok(accounts
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

/// Per-chain row returned by `node_policy_audit_endpoints`. The
/// `decision` is `None` when the URL did not parse, when no
/// effective URL is configured, or when the chain has no provider.
#[derive(Debug, Serialize, specta::Type)]
pub struct NodePolicyAuditRow {
    pub chain_id: String,
    pub url: Option<String>,
    pub decision: Option<atlas_node_config::NodeDecision>,
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
    // Gate: refuse public-RPC URLs when the user has enabled
    // own-node-only mode. This is the single chokepoint where the
    // wallet would otherwise persist a third-party endpoint.
    {
        let policy = state.node_policy.read().await;
        let decision = atlas_node_config::endpoint_decision(&url, &policy)
            .map_err(|e| CmdError::InvalidInput(e.to_string()))?;
        if let atlas_node_config::NodeDecision::Block { host, reason } = decision {
            return Err(CmdError::InvalidInput(format!(
                "node policy blocked {host}: {reason}"
            )));
        }
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

// =============================================================================
// Address book (encrypted)
// =============================================================================

/// One decrypted address-book entry as the UI sees it.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AddressBookEntry {
    /// Atlas chain id (`"eth"`, `"btc"`, …).
    pub chain_id: String,
    /// Public address (plaintext).
    pub address: String,
    /// User-supplied display label (decrypted).
    pub label: String,
    /// Optional notes (decrypted).
    pub notes: Option<String>,
    /// Unix timestamp (seconds) when the entry was added.
    pub created_at: i64,
}

/// Domain-separation context for the address-book encryption key.
const ADDRESS_BOOK_CTX: &[u8] = b"atlas/address-book/v1";

#[derive(Debug, Serialize, Deserialize)]
struct AddressBookPlaintext {
    label: String,
    notes: Option<String>,
}

fn address_book_key(mnemonic: &Mnemonic) -> CmdResult<[u8; 32]> {
    let key = atlas_wallet_core::derive_app_key(mnemonic.seed(), ADDRESS_BOOK_CTX)
        .map_err(|e| CmdError::Wallet(e.to_string()))?;
    Ok(*key)
}

fn encrypt_entry(
    key: &[u8; 32],
    plaintext: &AddressBookPlaintext,
) -> CmdResult<([u8; 12], Vec<u8>)> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Key, Nonce};
    use rand::RngCore;

    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let pt = serde_json::to_vec(plaintext)
        .map_err(|e| CmdError::InvalidInput(format!("address-book encode: {e}")))?;
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), pt.as_ref())
        .map_err(|_| CmdError::Wallet("address-book encryption failed".into()))?;
    Ok((nonce, ct))
}

fn decrypt_entry(
    key: &[u8; 32],
    nonce: &[u8],
    ciphertext: &[u8],
) -> CmdResult<AddressBookPlaintext> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Key, Nonce};
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    if nonce.len() != 12 {
        return Err(CmdError::InvalidInput("address-book nonce length".into()));
    }
    let pt = cipher
        .decrypt(Nonce::from_slice(nonce), ciphertext)
        .map_err(|_| CmdError::Wallet("address-book decryption failed".into()))?;
    serde_json::from_slice(&pt)
        .map_err(|e| CmdError::InvalidInput(format!("address-book decode: {e}")))
}

/// Add (or replace) an encrypted address-book entry. Requires the
/// wallet to be unlocked because the encryption key is derived from
/// the BIP-39 seed.
#[tauri::command]
#[specta::specta]
pub async fn address_book_put(
    state: State<'_, Arc<AppState>>,
    chain_id: String,
    address: String,
    label: String,
    notes: Option<String>,
) -> CmdResult<()> {
    if address.trim().is_empty() {
        return Err(CmdError::InvalidInput("address required".into()));
    }
    if label.trim().is_empty() {
        return Err(CmdError::InvalidInput("label required".into()));
    }
    let mnemonic = require_mnemonic(&state).await?;
    let key = address_book_key(&mnemonic)?;
    let pt = AddressBookPlaintext {
        label: label.trim().into(),
        notes: notes
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
    };
    let (nonce, ct) = encrypt_entry(&key, &pt)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    state
        .db
        .address_book_put(&chain_id, address.trim(), &nonce, &ct, now)
        .await
        .map_err(|e| CmdError::Io(e.to_string()))?;
    Ok(())
}

/// List address-book entries (decrypted) for `chain_id`. Pass an empty
/// string to list across every chain. Requires the wallet to be
/// unlocked.
#[tauri::command]
#[specta::specta]
pub async fn address_book_list(
    state: State<'_, Arc<AppState>>,
    chain_id: String,
) -> CmdResult<Vec<AddressBookEntry>> {
    let mnemonic = require_mnemonic(&state).await?;
    let key = address_book_key(&mnemonic)?;
    let rows = state
        .db
        .address_book_list(&chain_id)
        .await
        .map_err(|e| CmdError::Io(e.to_string()))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let pt = decrypt_entry(&key, &row.nonce, &row.ciphertext)?;
        out.push(AddressBookEntry {
            chain_id: row.chain_id,
            address: row.address,
            label: pt.label,
            notes: pt.notes,
            created_at: row.created_at,
        });
    }
    Ok(out)
}

/// Remove one address-book entry. Returns `true` if a row was deleted.
/// Does NOT require the wallet to be unlocked — the row identifiers
/// (chain_id, address) are stored as plaintext.
#[tauri::command]
#[specta::specta]
pub async fn address_book_remove(
    state: State<'_, Arc<AppState>>,
    chain_id: String,
    address: String,
) -> CmdResult<bool> {
    let n = state
        .db
        .address_book_remove(&chain_id, &address)
        .await
        .map_err(|e| CmdError::Io(e.to_string()))?;
    Ok(n > 0)
}

// =============================================================================
// NFT viewer (watch-only, Reservoir API)
// =============================================================================

/// Atlas EVM chain ids for which the NFT viewer can fetch data.
#[tauri::command]
#[specta::specta]
pub async fn nft_supported_chains() -> CmdResult<Vec<String>> {
    Ok(atlas_nft_registry::SUPPORTED_CHAINS
        .iter()
        .map(|s| (*s).to_string())
        .collect())
}

/// Fetch NFTs that `address` owns on `chain_id`. `chain_id` must be
/// one of [`nft_supported_chains`]. Read-only — never signs or
/// transfers anything.
#[tauri::command]
#[specta::specta]
pub async fn nft_list_owned(
    chain_id: String,
    address: String,
) -> CmdResult<Vec<atlas_nft_registry::OwnedNft>> {
    if address.trim().is_empty() {
        return Err(CmdError::InvalidInput("address required".into()));
    }
    if !is_valid_evm_address(address.trim()) {
        return Err(CmdError::InvalidInput("expected EVM address".into()));
    }
    atlas_nft_registry::fetch_owned_nfts(&chain_id, address.trim(), None)
        .await
        .map_err(|e| CmdError::Wallet(e.to_string()))
}

// =============================================================================
// Tor proxy + kill-switch
// =============================================================================
//
// Atlas routes outbound network traffic through Tor by default
// (mode = `Required`). The kill-switch — encoded in
// [`atlas_tor::enforce`] — refuses to send a request when Tor is
// unavailable rather than leaking the user's IP onto clearnet.
//
// These commands let the UI inspect the current status, change the
// posture, and restart circuits. The actual proxy enforcement
// happens inside each HTTP client wrapper that consults
// [`atlas_tor::enforce`] before issuing a request.

async fn persist_tor(state: &Arc<AppState>) -> Result<(), CmdError> {
    let snapshot = crate::state::TorPersisted {
        mode: *state.tor.mode.read().await,
        config: state.tor.config.read().await.clone(),
    };
    crate::state::save_json(&state.data_dir, "tor.json", &snapshot)
        .map_err(|e| CmdError::Io(e.to_string()))
}

/// Cheap snapshot of the embedded Tor client's lifecycle.
#[tauri::command]
#[specta::specta]
pub async fn tor_status(state: State<'_, Arc<AppState>>) -> CmdResult<atlas_tor::TorStatus> {
    Ok(state.tor.provider.status().await)
}

/// Persisted user-selected mode (Disabled / Preferred / Required).
#[tauri::command]
#[specta::specta]
pub async fn tor_get_mode(state: State<'_, Arc<AppState>>) -> CmdResult<atlas_tor::TorMode> {
    Ok(*state.tor.mode.read().await)
}

/// Update the Tor posture and persist it. Does not start or stop
/// the provider — the host calls [`tor_start`] / [`tor_stop`]
/// explicitly so the UI can show bootstrap progress.
#[tauri::command]
#[specta::specta]
pub async fn tor_set_mode(
    state: State<'_, Arc<AppState>>,
    mode: atlas_tor::TorMode,
) -> CmdResult<atlas_tor::TorMode> {
    {
        let mut m = state.tor.mode.write().await;
        *m = mode;
    }
    persist_tor(&state).await?;
    Ok(mode)
}

/// Current SOCKS listener address + bridges.
#[tauri::command]
#[specta::specta]
pub async fn tor_get_config(state: State<'_, Arc<AppState>>) -> CmdResult<atlas_tor::TorConfig> {
    Ok(state.tor.config.read().await.clone())
}

/// Replace the SOCKS / bridges configuration. Takes effect on the
/// next [`tor_start`].
#[tauri::command]
#[specta::specta]
pub async fn tor_set_config(
    state: State<'_, Arc<AppState>>,
    config: atlas_tor::TorConfig,
) -> CmdResult<atlas_tor::TorConfig> {
    if config.socks_addr.trim().is_empty() {
        return Err(CmdError::InvalidInput("socks_addr required".into()));
    }
    {
        let mut c = state.tor.config.write().await;
        *c = config.clone();
    }
    persist_tor(&state).await?;
    Ok(config)
}

/// Start the embedded Tor client. Idempotent; surfaces any
/// transport error verbatim.
#[tauri::command]
#[specta::specta]
pub async fn tor_start(state: State<'_, Arc<AppState>>) -> CmdResult<atlas_tor::TorStatus> {
    let cfg = state.tor.config.read().await.clone();
    state
        .tor
        .provider
        .start(cfg)
        .await
        .map_err(|e| CmdError::Wallet(e.to_string()))
}

/// Stop the embedded Tor client. With mode = Required this means
/// the kill-switch will start blocking outbound requests.
#[tauri::command]
#[specta::specta]
pub async fn tor_stop(state: State<'_, Arc<AppState>>) -> CmdResult<()> {
    state.tor.provider.stop().await;
    Ok(())
}

/// Force a brand-new circuit (analogous to "New Identity" in Tor
/// Browser). Requires [`atlas_tor::TorStatus::Ready`].
#[tauri::command]
#[specta::specta]
pub async fn tor_new_circuit(state: State<'_, Arc<AppState>>) -> CmdResult<()> {
    state
        .tor
        .provider
        .new_circuit()
        .await
        .map_err(|e| CmdError::Wallet(e.to_string()))
}

/// Run the kill-switch policy against the current mode + status +
/// config and return the decision. The frontend uses this to show
/// the user what would happen on the next outbound request without
/// actually issuing one.
#[tauri::command]
#[specta::specta]
pub async fn tor_enforce_decision(
    state: State<'_, Arc<AppState>>,
) -> CmdResult<atlas_tor::ProxyDecision> {
    let mode = *state.tor.mode.read().await;
    let status = state.tor.provider.status().await;
    let config = state.tor.config.read().await.clone();
    Ok(atlas_tor::enforce(mode, &status, &config))
}

// =============================================================================
// Coin control (UTXO labels + privacy-aware selection)
// =============================================================================
//
// Combining a KYC-tainted UTXO with a private one in the same
// transaction destroys the privacy of the private one through
// the common-input-ownership heuristic. These commands let the
// UI label UTXOs by provenance, ask whether a planned selection
// is privacy-safe, and have the wallet propose a single-bucket
// selection that funds a target value.

async fn persist_utxo_labels(state: &Arc<AppState>) -> Result<(), CmdError> {
    let snapshot = state.utxo_labels.read().await.clone();
    crate::state::save_json(&state.data_dir, "utxo_labels.json", &snapshot)
        .map_err(|e| CmdError::Io(e.to_string()))
}

/// All `(UtxoRef, UtxoLabel)` pairs currently stored, sorted by
/// `txid` then `vout`.
#[tauri::command]
#[specta::specta]
pub async fn coincontrol_label_list(
    state: State<'_, Arc<AppState>>,
) -> CmdResult<Vec<atlas_coincontrol::LabeledUtxoEntry>> {
    let store = state.utxo_labels.read().await;
    Ok(store
        .entries()
        .into_iter()
        .map(|(utxo, label)| atlas_coincontrol::LabeledUtxoEntry { utxo, label })
        .collect())
}

/// Insert or replace the label for `utxo`.
#[tauri::command]
#[specta::specta]
pub async fn coincontrol_label_upsert(
    state: State<'_, Arc<AppState>>,
    utxo: atlas_coincontrol::UtxoRef,
    label: atlas_coincontrol::UtxoLabel,
) -> CmdResult<()> {
    if utxo.txid.trim().is_empty() {
        return Err(CmdError::InvalidInput("txid required".into()));
    }
    {
        let mut store = state.utxo_labels.write().await;
        store.upsert(utxo, label);
    }
    persist_utxo_labels(&state).await
}

/// Remove the label for `utxo`. Returns `true` if a label was
/// present.
#[tauri::command]
#[specta::specta]
pub async fn coincontrol_label_remove(
    state: State<'_, Arc<AppState>>,
    utxo: atlas_coincontrol::UtxoRef,
) -> CmdResult<bool> {
    let removed = {
        let mut store = state.utxo_labels.write().await;
        store.remove(&utxo)
    };
    if removed {
        persist_utxo_labels(&state).await?;
    }
    Ok(removed)
}

/// Inspect a manually-built selection and return every privacy
/// concern that applies. Empty result = selection is internally
/// consistent.
#[tauri::command]
#[specta::specta]
pub async fn coincontrol_detect_mix(
    selected: Vec<atlas_coincontrol::LabeledUtxo>,
) -> CmdResult<Vec<atlas_coincontrol::MixWarning>> {
    Ok(atlas_coincontrol::detect_mix(&selected))
}

/// Ask the wallet to propose a single-bucket selection that funds
/// `target` (in base units). Strategy controls how UTXOs are
/// picked inside the bucket. Buckets are tried in this order:
/// Private → Anonymous → Unlabelled → Identifying.
#[tauri::command]
#[specta::specta]
pub async fn coincontrol_suggest_selection(
    target: u64,
    available: Vec<atlas_coincontrol::LabeledUtxo>,
    strategy: atlas_coincontrol::SelectionStrategy,
) -> CmdResult<atlas_coincontrol::Selection> {
    atlas_coincontrol::suggest_selection(target, &available, strategy)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

// =============================================================================
// Sovereign node policy (own-node-only mode, refuse public RPC)
// =============================================================================
//
// The user's brief was unambiguous: "ki prarasy qe ne cdo opsion apo
// kod te ketij projekti te jem i lidhur drejtperdrejt me blockchain,
// m pak fjale mos te jem i varur nga pale te treta." These commands
// expose the [`atlas_node_config::NodePolicy`] gate so the user can
// flip a switch and have the wallet refuse to construct any
// provider against a hosted RPC SaaS. `set_rpc_endpoint` consults
// the policy before persisting; the dedicated `node_policy_check_url`
// command lets the UI preview a decision without applying it.

async fn persist_node_policy(state: &Arc<AppState>) -> Result<(), CmdError> {
    let snapshot = state.node_policy.read().await.clone();
    crate::state::save_json(&state.data_dir, "node_policy.json", &snapshot)
        .map_err(|e| CmdError::Io(e.to_string()))
}

/// Current sovereign-node policy.
#[tauri::command]
#[specta::specta]
pub async fn node_policy_get(
    state: State<'_, Arc<AppState>>,
) -> CmdResult<atlas_node_config::NodePolicy> {
    Ok(state.node_policy.read().await.clone())
}

/// Replace the sovereign-node policy with `policy`. The trusted-host
/// list is canonicalised (trim, lowercase, dedupe) before being
/// persisted.
#[tauri::command]
#[specta::specta]
pub async fn node_policy_set(
    state: State<'_, Arc<AppState>>,
    policy: atlas_node_config::NodePolicy,
) -> CmdResult<atlas_node_config::NodePolicy> {
    let mut canonical = policy;
    let trusted = std::mem::take(&mut canonical.trusted_hosts);
    canonical.set_trusted_hosts(trusted);
    {
        let mut guard = state.node_policy.write().await;
        *guard = canonical;
    }
    persist_node_policy(&state).await?;
    Ok(state.node_policy.read().await.clone())
}

/// Run the policy against a candidate URL without applying it.
/// Useful for the settings card preview row.
#[tauri::command]
#[specta::specta]
pub async fn node_policy_check_url(
    state: State<'_, Arc<AppState>>,
    url: String,
) -> CmdResult<atlas_node_config::NodeDecision> {
    let policy = state.node_policy.read().await.clone();
    atlas_node_config::endpoint_decision(&url, &policy)
        .map_err(|e| CmdError::InvalidInput(e.to_string()))
}

/// Run the policy against every currently configured RPC endpoint
/// (default + override) and return the per-chain decision so the UI
/// can colour the list.
#[tauri::command]
#[specta::specta]
pub async fn node_policy_audit_endpoints(
    state: State<'_, Arc<AppState>>,
) -> CmdResult<Vec<NodePolicyAuditRow>> {
    let policy = state.node_policy.read().await.clone();
    let mut out = Vec::new();
    for id in crate::state::all_chain_ids() {
        let row = endpoint_for(&state, id);
        let url = row.effective_url.clone();
        let decision = match url.as_deref() {
            Some(u) => atlas_node_config::endpoint_decision(u, &policy).ok(),
            None => None,
        };
        out.push(NodePolicyAuditRow {
            chain_id: id.to_string(),
            url,
            decision,
        });
    }
    Ok(out)
}
