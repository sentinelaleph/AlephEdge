//! Signal skip log: every signal a bot saw and did not open, with the reason.
//! The in-memory feed (`BotManager::push_skip`) shows the newest notes while
//! the app runs; this table keeps them so "why did the bot not take X" can be
//! answered after a restart. Newest `KEEP` rows only.

use rusqlite::{params, Connection};

use super::trades::csv_field;

/// Rows kept; older ones are pruned on insert.
const KEEP: i64 = 20_000;

#[derive(Debug, Clone, PartialEq)]
pub struct SkipRow {
    pub at_ms: u64,
    pub bot_kind: String,
    pub signal_id: String,
    pub symbol: String,
    pub direction: String,
    pub market: String,
    pub combo: String,
    pub reason: String,
    pub detail: String,
}

pub fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS signal_skips (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            at_ms INTEGER NOT NULL,
            bot_kind TEXT NOT NULL,
            signal_id TEXT NOT NULL,
            symbol TEXT NOT NULL,
            direction TEXT NOT NULL,
            market TEXT NOT NULL,
            combo TEXT NOT NULL,
            reason TEXT NOT NULL,
            detail TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS signal_skips_signal ON signal_skips (signal_id);",
    )
    .map_err(|_| "storeMigrationFailed".to_string())?;
    // One row per (bot, signal, reason). The judge ledger is memory-only, so
    // every restart re-settled each pending signal and wrote its verdict
    // again: 113 of 259 testnet rows were such repeats, and the skip CSV
    // counted each restart. Older DBs are deduplicated first (the earliest
    // sighting is kept) or the unique index could not be built; on a clean
    // table the DELETE finds nothing.
    conn.execute_batch(
        "DELETE FROM signal_skips WHERE id NOT IN
            (SELECT MIN(id) FROM signal_skips GROUP BY bot_kind, signal_id, reason);
        CREATE UNIQUE INDEX IF NOT EXISTS signal_skips_once ON signal_skips (bot_kind, signal_id, reason);",
    )
    .map_err(|_| "storeMigrationFailed".to_string())
}

/// Writes the row unless this (bot, signal, reason) is already logged; the
/// first sighting's time and detail are kept. `true` when a row was added.
pub fn insert(conn: &Connection, r: &SkipRow) -> Result<bool, String> {
    let added = conn.execute(
        "INSERT OR IGNORE INTO signal_skips (at_ms, bot_kind, signal_id, symbol, direction, market, combo, reason, detail)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            r.at_ms as i64,
            r.bot_kind,
            r.signal_id,
            r.symbol,
            r.direction,
            r.market,
            r.combo,
            r.reason,
            r.detail
        ],
    )
    .map_err(|_| "storeWriteFailed".to_string())?;
    if added == 0 {
        return Ok(false);
    }
    conn.execute(
        "DELETE FROM signal_skips WHERE id <= (SELECT MAX(id) FROM signal_skips) - ?1",
        params![KEEP],
    )
    .map(|_| true)
    .map_err(|_| "storeWriteFailed".to_string())
}

pub fn list(conn: &Connection) -> Result<Vec<SkipRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT at_ms, bot_kind, signal_id, symbol, direction, market, combo, reason, detail
             FROM signal_skips ORDER BY id DESC",
        )
        .map_err(|_| "storeQueryFailed".to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(SkipRow {
                at_ms: r.get::<_, i64>(0)? as u64,
                bot_kind: r.get(1)?,
                signal_id: r.get(2)?,
                symbol: r.get(3)?,
                direction: r.get(4)?,
                market: r.get(5)?,
                combo: r.get(6)?,
                reason: r.get(7)?,
                detail: r.get(8)?,
            })
        })
        .map_err(|_| "storeQueryFailed".to_string())?;
    rows.collect::<Result<_, _>>().map_err(|_| "storeQueryFailed".to_string())
}

