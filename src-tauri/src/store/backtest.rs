//! Backtest runs (DCA / Grid historical simulations). Desktop-only, local:
//! the newest `KEEP_RUNS` are kept, older ones are pruned on insert. A run is
//! a simulation over DataHub candles, never a trade: nothing here touches
//! `trades`, `open_positions` or the strategy-bot tables.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::bot::strategy::backtest::BacktestResult;
use crate::bot::strategy::model::{MarketKind, StrategyConfig};

pub const KEEP_RUNS: u32 = 50;

pub fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS backtest_runs (
            id TEXT PRIMARY KEY,
            created_at INTEGER NOT NULL,
            kind TEXT NOT NULL CHECK(kind IN ('dca','grid')),
            name TEXT NOT NULL,
            symbol TEXT NOT NULL,
            market TEXT NOT NULL,
            interval TEXT NOT NULL,
            start_ms INTEGER NOT NULL,
            end_ms INTEGER NOT NULL,
            total_pnl_pct REAL NOT NULL,
            max_drawdown_pct REAL NOT NULL,
            closed_cycles INTEGER NOT NULL,
            open_at_end INTEGER NOT NULL,
            coverage_pct REAL,
            run_json TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS backtest_runs_created ON backtest_runs (created_at DESC);",
    )
    .map_err(|_| "backtest table migration failed".to_string())
}

/// What the candles were and where they came from.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BacktestData {
    /// The history route's `source` label(s).
    pub source: String,
    /// Candles used (after dedupe and cleaning).
    pub candle_count: u64,
    /// Candles the server expected for the window, if it said.
    pub expected_candles: Option<u64>,
    /// candle_count / expected, %, from the server's coverage when present.
    pub coverage_pct: Option<f64>,
    /// Holes in the bar grid between the first and last candle.
    pub missing_bars: u64,
    /// Malformed, unparseable or still-forming candles dropped.
    pub dropped_candles: u32,
    pub requests: u32,
    /// Always false: no funding history exists in the source.
    pub funding_included: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BacktestRun {
    pub id: String,
    pub created_at: u64,
    pub config: StrategyConfig,
    pub symbol: String,
    pub market: MarketKind,
    pub interval: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub data: BacktestData,
    pub result: BacktestResult,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BacktestRunSummary {
    pub id: String,
    pub created_at: u64,
    pub kind: String,
    pub name: String,
    pub symbol: String,
    pub market: String,
    pub interval: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub total_pnl_pct: f64,
    pub max_drawdown_pct: f64,
    pub closed_cycles: u32,
    pub open_at_end: bool,
    pub coverage_pct: Option<f64>,
}

fn err(e: rusqlite::Error) -> String {
    e.to_string()
}

/// Inserts a run and prunes everything beyond the newest `KEEP_RUNS`.
pub fn insert(conn: &Connection, run: &BacktestRun) -> Result<(), String> {
    let json = serde_json::to_string(run).map_err(|e| e.to_string())?;
    let r = &run.result;
    conn.execute(
        "INSERT INTO backtest_runs (id, created_at, kind, name, symbol, market, interval, start_ms, end_ms,
            total_pnl_pct, max_drawdown_pct, closed_cycles, open_at_end, coverage_pct, run_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            run.id,
            run.created_at as i64,
            run.config.kind().as_str(),
            run.config.name,
            run.symbol,
            run.market.as_str(),
            run.interval,
            run.start_ms as i64,
            run.end_ms as i64,
            r.total_pnl_pct,
            r.max_drawdown_pct,
            i64::from(r.closed_cycles),
            i64::from(r.open_at_end.is_some()),
            run.data.coverage_pct,
            json,
        ],
    )
    .map_err(err)?;
    conn.execute(
        "DELETE FROM backtest_runs WHERE id NOT IN
            (SELECT id FROM backtest_runs ORDER BY created_at DESC, id DESC LIMIT ?1)",
        params![i64::from(KEEP_RUNS)],
    )
    .map_err(err)?;
    Ok(())
}

