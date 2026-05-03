//! Typed Tauri events Atlas pushes to the SvelteKit frontend.
//!
//! Each event implements [`tauri_specta::Event`] which gives us a typed
//! emit/listen API on both sides (the TS bindings get a strongly-typed
//! `events.<eventName>.listen(...)` helper auto-generated alongside the
//! command surface).

use crate::db::TxRecord;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

/// Emitted when the wallet transitions from unlocked to locked, regardless
/// of cause (manual, idle timeout, profile switch, …). The frontend uses
/// this to evict in-memory state and route to `/unlock`.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct WalletLockedEvent {
    /// `"manual"`, `"idle"`, `"profile_switch"`, or `"shutdown"`.
    pub reason: String,
}

/// Emitted right after a tx is appended to the local cache (whether Atlas
/// signed it or the UI explicitly recorded an external one). The TX list
/// view subscribes and updates in place.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct TxRecordedEvent {
    pub record: TxRecord,
}

/// Emitted when [`crate::commands::tx_history_set_status`] flips a row's
/// status field (e.g. pending → confirmed once a watcher resolves).
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct TxStatusChangedEvent {
    pub chain_id: String,
    pub txid: String,
    pub status: String,
}

/// Emitted when a balance refresh observes a different value than what the
/// frontend last queried. Reserved for the upcoming background watcher —
/// kept in the schema now so listening UI code is forward-compatible.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct BalanceUpdatedEvent {
    pub chain_id: String,
    pub address: String,
    /// Base-units integer encoded as a decimal string for u128 safety.
    pub amount: String,
    pub asset: String,
}
