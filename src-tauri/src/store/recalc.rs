//! Desktop-only results of `bot::engine::recalc`: one row per checked paper
//! trade, keyed by the trade's rowid. A recalculated trade carries its new
//! exit; a kept one only its reason, so it is checked once. The `trades` row
//! is never rewritten (the bot farm shares that schema and never has this
//! table).
//!
//! The desk's history and its displayed stats read a recalculated trade's
//! figures; the kill switch keeps reading the rows as booked (`stats_for`),
//! so a recalculation never trips or untrips a stop after the fact.

use std::collections::HashMap;

use rusqlite::{params, Connection};

use super::bot_stats::BotStats;
use super::model::{PnlStats, TradeRecalc, TradeRecord};
use super::trades;

pub fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS trade_recalcs (
            trade_id INTEGER PRIMARY KEY,
            status TEXT NOT NULL,
            exit REAL,
            exit_reason TEXT,
            closed_at INTEGER,
            pnl_pct REAL,
            pnl_quote REAL,
            unlevered_net_pct REAL,
            checked_at INTEGER NOT NULL
        );",
    )
    .map_err(|_| "storeMigrationFailed".to_string())
}

/// Paper trades closed before `before_ms` that were never checked, newest
/// first. Only these can have missed a close (later ones ran the catch-up).
pub fn pending(conn: &Connection, before_ms: u64, limit: u32) -> Result<Vec<TradeRecord>, String> {
    let checked: std::collections::HashSet<i64> = {
        let mut stmt = conn
            .prepare("SELECT trade_id FROM trade_recalcs")
            .map_err(|_| "storeQueryFailed".to_string())?;
        let ids = stmt
            .query_map([], |r| r.get(0))
            .map_err(|_| "storeQueryFailed".to_string())?
            .collect::<Result<_, _>>()
            .map_err(|_| "storeRowMalformed".to_string())?;
        ids
    };
    Ok(trades::list(conn, 10_000)?
        .into_iter()
        .filter(|t| !t.live && t.closed_at < before_ms && !checked.contains(&t.id))
        .take(limit as usize)
        .collect())
}

/// Stores a recalculated close.
pub fn save_recalc(conn: &Connection, trade_id: i64, r: &TradeRecalc, now_ms: u64) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO trade_recalcs
            (trade_id, status, exit, exit_reason, closed_at, pnl_pct, pnl_quote, unlevered_net_pct, checked_at)
         VALUES (?1, 'recalculated', ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![trade_id, r.exit, r.exit_reason, r.closed_at as i64, r.pnl_pct, r.pnl_quote, r.unlevered_net_pct, now_ms as i64],
    )
    .map(|_| ())
    .map_err(|_| "storeWriteFailed".to_string())
}

/// Stores a trade kept as booked, with why.
pub fn save_kept(conn: &Connection, trade_id: i64, status: &str, now_ms: u64) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO trade_recalcs (trade_id, status, checked_at) VALUES (?1, ?2, ?3)",
        params![trade_id, status, now_ms as i64],
    )
    .map(|_| ())
    .map_err(|_| "storeWriteFailed".to_string())
}

fn recalcs(conn: &Connection) -> Result<HashMap<i64, TradeRecalc>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT trade_id, exit, exit_reason, closed_at, pnl_pct, pnl_quote, unlevered_net_pct
             FROM trade_recalcs WHERE status = 'recalculated'",
        )
        .map_err(|_| "storeQueryFailed".to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                TradeRecalc {
                    exit: r.get(1)?,
                    exit_reason: r.get(2)?,
                    closed_at: r.get(3)?,
                    pnl_pct: r.get(4)?,
                    pnl_quote: r.get(5)?,
                    unlevered_net_pct: r.get(6)?,
                },
            ))
        })
        .map_err(|_| "storeQueryFailed".to_string())?
        .collect::<Result<HashMap<_, _>, _>>()
        .map_err(|_| "storeRowMalformed".to_string())?;
    Ok(rows)
}

/// Sets `recalc` on every listed trade that has one.
pub fn mark(conn: &Connection, rows: &mut [TradeRecord]) -> Result<(), String> {
    let map = recalcs(conn)?;
    for t in rows.iter_mut() {
        t.recalc = map.get(&t.id).cloned();
    }
    Ok(())
}

/// (bot_kind, pnl_quote, pnl_pct, exit_reason, closed_at).
type ShownRow = (String, f64, f64, String, u64);

/// Every trade (real, simulated or both) as the desk shows it, oldest
/// close first.
fn shown_rows(conn: &Connection, live: Option<bool>) -> Result<Vec<ShownRow>, String> {
    let map = recalcs(conn)?;
    let mut rows: Vec<_> = trades::list(conn, u32::MAX)?
        .into_iter()
        .filter(|t| live.is_none_or(|l| t.live == l))
        .map(|t| match map.get(&t.id) {
            Some(r) => (t.bot_kind, r.pnl_quote, r.pnl_pct, r.exit_reason.clone(), r.closed_at),
            None => (t.bot_kind, t.pnl_quote, t.pnl_pct, t.exit_reason, t.closed_at),
        })
        .collect();
    rows.sort_by_key(|r| r.4);
    Ok(rows)
}

/// `trades::stats_scoped` on the figures the desk shows.
pub fn shown_stats(conn: &Connection, utc_day_start_ms: u64, live: Option<bool>) -> Result<PnlStats, String> {
    let rows: Vec<_> = shown_rows(conn, live)?.into_iter().map(|(_, q, p, r, at)| (q, p, r, at)).collect();
    Ok(trades::aggregate(&rows, utc_day_start_ms))
}

/// `bot_stats::by_bot` on the figures the desk shows.
pub fn shown_by_bot(conn: &Connection, utc_day_start_ms: u64, live: Option<bool>) -> Result<Vec<BotStats>, String> {
    let mut per: std::collections::BTreeMap<String, Vec<(f64, f64, String, u64)>> = Default::default();
    for (kind, q, p, r, at) in shown_rows(conn, live)? {
        per.entry(kind).or_default().push((q, p, r, at));
    }
    Ok(per
        .into_iter()
        .map(|(bot_kind, rows)| BotStats { stats: trades::aggregate(&rows, utc_day_start_ms), bot_kind })
        .collect())
}
