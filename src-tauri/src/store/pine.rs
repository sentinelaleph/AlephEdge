//! The Pine bot's action keys and results (docs/aleph-edge-pine-lite-spec.md
//! sections 6 and 11): one row per (script hash, symbol, bar close), claimed
//! BEFORE any order is sent, so a restart or a retry never acts twice on one
//! bar. The row then carries what happened (the result the screens list) and,
//! for a real entry whose outcome stayed unknown, what was sent (looked up by
//! its client id later). Rows are kept 30 days.

use std::path::Path;

use rusqlite::{params, Connection};

use super::StoreManager;

/// Rows older than this (by their claim time) are pruned on the next claim.
pub const KEEP_MS: u64 = 30 * 86_400_000;

pub fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS pine_actions (
            key TEXT PRIMARY KEY,
            script_hash TEXT NOT NULL,
            symbol TEXT NOT NULL,
            bar_close_ms INTEGER NOT NULL,
            created_ms INTEGER NOT NULL,
            counted INTEGER NOT NULL DEFAULT 0,
            body TEXT NOT NULL,
            pending TEXT
        );
        CREATE INDEX IF NOT EXISTS pine_actions_created ON pine_actions (created_ms);",
    )
    .map_err(|_| "storeMigrationFailed".to_string())
}

/// One claimed key.
#[derive(Debug, Clone, PartialEq)]
pub struct Claim<'a> {
    pub key: &'a str,
    pub script_hash: &'a str,
    pub symbol: &'a str,
    pub bar_close_ms: u64,
    pub now_ms: u64,
    pub body: &'a str,
}

/// Claims `key`: `true` when this call added it (act now), `false` when it
/// was already there (acted on, or being acted on: never again). Prunes rows
/// older than `KEEP_MS` in the same transaction.
pub fn claim(conn: &Connection, c: &Claim) -> Result<bool, String> {
    let tx = conn.unchecked_transaction().map_err(|_| "storeWriteFailed".to_string())?;
    let added = tx
        .execute(
            "INSERT OR IGNORE INTO pine_actions (key, script_hash, symbol, bar_close_ms, created_ms, body)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![c.key, c.script_hash, c.symbol, c.bar_close_ms as i64, c.now_ms as i64, c.body],
        )
        .map_err(|_| "storeWriteFailed".to_string())?;
    tx.execute("DELETE FROM pine_actions WHERE created_ms < ?1", params![c.now_ms.saturating_sub(KEEP_MS) as i64])
        .map_err(|_| "storeWriteFailed".to_string())?;
    tx.commit().map_err(|_| "storeWriteFailed".to_string())?;
    Ok(added == 1)
}

/// Writes the result of `key`: its body, whether an open leg was sent
/// (`counted`, the daily entry cap) and the pending entry, if any.
pub fn update(conn: &Connection, key: &str, body: &str, counted: bool, pending: Option<&str>) -> Result<(), String> {
    conn.execute("UPDATE pine_actions SET body = ?2, counted = ?3, pending = ?4 WHERE key = ?1", params![key, body, i64::from(counted), pending])
        .map(|_| ())
        .map_err(|_| "storeWriteFailed".to_string())
}

/// Bodies, newest claim first.
pub fn list(conn: &Connection, limit: u32) -> Result<Vec<String>, String> {
    let mut stmt = conn.prepare("SELECT body FROM pine_actions ORDER BY created_ms DESC, rowid DESC LIMIT ?1").map_err(|_| "storeQueryFailed".to_string())?;
    let rows = stmt.query_map(params![i64::from(limit)], |r| r.get(0)).map_err(|_| "storeQueryFailed".to_string())?;
    rows.collect::<Result<_, _>>().map_err(|_| "storeQueryFailed".to_string())
}

/// Entries sent since `since_ms` (claim time): the daily cap's count.
pub fn opens_since(conn: &Connection, since_ms: u64) -> Result<u32, String> {
    conn.query_row("SELECT COUNT(*) FROM pine_actions WHERE counted = 1 AND created_ms >= ?1", params![since_ms as i64], |r| r.get::<_, i64>(0))
        .map(|n| n.max(0) as u32)
        .map_err(|_| "storeQueryFailed".to_string())
}

