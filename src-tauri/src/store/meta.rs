//! Small key/value table for engine state that must survive a restart (the
//! kill-switch trip day). Desktop-only; the paper runner has its own.

use rusqlite::{params, Connection, OptionalExtension};

pub fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
        .map_err(|_| "storeMigrationFailed".to_string())
}

pub fn get(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| r.get(0))
        .optional()
        .map_err(|_| "storeQueryFailed".to_string())
}

pub fn set(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map(|_| ())
    .map_err(|_| "storeWriteFailed".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_then_get_round_trips_and_overwrites() {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c).unwrap();
        assert_eq!(get(&c, "tripped_day").unwrap(), None);
        set(&c, "tripped_day", "1").unwrap();
        set(&c, "tripped_day", "2").unwrap();
        assert_eq!(get(&c, "tripped_day").unwrap().as_deref(), Some("2"));
    }

    /// SQLite's own (English) message never reaches the UI: stable codes.
    #[test]
    fn failures_are_stable_codes() {
        let c = Connection::open_in_memory().unwrap();
        // No migration: the table is missing.
        assert_eq!(get(&c, "k").unwrap_err(), "storeQueryFailed");
        assert_eq!(set(&c, "k", "v").unwrap_err(), "storeWriteFailed");
    }
}