pub fn export_csv(conn: &Connection) -> Result<String, String> {
    let mut out = String::from("at_ms,bot_kind,signal_id,symbol,direction,market,combo,reason,detail\n");
    for r in list(conn)? {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            r.at_ms,
            csv_field(&r.bot_kind),
            csv_field(&r.signal_id),
            csv_field(&r.symbol),
            csv_field(&r.direction),
            csv_field(&r.market),
            csv_field(&r.combo),
            csv_field(&r.reason),
            csv_field(&r.detail),
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(n: u64) -> SkipRow {
        SkipRow {
            at_ms: n,
            bot_kind: "futures".into(),
            signal_id: format!("s{n}"),
            symbol: "TRUTHUSDT".into(),
            direction: "long".into(),
            market: "futures".into(),
            combo: String::new(),
            reason: "comboFiltered".into(),
            detail: "none".into(),
        }
    }

    #[test]
    fn rows_round_trip_newest_first_and_export() {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c).unwrap();
        assert!(insert(&c, &row(1)).unwrap());
        assert!(insert(&c, &row(2)).unwrap());
        let got = list(&c).unwrap();
        assert_eq!(got, vec![row(2), row(1)]);
        let csv = export_csv(&c).unwrap();
        assert!(csv.lines().nth(1).unwrap().ends_with(",comboFiltered,none"), "{csv}");
    }

    #[test]
    fn only_the_newest_rows_are_kept() {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c).unwrap();
        c.execute("INSERT INTO sqlite_sequence (name, seq) VALUES ('signal_skips', ?1)", params![KEEP + 5])
            .unwrap();
        insert(&c, &row(1)).unwrap();
        // ids start past KEEP; a row KEEP below the newest is pruned.
        c.execute(
            "INSERT INTO signal_skips (id, at_ms, bot_kind, signal_id, symbol, direction, market, combo, reason, detail)
             VALUES (1, 0, 'spot', 'old', 'X', 'long', 'spot', '', 'r', '')",
            [],
        )
        .unwrap();
        insert(&c, &row(2)).unwrap();
        assert!(list(&c).unwrap().iter().all(|r| r.signal_id != "old"));
    }
    // A restart re-settles every pending signal: the same verdict must not be
    // logged twice, and the first sighting's time is the one kept.
    #[test]
    fn the_same_verdict_is_logged_once() {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c).unwrap();
        assert!(insert(&c, &row(1)).unwrap());
        let mut again = row(1);
        again.at_ms = 99;
        assert!(!insert(&c, &again).unwrap(), "repeat is ignored");
        let mut other_reason = row(1);
        other_reason.reason = "tooOld".into();
        assert!(insert(&c, &other_reason).unwrap(), "a different reason is a new row");
        let mut other_bot = row(1);
        other_bot.bot_kind = "spot".into();
        assert!(insert(&c, &other_bot).unwrap(), "another bot is a new row");
        let got = list(&c).unwrap();
        assert_eq!(got.len(), 3);
        assert!(got.iter().all(|r| r.at_ms == 1), "first sighting kept");
        assert_eq!(export_csv(&c).unwrap().lines().count(), 4, "header + 3 rows");
    }

    // A DB written before the unique key holds restart repeats: the migration
    // keeps the earliest of each and is a no-op the second time.
    #[test]
    fn migration_dedupes_an_old_table() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE signal_skips (
                id INTEGER PRIMARY KEY AUTOINCREMENT, at_ms INTEGER NOT NULL, bot_kind TEXT NOT NULL,
                signal_id TEXT NOT NULL, symbol TEXT NOT NULL, direction TEXT NOT NULL,
                market TEXT NOT NULL, combo TEXT NOT NULL, reason TEXT NOT NULL, detail TEXT NOT NULL);",
        )
        .unwrap();
        for (at, reason) in [(1, "tooOld"), (2, "tooOld"), (3, "signalOtherMarket"), (4, "tooOld")] {
            c.execute(
                "INSERT INTO signal_skips (at_ms, bot_kind, signal_id, symbol, direction, market, combo, reason, detail)
                 VALUES (?1, 'futures', 's1', 'X', 'long', 'futures', '', ?2, '')",
                params![at, reason],
            )
            .unwrap();
        }
        migrate(&c).unwrap();
        let mut got: Vec<(u64, String)> = list(&c).unwrap().into_iter().map(|r| (r.at_ms, r.reason)).collect();
        got.sort();
        assert_eq!(got, vec![(1, "tooOld".to_string()), (3, "signalOtherMarket".to_string())]);
        migrate(&c).unwrap();
        assert_eq!(list(&c).unwrap().len(), 2);
        assert!(!insert(&c, &SkipRow { reason: "tooOld".into(), signal_id: "s1".into(), ..row(9) }).unwrap());
    }
}
