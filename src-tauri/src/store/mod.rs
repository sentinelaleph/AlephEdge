//! Local SQLite store: trade history + metrics cache only — this
//! database NEVER holds keys, secrets, or anything vault-adjacent.

pub(crate) mod backtest;
pub mod bot_stats;
mod commands;
mod manual;
mod meta;
pub mod model;
pub(crate) mod positions;
pub(crate) mod restore_check;
pub(crate) mod skips;
pub(crate) mod strategy;
mod trades;

pub use commands::{skips_export_csv, trades_export_csv, trades_list, trades_stats, trades_stats_by_bot};

use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use model::{PnlStats, TradeRecord};

pub struct StoreManager {
    conn: Mutex<Option<Connection>>,
}

impl StoreManager {
    /// The strategy (DCA / Grid) tables of `edge.db`. The table code itself
    /// (`strategy.rs`) is Tauri- and manager-free so the headless bot farm can
    /// compile it; only this accessor ties it to the desktop's connection.
    pub fn strategy<T>(&self, dir: &Path, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
        self.with_conn(dir, f)
    }
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
            migrate_all(&conn)?;
            *guard = Some(conn);
        }
        f(guard.as_ref().expect("store conn"))
    }

    /// Why the store cannot be used now (it could not be opened or migrated,
    /// or a newer build wrote it), as an i18n code; None when it opens.
    pub fn open_error(&self, dir: &Path) -> Option<String> {
        self.with_conn(dir, |_| Ok(())).err()
    }

    /// Records a closed position's trade and deletes its open row in ONE
    /// transaction: both land or neither does. Two separate commits left a
    /// window (an updater exit, a shutdown) where the trade was saved and the
    /// position row was not, and the next start restored and closed it again.
    pub fn settle_close(&self, dir: &Path, trade: &TradeRecord, signal_id: &str, bot_kind: &str) -> Result<(), String> {
        self.with_conn(dir, |c| settle(c, trade, signal_id, bot_kind))
    }

    /// See `trades::is_settled`.
    pub fn is_settled(&self, dir: &Path, signal_id: &str, bot_kind: &str, opened_at: u64) -> Result<bool, String> {
        self.with_conn(dir, |c| trades::is_settled(c, signal_id, bot_kind, opened_at))
    }

    pub fn list_trades(&self, dir: &Path, limit: u32) -> Result<Vec<TradeRecord>, String> {
        self.with_conn(dir, |c| {
            let mut rows = trades::list(c, limit)?;
            manual::mark(c, &mut rows)?;
            Ok(rows)
        })
    }

    pub fn stats(&self, dir: &Path) -> Result<PnlStats, String> {
        self.with_conn(dir, |c| trades::stats(c, utc_day_start_ms()))
    }

    /// Stats per signal bot, over real, simulated or all trades.
    pub fn stats_by_bot(&self, dir: &Path, live: Option<bool>) -> Result<Vec<bot_stats::BotStats>, String> {
        self.with_conn(dir, |c| bot_stats::by_bot(c, utc_day_start_ms(), live))
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

    /// (signal_id, bot_kind, body) of every open-position row.
    pub fn load_positions(&self, dir: &Path) -> Result<Vec<(String, String, String)>, String> {
        self.with_conn(dir, positions::load_all_keyed)
    }

    pub fn export_csv(&self, dir: &Path) -> Result<String, String> {
        self.with_conn(dir, trades::export_csv)
    }

    /// `Ok(false)`: this (bot, signal, reason) was already logged.
    pub fn record_skip(&self, dir: &Path, row: &skips::SkipRow) -> Result<bool, String> {
        self.with_conn(dir, |c| skips::insert(c, row))
    }

    pub fn export_skips_csv(&self, dir: &Path) -> Result<String, String> {
        self.with_conn(dir, skips::export_csv)
    }

    /// Writes a consistent copy of `edge.db` to `dest` (which must not exist
    /// yet), through the store's own connection and under its lock: no write
    /// lands half in the copy. Never a raw file copy of a database in use.
    pub fn backup_into(&self, dir: &Path, dest: &Path) -> Result<(), String> {
        self.with_conn(dir, |c| vacuum_into(c, dest))
    }
}

/// The schema this build writes, stored in SQLite's `user_version` header.
/// 0 = written before versions existed (0.3.18 and older); this build reads
/// those and stamps them. Raise it with every migration that changes what an
/// older build could misread, and keep the migrations additive.
pub const SCHEMA_VERSION: i64 = 1;

/// Why this build may not open a file at `found` (pure): only a newer
/// build's schema is refused. Code `storeSchemaNewer|<found>`.
pub fn schema_refusal(found: i64) -> Option<String> {
    (found > SCHEMA_VERSION).then(|| format!("storeSchemaNewer|{found}"))
}

/// Brings `conn` to `SCHEMA_VERSION` in ONE transaction: every table and
/// column lands and the version is stamped, or nothing changes (a process
/// exit mid-migration leaves the old file as it was). A file a newer build
/// wrote is refused before anything is touched.
pub(crate) fn migrate_all(conn: &Connection) -> Result<(), String> {
    let found: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|_| "storeOpenFailed".to_string())?;
    if let Some(refusal) = schema_refusal(found) {
        return Err(refusal);
    }
    let tx = conn.unchecked_transaction().map_err(|_| "storeMigrationFailed".to_string())?;
    trades::migrate(&tx)?;
    meta::migrate(&tx)?;
    positions::migrate(&tx)?;
    skips::migrate(&tx)?;
    // Desktop-only strategy (DCA/Grid, paper) tables.
    strategy::migrate(&tx)?;
    // Desktop-only backtest runs (newest 50).
    backtest::migrate(&tx)?;
    // Desktop-only marks of hand-opened trades (Execute).
    manual::migrate(&tx)?;
    if found != SCHEMA_VERSION {
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(|_| "storeMigrationFailed".to_string())?;
    }
    tx.commit().map_err(|_| "storeMigrationFailed".to_string())
}

/// SQLite's `VACUUM INTO`: a complete, compacted copy of the open database
/// as one transaction's snapshot.
pub(crate) fn vacuum_into(conn: &Connection, dest: &Path) -> Result<(), String> {
    let target = dest.to_str().ok_or_else(|| "backupPathInvalid".to_string())?;
    conn.execute("VACUUM INTO ?1", [target])
        .map(|_| ())
        .map_err(|e| format!("backupDbFailed|{e}"))
}

impl Default for StoreManager {
    fn default() -> Self {
        Self::new()
    }
}

/// The trade INSERT and the open-row DELETE of one close, as one transaction
/// (the pattern of the strategy store's `persist_tick`).
fn settle(conn: &Connection, trade: &TradeRecord, signal_id: &str, bot_kind: &str) -> Result<(), String> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|_| "storeWriteFailed".to_string())?;
    trades::insert(&tx, trade)?;
    manual::record(&tx, trade)?;
    positions::delete(&tx, signal_id, bot_kind)?;
    tx.commit().map_err(|_| "storeWriteFailed".to_string())
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
mod fixtures_tests;
#[cfg(test)]
mod positions_tests;
#[cfg(test)]
mod trades_tests;
#[cfg(test)]
pub(crate) mod update_common;
#[cfg(test)]
mod update_gen;
#[cfg(test)]
mod update_persist_tests;
#[cfg(test)]
mod update_gen_live;
#[cfg(test)]
mod update_live_tests;
#[cfg(test)]
mod schema_tests;
