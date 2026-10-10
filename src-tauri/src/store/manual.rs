//! Desktop-only record of the trades the user opened by hand on one signal
//! (Execute, "Open position"). Kept apart from `trades`: the VDS paper runner
//! compiles `trades.rs` and writes the same schema, and it never opens a
//! manual trade. A trade is keyed as `trades::is_settled` keys it.

use std::collections::HashSet;

use rusqlite::{params, Connection};

use super::model::TradeRecord;

pub fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS manual_trades (
            signal_id TEXT NOT NULL,
            bot_kind TEXT NOT NULL,
            opened_at INTEGER NOT NULL,
            PRIMARY KEY (signal_id, bot_kind, opened_at)
        );",
    )
    .map_err(|_| "storeMigrationFailed".to_string())
}

/// Marks a closed trade as manual (no-op for any other trade).
pub fn record(conn: &Connection, t: &TradeRecord) -> Result<(), String> {
    if !t.manual {
        return Ok(());
    }
    conn.execute(
        "INSERT OR IGNORE INTO manual_trades (signal_id, bot_kind, opened_at) VALUES (?1, ?2, ?3)",
        params![t.signal_id, t.bot_kind, t.opened_at as i64],
    )
    .map(|_| ())
    .map_err(|_| "storeWriteFailed".to_string())
}

/// Sets `manual` on every listed trade the table holds.
pub fn mark(conn: &Connection, rows: &mut [TradeRecord]) -> Result<(), String> {
    let mut stmt = conn
        .prepare("SELECT signal_id, bot_kind, opened_at FROM manual_trades")
        .map_err(|_| "storeQueryFailed".to_string())?;
    let keys: HashSet<(String, String, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(|_| "storeQueryFailed".to_string())?
        .collect::<Result<_, _>>()
        .map_err(|_| "storeRowMalformed".to_string())?;
    for t in rows.iter_mut() {
        t.manual = keys.contains(&(t.signal_id.clone(), t.bot_kind.clone(), t.opened_at as i64));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::engine::test_fixtures::{position, signal};

    #[test]
    fn a_manual_trade_is_marked_on_read_and_others_are_not() {
        let conn = Connection::open_in_memory().unwrap();
        super::super::trades::migrate(&conn).unwrap();
        migrate(&conn).unwrap();
        let mut pos = position(&signal("long", 100.0, 106.0, 90.0), None);
        let auto = crate::bot::engine::book::trade_record(&pos, 106.0, "tp", 9);
        pos.manual = true;
        pos.opened_at = 5;
        let manual = crate::bot::engine::book::trade_record(&pos, 90.0, "sl", 10);
        for t in [&auto, &manual] {
            super::super::trades::insert(&conn, t).unwrap();
            record(&conn, t).unwrap();
        }
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM manual_trades", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1, "only the manual trade is recorded");
        let mut rows = super::super::trades::list(&conn, 10).unwrap();
        mark(&conn, &mut rows).unwrap();
        let flags: Vec<(u64, bool)> = rows.iter().map(|t| (t.opened_at, t.manual)).collect();
        assert_eq!(flags, vec![(5, true), (0, false)]);
    }
}
