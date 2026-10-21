//! edge.db schema version (0.3.19): migrations run in one transaction and
//! stamp `user_version`; a file a newer build wrote is refused, untouched,
//! with a code the UI names.

use rusqlite::Connection;

use super::{migrate_all, schema_refusal, StoreManager, SCHEMA_VERSION};

fn scratch() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("ae-schema-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn version(c: &Connection) -> i64 {
    c.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap()
}

#[test]
fn only_a_newer_schema_is_refused() {
    assert_eq!(schema_refusal(0), None, "0.3.18 and older");
    assert_eq!(schema_refusal(SCHEMA_VERSION), None);
    assert_eq!(schema_refusal(SCHEMA_VERSION + 1), Some(format!("storeSchemaNewer|{}", SCHEMA_VERSION + 1)));
}

#[test]
fn a_fresh_or_older_file_is_migrated_and_stamped_and_a_rerun_changes_nothing() {
    let c = Connection::open_in_memory().unwrap();
    assert_eq!(version(&c), 0);
    migrate_all(&c).unwrap();
    assert_eq!(version(&c), SCHEMA_VERSION);
    let tables: i64 = c.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'", [], |r| r.get(0)).unwrap();
    migrate_all(&c).unwrap();
    let again: i64 = c.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'", [], |r| r.get(0)).unwrap();
    assert_eq!(tables, again);
    assert_eq!(version(&c), SCHEMA_VERSION);
    assert!(c.is_autocommit(), "no transaction left open");
}

#[test]
fn a_newer_file_is_refused_untouched_and_the_store_says_why() {
    let dir = scratch();
    {
        let c = Connection::open(dir.join("edge.db")).unwrap();
        c.execute_batch("CREATE TABLE future_only (x INTEGER); INSERT INTO future_only VALUES (7);").unwrap();
        c.pragma_update(None, "user_version", SCHEMA_VERSION + 1).unwrap();
    }
    let store = StoreManager::new();
    let want = format!("storeSchemaNewer|{}", SCHEMA_VERSION + 1);
    assert_eq!(store.open_error(&dir).as_deref(), Some(want.as_str()));
    assert_eq!(store.get_meta(&dir, "anything").unwrap_err(), want, "every read refuses");
    assert!(store.set_meta(&dir, "k", "v").is_err(), "and every write");
    drop(store);
    let c = Connection::open(dir.join("edge.db")).unwrap();
    assert_eq!(version(&c), SCHEMA_VERSION + 1, "the version is not rewritten");
    let tables: i64 = c.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'", [], |r| r.get(0)).unwrap();
    assert_eq!(tables, 1, "no table of this build was added");
    drop(c);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_failed_migration_leaves_the_file_as_it_was() {
    // A table a later migration indexes, already there without the column:
    // that migration fails, and the transaction undoes what the earlier ones
    // had done (no new table, no stamp).
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch("CREATE TABLE strategy_cycles (bot_id TEXT);").unwrap();
    assert!(migrate_all(&c).is_err());
    assert_eq!(version(&c), 0, "not stamped");
    let trades: i64 = c.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = 'trades'", [], |r| r.get(0)).unwrap();
    assert_eq!(trades, 0, "the trades table created before the failure was rolled back");
    assert!(c.is_autocommit(), "no transaction left open");
}
