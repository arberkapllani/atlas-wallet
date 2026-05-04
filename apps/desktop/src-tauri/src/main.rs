// Atlas desktop entry point.
// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod db;
mod error;
mod events;
mod network_health;
mod state;

use std::sync::Arc;
use tauri::Manager;

/// Build the typed command surface. Extracted so both `main()` and the
/// `export_bindings` test can construct an identical builder.
fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![
            commands::vault_exists,
            commands::create_wallet,
            commands::import_wallet,
            commands::unlock_wallet,
            commands::lock_wallet,
            commands::is_unlocked,
            commands::list_chains,
            commands::list_tokens,
            commands::token_list_default_urls,
            commands::token_list_fetch,
            commands::token_add_custom,
            commands::token_list_custom,
            commands::token_remove_custom,
            commands::list_rpc_endpoints,
            commands::set_rpc_endpoint,
            commands::clear_rpc_endpoint,
            commands::get_address,
            commands::get_balance,
            commands::get_fee_options,
            commands::send_native,
            commands::send_token,
            commands::get_prices,
            commands::get_fiat_currency,
            commands::set_fiat_currency,
            commands::get_auto_lock_minutes,
            commands::set_auto_lock_minutes,
            commands::get_anti_phishing_phrase,
            commands::set_anti_phishing_phrase,
            commands::biometric_status,
            commands::set_biometric_unlock_enabled,
            commands::recovery_drill_status,
            commands::set_recovery_drill_interval_days,
            commands::record_recovery_drill_completed,
            commands::get_swap_fee_config,
            commands::set_swap_fee_bps,
            commands::set_thorchain_affiliate,
            commands::flashbots_protect_enabled,
            commands::set_flashbots_protect,
            commands::get_moonpay_config,
            commands::set_moonpay_config,
            commands::build_moonpay_buy_url,
            commands::build_moonpay_sell_url,
            commands::multisig_btc_build_descriptor,
            commands::multisig_btc_derive_address,
            commands::multisig_btc_psbt_summary,
            commands::multisig_btc_psbt_combine,
            commands::multisig_btc_psbt_finalize,
            commands::multisig_btc_build_invite,
            commands::multisig_btc_parse_invite,
            commands::multisig_evm_safe_tx_hash,
            commands::multisig_evm_pack_signatures,
            commands::aa_user_op_hash,
            commands::aa_encode_execute_calldata,
            commands::shamir_split,
            commands::shamir_combine,
            commands::wc_parse_uri,
            commands::wc_build_uri,
            commands::network_health,
            commands::network_health_all,
            commands::get_exchange_settings,
            commands::set_exchange_settings,
            commands::exchange_quote,
            commands::exchange_swap,
            commands::jupiter_quote,
            commands::jupiter_swap,
            commands::thorchain_quote,
            commands::changenow_estimate,
            commands::changenow_create,
            commands::route_quotes,
            commands::list_profiles,
            commands::active_profile,
            commands::active_signing_capability,
            commands::switch_profile,
            commands::rename_profile,
            commands::delete_profile,
            commands::create_watch_only_profile,
            commands::create_hardware_profile,
            commands::tx_history_list,
            commands::tx_history_set_status,
            commands::tx_history_record,
            commands::address_book_put,
            commands::address_book_list,
            commands::address_book_remove,
            commands::nft_supported_chains,
            commands::nft_list_owned,
        ])
        .events(tauri_specta::collect_events![
            events::WalletLockedEvent,
            events::TxRecordedEvent,
            events::TxStatusChangedEvent,
            events::BalanceUpdatedEvent,
        ])
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "Atlas=info,warn".into()),
        )
        .init();

    let builder = specta_builder();

    // Regenerate the TypeScript bindings on every debug app start so the
    // SvelteKit app stays in sync. The dedicated test below also writes
    // them, so CI can produce them without launching the GUI.
    #[cfg(debug_assertions)]
    builder
        .export(
            specta_typescript::Typescript::default()
                .header("// AUTO-GENERATED by tauri-specta — do not edit by hand.\n"),
            "../src/lib/bindings.ts",
        )
        .expect("failed to export specta typescript bindings");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("app_data_dir resolves on every supported OS");
            std::fs::create_dir_all(&data_dir).ok();
            let app_state = tauri::async_runtime::block_on(state::AppState::new(data_dir))
                .expect("failed to load profile registry / open database");
            let app_state = Arc::new(app_state);
            app.manage(app_state);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Atlas");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regenerates `apps/desktop/src/lib/bindings.ts` deterministically.
    /// Run via `cargo test -p atlas-desktop export_bindings`.
    #[test]
    fn export_bindings() {
        specta_builder()
            .export(
                specta_typescript::Typescript::default()
                    .header("// AUTO-GENERATED by tauri-specta — do not edit by hand.\n"),
                "../src/lib/bindings.ts",
            )
            .expect("export bindings");
    }
}
