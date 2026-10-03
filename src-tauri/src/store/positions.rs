//! Open-position table. Positions used to live only in memory, so a restart
//! orphaned every open paper position: it never closed and never reached the
//! trade history. Rows are the position's JSON so management fields can grow
//! without a migration per field (the bot model's serde defaults absorb it).

use rusqlite::{params, Connection};

pub fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS open_positions (
            signal_id TEXT NOT NULL,
            bot_kind TEXT NOT NULL,
            body TEXT NOT NULL,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY (signal_id, bot_kind)
        );",
    )
    .map_err(|_| "storeMigrationFailed".to_string())
}

/// Insert on open, replace on state change.
pub fn upsert(
    conn: &Connection,
    signal_id: &str,
    bot_kind: &str,
    body: &str,
    updated_at: u64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO open_positions (signal_id, bot_kind, body, updated_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (signal_id, bot_kind) DO UPDATE SET body = ?3, updated_at = ?4",
        params![signal_id, bot_kind, body, updated_at],
    )
    .map(|_| ())
    .map_err(|_| "storeWriteFailed".to_string())
}

pub fn delete(conn: &Connection, signal_id: &str, bot_kind: &str) -> Result<(), String> {
    conn.execute(
        "DELETE FROM open_positions WHERE signal_id = ?1 AND bot_kind = ?2",
        params![signal_id, bot_kind],
    )
    .map(|_| ())
    .map_err(|_| "storeWriteFailed".to_string())
}

/// All persisted position bodies, oldest update first.
pub fn load_all(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare("SELECT body FROM open_positions ORDER BY updated_at ASC")
        .map_err(|_| "storeQueryFailed".to_string())?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|_| "storeQueryFailed".to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|_| "storeRowMalformed".to_string())
}
