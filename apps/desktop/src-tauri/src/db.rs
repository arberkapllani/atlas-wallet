//! Local SQLite database (sqlx) for Atlas.
//!
//! Currently stores a per-chain transaction history cache: every send/swap
//! issued by Atlas is recorded so the UI can show a recent-activity list
//! without re-scanning the chain on every startup. Future tables (address
//! book, NFT cache, contact labels…) live alongside it.

use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::Path;
use std::str::FromStr;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS tx_history (
    txid        TEXT NOT NULL,
    chain_id    TEXT NOT NULL,
    direction   TEXT NOT NULL,
    counterparty TEXT NOT NULL,
    amount      TEXT NOT NULL,
    asset       TEXT NOT NULL,
    fee         TEXT NOT NULL,
    timestamp   INTEGER NOT NULL,
    status      TEXT NOT NULL,
    memo        TEXT,
    PRIMARY KEY (chain_id, txid)
);
CREATE INDEX IF NOT EXISTS idx_tx_history_chain_ts
    ON tx_history(chain_id, timestamp DESC);
"#;

/// One on-chain transaction the user (or Atlas itself) initiated.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, specta::Type)]
pub struct TxRecord {
    /// Transaction id / hash, lower-case hex (no `0x` for BTC; with `0x` for EVM).
    pub txid: String,
    /// Chain id (`"btc"`, `"eth"`, `"sol"`, …).
    pub chain_id: String,
    /// `"send"` or `"receive"`. Atlas only writes `"send"` itself today;
    /// `"receive"` rows are reserved for future explorer-driven enrichment.
    pub direction: String,
    /// Other party's address.
    pub counterparty: String,
    /// Amount in base units, decimal-string (u128-friendly).
    pub amount: String,
    /// Asset ticker (`"BTC"`, `"ETH"`, `"USDT"`, …).
    pub asset: String,
    /// Network fee paid, decimal string in the chain's native asset base units.
    pub fee: String,
    /// Unix timestamp (seconds) when the tx was broadcast.
    pub timestamp: i64,
    /// `"pending"`, `"confirmed"`, or `"failed"`.
    pub status: String,
    /// Optional user-supplied note.
    pub memo: Option<String>,
}

/// Thin wrapper around a SQLite connection pool.
#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    /// Open (or create) the database file at `path`.
    pub async fn open(path: &Path) -> Result<Self, sqlx::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await?;
        sqlx::query(SCHEMA).execute(&pool).await?;
        Ok(Self { pool })
    }

    /// Insert (or replace) a tx_history row.
    pub async fn record_tx(&self, rec: &TxRecord) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"INSERT OR REPLACE INTO tx_history
               (txid, chain_id, direction, counterparty, amount, asset, fee, timestamp, status, memo)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(&rec.txid)
        .bind(&rec.chain_id)
        .bind(&rec.direction)
        .bind(&rec.counterparty)
        .bind(&rec.amount)
        .bind(&rec.asset)
        .bind(&rec.fee)
        .bind(rec.timestamp)
        .bind(&rec.status)
        .bind(&rec.memo)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Return up to `limit` most-recent rows for `chain_id` (newest first).
    /// Pass `chain_id = ""` to query across every chain.
    pub async fn list_history(
        &self,
        chain_id: &str,
        limit: i64,
    ) -> Result<Vec<TxRecord>, sqlx::Error> {
        if chain_id.is_empty() {
            sqlx::query_as::<_, TxRecord>(
                "SELECT txid, chain_id, direction, counterparty, amount, asset, fee, timestamp, status, memo
                 FROM tx_history ORDER BY timestamp DESC LIMIT ?",
            )
            .bind(limit)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as::<_, TxRecord>(
                "SELECT txid, chain_id, direction, counterparty, amount, asset, fee, timestamp, status, memo
                 FROM tx_history WHERE chain_id = ? ORDER BY timestamp DESC LIMIT ?",
            )
            .bind(chain_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
        }
    }

    /// Update the `status` column for a single tx (e.g. pending → confirmed).
    pub async fn update_status(
        &self,
        chain_id: &str,
        txid: &str,
        status: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE tx_history SET status = ? WHERE chain_id = ? AND txid = ?")
            .bind(status)
            .bind(chain_id)
            .bind(txid)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Drop every cached row. Used by the "wipe local data" UI action.
    #[allow(dead_code)]
    pub async fn wipe(&self) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM tx_history")
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn insert_and_list_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("atlas.db")).await.unwrap();
        let rec = TxRecord {
            txid: "abc".into(),
            chain_id: "btc".into(),
            direction: "send".into(),
            counterparty: "bc1q...".into(),
            amount: "1000".into(),
            asset: "BTC".into(),
            fee: "200".into(),
            timestamp: 1_700_000_000,
            status: "pending".into(),
            memo: None,
        };
        db.record_tx(&rec).await.unwrap();
        // Idempotent on duplicate primary key.
        db.record_tx(&rec).await.unwrap();
        let rows = db.list_history("btc", 10).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].txid, "abc");
        db.update_status("btc", "abc", "confirmed").await.unwrap();
        let rows = db.list_history("", 10).await.unwrap();
        assert_eq!(rows[0].status, "confirmed");
    }
}
