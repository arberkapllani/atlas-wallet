// Atlas desktop entry point.
// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod error;
mod network_health;
mod state;

use std::sync::Arc;
use tauri::Manager;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "Atlas=info,warn".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("app_data_dir resolves on every supported OS");
            std::fs::create_dir_all(&data_dir).ok();
            let app_state =
                Arc::new(state::AppState::new(data_dir).expect("failed to load profile registry"));
            app.manage(app_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::vault_exists,
            commands::create_wallet,
            commands::import_wallet,
            commands::unlock_wallet,
            commands::lock_wallet,
            commands::is_unlocked,
            commands::list_chains,
            commands::list_tokens,
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
            commands::network_health,
            commands::network_health_all,
            commands::list_profiles,
            commands::active_profile,
            commands::switch_profile,
            commands::rename_profile,
            commands::delete_profile,
            commands::create_watch_only_profile,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Atlas");
}
