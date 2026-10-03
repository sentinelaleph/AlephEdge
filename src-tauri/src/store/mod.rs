//! Local SQLite store (PRD §3.1): trade history + metrics cache only — this
//! database NEVER holds keys, secrets, or anything vault-adjacent.

pub(crate) mod backtest;
mod commands;
mod meta;
pub mod model;
pub(crate) mod positions;
pub(crate) mod strategy;
mod trades;

pub use commands::{trades_export_csv, trades_list, trades_stats};

use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use model::{PnlStats, TradeRecord};

pub struct StoreManager {
    conn: Mutex<Option<Connection>>,
}

impl StoreManager {
    pub fn new() -> Self {
        Self {
            conn: Mutex::new(None),
        }
    }

    /// Opens (and migrates) `edge.db` under the app-data dir on first use.
    fn with_conn<T>(
        &self,
        dir: &Path,
        f: impl FnOnce(&Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut guard = self.conn.lock().expect("store mutex");
        if guard.is_none() {
            std::fs::create_dir_all(dir).map_err(|_| "storeDirFailed".to_string())?;
            let db = dir.join("edge.db");
            let conn = Connection::open(&db).map_err(|_| "storeOpenFailed".to_string())?;
            // This database holds no keys, but it holds every trade the user
            // ever made, and SQLite creates it at the umask default (0644 on
            // most Unix systems). Tighten it right after the open, which is the
            // only moment we know the file exists. Not atomic: SQLite created
            // the file, so there is a brief window at the old mode — acceptable
            // for a file whose worst case is a readable trade history, and the
            // alternative is pre-creating the file behind SQLite's back.
            //
            // A failure here is fatal rather than ignored. We have just created
            // this directory ourselves, so a chmod that fails means something is
            // wrong with the app-data path, and carrying on would only produce a
            // worse error one layer down — with a world-readable database.
            //
            // The rollback journal (`edge.db-journal`) is created and deleted by
            // SQLite per transaction and is not ours to chmod; on Unix it
            // inherits the umask. Worth knowing if you are reading this looking
            // for holes.
            crate::vault::secure_file::restrict_to_owner(&db)
                .map_err(|_| "storePermissionsFailed".to_string())?;
            trades::migrate(&conn)?;
            meta::migrate(&conn)?;
            positions::migrate(&conn)?;
            // Desktop-only strategy (DCA/Grid, paper) tables.
            strategy::migrate(&conn)?;
            // Desktop-only backtest runs (newest 50).
            backtest::migrate(&conn)?;
            *guard = Some(conn);
        }
        f(guard.as_ref().expect("store conn"))
    }

    pub fn record_trade(&self, dir: &Path, trade: &TradeRecord) -> Result<(), String> {
        self.with_conn(dir, |c| trades::insert(c, trade))
    }

    pub fn list_trades(&self, dir: &Path, limit: u32) -> Result<Vec<TradeRecord>, String> {
        self.with_conn(dir, |c| trades::list(c, limit))
    }

    pub fn stats(&self, dir: &Path) -> Result<PnlStats, String> {
        self.with_conn(dir, |c| trades::stats(c, utc_day_start_ms()))
    }

    /// Stats over real (`true`) or simulated (`false`) trades only.
    pub fn stats_for(&self, dir: &Path, live: bool) -> Result<PnlStats, String> {
        self.with_conn(dir, |c| trades::stats_scoped(c, utc_day_start_ms(), Some(live)))
    }

    pub fn upsert_position(
        &self,
        dir: &Path,
        signal_id: &str,
        bot_kind: &str,
        body: &str,
        updated_at: u64,
    ) -> Result<(), String> {
        self.with_conn(dir, |c| {
            positions::upsert(c, signal_id, bot_kind, body, updated_at)
        })
    }

    pub fn delete_position(
        &self,
        dir: &Path,
        signal_id: &str,
        bot_kind: &str,
    ) -> Result<(), String> {
        self.with_conn(dir, |c| positions::delete(c, signal_id, bot_kind))
    }

    pub fn get_meta(&self, dir: &Path, key: &str) -> Result<Option<String>, String> {
        self.with_conn(dir, |c| meta::get(c, key))
    }

    pub fn set_meta(&self, dir: &Path, key: &str, value: &str) -> Result<(), String> {
        self.with_conn(dir, |c| meta::set(c, key, value))
    }

    pub fn recent_trade_keys(
        &self,
        dir: &Path,
        since_ms: u64,
    ) -> Result<Vec<(String, String)>, String> {
        self.with_conn(dir, |c| trades::recent_keys(c, since_ms))
    }

    pub fn load_positions(&self, dir: &Path) -> Result<Vec<String>, String> {
        self.with_conn(dir, positions::load_all)
    }

    pub fn export_csv(&self, dir: &Path) -> Result<String, String> {
        self.with_conn(dir, trades::export_csv)
    }
}

impl Default for StoreManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Start of the current UTC day in UNIX millis (daily kill-switch window).
pub fn utc_day_start_ms() -> u64 {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    now_ms - (now_ms % 86_400_000)
}

#[cfg(test)]
mod positions_tests;
#[cfg(test)]
mod trades_tests;