/// (key, pending, body) of every row whose real entry is still unknown.
pub fn pending(conn: &Connection) -> Result<Vec<(String, String, String)>, String> {
    let mut stmt = conn
        .prepare("SELECT key, pending, body FROM pine_actions WHERE pending IS NOT NULL ORDER BY created_ms")
        .map_err(|_| "storeQueryFailed".to_string())?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(|_| "storeQueryFailed".to_string())?;
    rows.collect::<Result<_, _>>().map_err(|_| "storeQueryFailed".to_string())
}

impl StoreManager {
    pub fn pine_claim(&self, dir: &Path, c: &Claim) -> Result<bool, String> {
        self.with_conn(dir, |conn| claim(conn, c))
    }

    pub fn pine_update(&self, dir: &Path, key: &str, body: &str, counted: bool, pending: Option<&str>) -> Result<(), String> {
        self.with_conn(dir, |conn| update(conn, key, body, counted, pending))
    }

    pub fn pine_results(&self, dir: &Path, limit: u32) -> Result<Vec<String>, String> {
        self.with_conn(dir, |conn| list(conn, limit))
    }

    pub fn pine_opens_since(&self, dir: &Path, since_ms: u64) -> Result<u32, String> {
        self.with_conn(dir, |conn| opens_since(conn, since_ms))
    }

    pub fn pine_pending(&self, dir: &Path) -> Result<Vec<(String, String, String)>, String> {
        self.with_conn(dir, pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ae-pine-store-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn c<'a>(key: &'a str, now: u64) -> Claim<'a> {
        Claim { key, script_hash: "h", symbol: "BTCUSDT", bar_close_ms: 3_600_000, now_ms: now, body: "{}" }
    }

    // Spec section 6: one action per (hash, symbol, bar), across a restart.
    #[test]
    fn a_key_is_claimed_once_even_across_a_restart() {
        let dir = scratch();
        let now = 1_761_900_000_000;
        let store = StoreManager::new();
        assert!(store.pine_claim(&dir, &c("pine:h:BTCUSDT:3600000", now)).unwrap());
        assert!(!store.pine_claim(&dir, &c("pine:h:BTCUSDT:3600000", now + 4_000)).unwrap(), "a retry never acts again");
        store.pine_update(&dir, "pine:h:BTCUSDT:3600000", r#"{"outcome":"opened"}"#, true, None).unwrap();
        drop(store);
        // A new process on the same data folder.
        let again = StoreManager::new();
        assert!(!again.pine_claim(&dir, &c("pine:h:BTCUSDT:3600000", now + 60_000)).unwrap(), "restart: still claimed");
        assert!(again.pine_claim(&dir, &c("pine:h:ETHUSDT:3600000", now)).unwrap(), "another symbol is its own key");
        assert_eq!(again.pine_opens_since(&dir, now - 1).unwrap(), 1, "only the counted row");
        let rows = again.pine_results(&dir, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.contains(&r#"{"outcome":"opened"}"#.to_string()), "the result survives the restart");
        drop(again);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rows_are_kept_thirty_days_and_pending_rows_are_listed() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
        let t0 = 10 * KEEP_MS;
        assert!(claim(&conn, &c("old", t0)).unwrap());
        update(&conn, "old", "{}", false, Some(r#"{"x":1}"#)).unwrap();
        assert_eq!(pending(&conn).unwrap().len(), 1);
        assert!(claim(&conn, &c("new", t0 + KEEP_MS)).unwrap(), "exactly 30 days later: the old row stays");
        assert_eq!(list(&conn, 10).unwrap().len(), 2);
        assert!(claim(&conn, &c("newer", t0 + KEEP_MS + 1)).unwrap());
        assert_eq!(list(&conn, 10).unwrap().len(), 2, "past 30 days it is pruned");
        assert!(pending(&conn).unwrap().is_empty());
        update(&conn, "newer", "{}", false, None).unwrap();
    }
}
