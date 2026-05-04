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
CREATE TABLE IF NOT EXISTS custom_tokens (
    chain_id     TEXT NOT NULL,
    contract     TEXT NOT NULL,
    symbol       TEXT NOT NULL,
    display_name TEXT NOT NULL,
    decimals     INTEGER NOT NULL,
    standard     TEXT NOT NULL,
    logo_uri     TEXT,
    PRIMARY KEY (chain_id, contract)
);
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

/// One user-imported token row. Mirrors `OwnedTokenMeta` minus the
/// derived `id` / `source` (which are always `"custom"` here).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, specta::Type)]
pub struct CustomToken {
    /// Atlas chain id (`"eth"`, `"polygon"`, …).
    pub chain_id: String,
    /// Contract address (case-preserved as the user typed it; lookups
    /// should compare case-insensitively).
    pub contract: String,
    /// On-chain ticker.
    pub symbol: String,
    /// Display name.
    pub display_name: String,
    /// Decimals (0–38).
    pub decimals: u8,
    /// `"erc-20"` or `"trc-20"`.
    pub standard: String,
    /// Optional logo URL.
    pub logo_uri: Option<String>,
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

    /// Insert (or replace) a custom user-imported token row.
    pub async fn add_custom_token(&self, t: &CustomToken) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"INSERT OR REPLACE INTO custom_tokens
               (chain_id, contract, symbol, display_name, decimals, standard, logo_uri)
               VALUES (?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(&t.chain_id)
        .bind(&t.contract)
        .bind(&t.symbol)
        .bind(&t.display_name)
        .bind(t.decimals as i64)
        .bind(&t.standard)
        .bind(&t.logo_uri)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// List custom tokens. Pass `chain_id = ""` to query across chains.
    pub async fn list_custom_tokens(
        &self,
        chain_id: &str,
    ) -> Result<Vec<CustomToken>, sqlx::Error> {
        if chain_id.is_empty() {
            sqlx::query_as::<_, CustomToken>(
                "SELECT chain_id, contract, symbol, display_name, decimals, standard, logo_uri
                 FROM custom_tokens ORDER BY symbol",
            )
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as::<_, CustomToken>(
                "SELECT chain_id, contract, symbol, display_name, decimals, standard, logo_uri
                 FROM custom_tokens WHERE chain_id = ? ORDER BY symbol",
            )
            .bind(chain_id)
            .fetch_all(&self.pool)
            .await
        }
    }

    /// Delete one custom-token row. Returns the number of rows affected
    /// (0 if the token wasn't there).
    pub async fn remove_custom_token(
        &self,
        chain_id: &str,
        contract: &str,
    ) -> Result<u64, sqlx::Error> {
        let res = sqlx::query("DELETE FROM custom_tokens WHERE chain_id = ? AND contract = ?")
            .bind(chain_id)
            .bind(contract)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected())
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

    #[tokio::test]
    async fn custom_token_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("atlas.db")).await.unwrap();
        let t = CustomToken {
            chain_id: "eth".into(),
            contract: "0xdeadbeef".into(),
            symbol: "FOO".into(),
            display_name: "Foo Token".into(),
            decimals: 18,
            standard: "erc-20".into(),
            logo_uri: None,
        };
        db.add_custom_token(&t).await.unwrap();
        // Idempotent.
        db.add_custom_token(&t).await.unwrap();
        let rows = db.list_custom_tokens("eth").await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].symbol, "FOO");
        let removed = db.remove_custom_token("eth", "0xdeadbeef").await.unwrap();
        assert_eq!(removed, 1);
        assert!(db.list_custom_tokens("").await.unwrap().is_empty());
    }
}