pub fn list(conn: &Connection) -> Result<Vec<BacktestRunSummary>, String> {
    let mut st = conn
        .prepare(
            "SELECT id, created_at, kind, name, symbol, market, interval, start_ms, end_ms,
                total_pnl_pct, max_drawdown_pct, closed_cycles, open_at_end, coverage_pct
             FROM backtest_runs ORDER BY created_at DESC, id DESC LIMIT ?1",
        )
        .map_err(err)?;
    let rows = st
        .query_map(params![i64::from(KEEP_RUNS)], |r| {
            Ok(BacktestRunSummary {
                id: r.get(0)?,
                created_at: r.get::<_, i64>(1)? as u64,
                kind: r.get(2)?,
                name: r.get(3)?,
                symbol: r.get(4)?,
                market: r.get(5)?,
                interval: r.get(6)?,
                start_ms: r.get::<_, i64>(7)? as u64,
                end_ms: r.get::<_, i64>(8)? as u64,
                total_pnl_pct: r.get(9)?,
                max_drawdown_pct: r.get(10)?,
                closed_cycles: r.get::<_, i64>(11)? as u32,
                open_at_end: r.get::<_, i64>(12)? != 0,
                coverage_pct: r.get(13)?,
            })
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<BacktestRun>, String> {
    let json: Option<String> = conn
        .query_row("SELECT run_json FROM backtest_runs WHERE id = ?1", params![id], |r| r.get(0))
        .optional()
        .map_err(err)?;
    match json {
        Some(j) => serde_json::from_str(&j).map(Some).map_err(|_| "backtestRunUnreadable".to_string()),
        None => Ok(None),
    }
}

/// True when a row was removed.
pub fn delete(conn: &Connection, id: &str) -> Result<bool, String> {
    conn.execute("DELETE FROM backtest_runs WHERE id = ?1", params![id])
        .map(|n| n > 0)
        .map_err(err)
}

/// Removes every stored run; returns how many.
pub fn delete_all(conn: &Connection) -> Result<usize, String> {
    conn.execute("DELETE FROM backtest_runs", []).map_err(err)
}

impl super::StoreManager {
    pub fn backtests<T>(&self, dir: &std::path::Path, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
        self.with_conn(dir, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::strategy::backtest;
    use crate::bot::strategy::driver::tests::{sample_bot, wave};

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c).unwrap();
        c
    }

    fn run(id: &str, at: u64) -> BacktestRun {
        let cfg = sample_bot().cfg;
        let bars = wave();
        let result = backtest::run(&cfg, &bars).unwrap();
        BacktestRun {
            id: id.into(),
            created_at: at,
            symbol: cfg.symbol.clone(),
            market: cfg.market,
            config: cfg,
            interval: "1h".into(),
            start_ms: bars[0].open_ms,
            end_ms: bars[bars.len() - 1].close_ms + 1,
            data: BacktestData {
                source: "test".into(),
                candle_count: bars.len() as u64,
                expected_candles: Some(bars.len() as u64),
                coverage_pct: Some(100.0),
                missing_bars: 0,
                dropped_candles: 0,
                requests: 1,
                funding_included: false,
            },
            result,
        }
    }

    #[test]
    fn delete_all_empties_the_list() {
        let c = db();
        migrate(&c).unwrap();
        for i in 0..3u64 {
            insert(&c, &run(&format!("bt_{i}"), i)).unwrap();
        }
        assert_eq!(delete_all(&c).unwrap(), 3);
        assert!(list(&c).unwrap().is_empty());
        assert_eq!(delete_all(&c).unwrap(), 0);
    }

    #[test]
    fn round_trip_list_delete_and_keep_newest_50() {
        let c = db();
        migrate(&c).unwrap();
        let first = run("bt_a", 1);
        insert(&c, &first).unwrap();
        assert_eq!(get(&c, "bt_a").unwrap().as_ref(), Some(&first), "bit-identical round trip");
        for i in 0..55u64 {
            insert(&c, &run(&format!("bt_{i:03}"), 100 + i)).unwrap();
        }
        let l = list(&c).unwrap();
        assert_eq!(l.len(), 50);
        assert_eq!(l[0].id, "bt_054", "newest first");
        assert!(get(&c, "bt_a").unwrap().is_none(), "oldest pruned");
        assert_eq!(l[0].kind, "dca");
        assert!(delete(&c, "bt_054").unwrap());
        assert!(!delete(&c, "bt_054").unwrap());
        assert_eq!(list(&c).unwrap().len(), 49);
    }
}
