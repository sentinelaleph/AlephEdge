//! Strategy-bot (DCA / Grid, paper) tables. Desktop-only: the paper runner's
//! store mounts only model/positions/trades, and `migrate` here runs only from
//! the desktop `StoreManager`. DCA/Grid never write to `trades` or
//! `open_positions`, so signal-bot stats, the judge seed and CSV stay intact.
//!
//! Archive, never delete: `archived_at` hides a bot; its history stays.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::bot::strategy::cycle::CycleState;
use crate::bot::strategy::driver::EquityPoint;
use crate::bot::strategy::model::{
    BotRunState, ExitReason, Fill, OrderRole, OrderSide, OrderState, OrderType, SimOrder,
    StrategyBot, StrategyConfig, Utilisation,
};

use super::StoreManager;

pub fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS strategy_bots (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL CHECK(kind IN ('dca','grid')),
            name TEXT NOT NULL,
            exchange_id TEXT NOT NULL,
            market TEXT NOT NULL,
            symbol TEXT NOT NULL,
            side TEXT NOT NULL,
            config_json TEXT NOT NULL,
            runtime_json TEXT NOT NULL DEFAULT '{}',
            run_state TEXT NOT NULL,
            budget REAL NOT NULL,
            realized_quote REAL NOT NULL DEFAULT 0,
            peak_equity REAL NOT NULL,
            cycles_done INTEGER NOT NULL DEFAULT 0,
            dead_reason TEXT,
            paper INTEGER NOT NULL DEFAULT 1 CHECK(paper = 1),
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            archived_at INTEGER
        );
        CREATE TABLE IF NOT EXISTS strategy_cycles (
            bot_id TEXT NOT NULL REFERENCES strategy_bots(id),
            seq INTEGER NOT NULL,
            opened_at INTEGER NOT NULL,
            closed_at INTEGER,
            exit_reason TEXT,
            anchor_price REAL NOT NULL,
            size_factor REAL NOT NULL,
            avg_entry REAL,
            max_qty REAL,
            max_notional REAL,
            so_filled INTEGER,
            grid_closing_fills INTEGER,
            realized_quote REAL NOT NULL DEFAULT 0,
            fees_quote REAL NOT NULL DEFAULT 0,
            funding_quote REAL NOT NULL DEFAULT 0,
            pnl_pct_budget REAL,
            max_adverse_pct REAL,
            funding_unknown INTEGER NOT NULL DEFAULT 0,
            state_json TEXT,
            PRIMARY KEY (bot_id, seq)
        );
        CREATE INDEX IF NOT EXISTS idx_strategy_cycles_closed ON strategy_cycles(closed_at);
        CREATE TABLE IF NOT EXISTS strategy_orders (
            client_id TEXT PRIMARY KEY,
            bot_id TEXT NOT NULL,
            seq INTEGER NOT NULL,
            role TEXT NOT NULL,
            side TEXT NOT NULL,
            type TEXT NOT NULL,
            price REAL,
            qty REAL NOT NULL,
            state TEXT NOT NULL,
            active_from_leg INTEGER NOT NULL DEFAULT 0,
            exchange_order_id TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_strategy_orders_open ON strategy_orders(bot_id, state);
        CREATE TABLE IF NOT EXISTS strategy_fills (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            client_id TEXT NOT NULL,
            bot_id TEXT NOT NULL,
            seq INTEGER NOT NULL,
            ts INTEGER NOT NULL,
            price REAL NOT NULL,
            qty REAL NOT NULL,
            liquidity TEXT NOT NULL,
            fee_quote REAL NOT NULL,
            realized_quote REAL NOT NULL,
            kind TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_strategy_fills_bot ON strategy_fills(bot_id, ts);
        CREATE TABLE IF NOT EXISTS strategy_equity (
            bot_id TEXT NOT NULL,
            ts INTEGER NOT NULL,
            equity_quote REAL NOT NULL,
            margin_quote REAL NOT NULL,
            reserved_quote REAL NOT NULL,
            PRIMARY KEY (bot_id, ts)
        );
        CREATE TABLE IF NOT EXISTS strategy_cursors (
            exchange_id TEXT NOT NULL,
            market TEXT NOT NULL,
            symbol TEXT NOT NULL,
            last_close_ms INTEGER NOT NULL,
            last_close REAL NOT NULL,
            PRIMARY KEY (exchange_id, market, symbol)
        );",
    )
    .map_err(|_| "strategy table migration failed".to_string())
}

/// Bot fields that are not config: decision time, chain count, utilisation.
#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Runtime {
    #[serde(default)]
    next_decision_ms: u64,
    #[serde(default)]
    chain_cycles: u32,
    #[serde(default)]
    start_triggered: bool,
    #[serde(default)]
    util: Utilisation,
    #[serde(default)]
    max_dd_quote: f64,
}

fn err(e: rusqlite::Error) -> String {
    e.to_string()
}

pub fn save_bot(conn: &Connection, b: &StrategyBot, now: u64) -> Result<(), String> {
    let cfg = serde_json::to_string(&b.cfg).map_err(|e| e.to_string())?;
    let rt = serde_json::to_string(&Runtime {
        next_decision_ms: b.next_decision_ms,
        chain_cycles: b.chain_cycles,
        start_triggered: b.start_triggered,
        util: b.util,
        max_dd_quote: b.max_dd_quote,
    })
    .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO strategy_bots (id, kind, name, exchange_id, market, symbol, side, config_json,
            runtime_json, run_state, budget, realized_quote, peak_equity, cycles_done, dead_reason,
            paper, created_at, updated_at, archived_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, 1, ?16, ?17, ?18)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, market = excluded.market,
            symbol = excluded.symbol, side = excluded.side, config_json = excluded.config_json,
            runtime_json = excluded.runtime_json, run_state = excluded.run_state,
            budget = excluded.budget, realized_quote = excluded.realized_quote,
            peak_equity = excluded.peak_equity, cycles_done = excluded.cycles_done,
            dead_reason = excluded.dead_reason, updated_at = excluded.updated_at,
            archived_at = excluded.archived_at",
        params![
            b.id,
            b.cfg.kind().as_str(),
            b.cfg.name,
            b.cfg.exchange_id,
            b.cfg.market.as_str(),
            b.cfg.symbol,
            b.cfg.side.as_str(),
            cfg,
            rt,
            b.state.as_str(),
            b.cfg.budget,
            b.realized_quote,
            b.peak_equity,
            b.cycles_done,
            b.dead_reason.map(|r| r.as_str()),
            b.created_at as i64,
            now as i64,
            b.archived_at.map(|v| v as i64),
        ],
    )
    .map(|_| ())
    .map_err(err)
}

/// Every bot not archived. Rows that no longer parse are skipped.
pub fn load_bots(conn: &Connection) -> Result<Vec<StrategyBot>, String> {
    let mut st = conn
        .prepare(
            "SELECT id, config_json, runtime_json, run_state, realized_quote, peak_equity,
                    cycles_done, dead_reason, created_at, archived_at
             FROM strategy_bots WHERE archived_at IS NULL ORDER BY created_at",
        )
        .map_err(err)?;
    let rows = st
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, f64>(4)?,
                r.get::<_, f64>(5)?,
                r.get::<_, u32>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, i64>(8)?,
                r.get::<_, Option<i64>>(9)?,
            ))
        })
        .map_err(err)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, cfg, rt, state, realized, peak, cycles, dead, created, archived) = row.map_err(err)?;
        let Ok(cfg) = serde_json::from_str::<StrategyConfig>(&cfg) else {
            continue;
        };
        let rt: Runtime = serde_json::from_str(&rt).unwrap_or_default();
        out.push(StrategyBot {
            id,
            cfg,
            state: BotRunState::parse(&state).unwrap_or(BotRunState::Stopped),
            created_at: created as u64,
            cycles_done: cycles,
            realized_quote: realized,
            peak_equity: peak,
            max_dd_quote: rt.max_dd_quote,
            dead_reason: dead.as_deref().and_then(ExitReason::parse),
            next_decision_ms: rt.next_decision_ms,
            chain_cycles: rt.chain_cycles,
            start_triggered: rt.start_triggered,
            util: rt.util,
            archived_at: archived.map(|v| v as u64),
        });
    }
    Ok(out)
}

/// Writes a cycle row: open cycles carry their full state snapshot; a closed
/// cycle gets its result and the snapshot is cleared.
pub fn save_cycle(conn: &Connection, c: &CycleState, bot_budget: f64) -> Result<(), String> {
    let open = c.is_open();
    let state = if open {
        Some(serde_json::to_string(c).map_err(|e| e.to_string())?)
    } else {
        None
    };
    let realized = c.core.cash + c.core.fees + c.core.funding;
    let pct = |q: f64| if bot_budget > 0.0 { q / bot_budget * 100.0 } else { 0.0 };
    conn.execute(
        "INSERT INTO strategy_cycles (bot_id, seq, opened_at, closed_at, exit_reason, anchor_price,
            size_factor, avg_entry, max_qty, max_notional, so_filled, grid_closing_fills,
            realized_quote, fees_quote, funding_quote, pnl_pct_budget, max_adverse_pct,
            funding_unknown, state_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
         ON CONFLICT(bot_id, seq) DO UPDATE SET closed_at = excluded.closed_at,
            exit_reason = excluded.exit_reason, avg_entry = excluded.avg_entry,
            max_qty = excluded.max_qty, max_notional = excluded.max_notional,
            so_filled = excluded.so_filled, grid_closing_fills = excluded.grid_closing_fills,
            realized_quote = excluded.realized_quote, fees_quote = excluded.fees_quote,
            funding_quote = excluded.funding_quote, pnl_pct_budget = excluded.pnl_pct_budget,
            max_adverse_pct = excluded.max_adverse_pct, funding_unknown = excluded.funding_unknown,
            state_json = excluded.state_json",
        params![
            c.core.bot_id,
            c.core.seq,
            c.core.opened_at as i64,
            (!open).then_some(c.core.now_ms as i64),
            c.core.exit.map(|e| e.as_str()),
            c.core.anchor_price,
            c.core.size_factor,
            c.avg_entry(),
            c.signed_qty().abs(),
            c.core.max_notional,
            c.so_filled(),
            c.grid_closing_fills(),
            realized,
            c.core.fees,
            c.core.funding,
            (!open).then(|| pct(c.core.cash)),
            pct(c.core.max_adverse_quote),
            c.core.funding_unknown,
            state,
        ],
    )
    .map(|_| ())
    .map_err(err)
}

/// Open cycles' state snapshots (restore).
pub fn load_open_cycles(conn: &Connection) -> Result<Vec<CycleState>, String> {
    let mut st = conn
        .prepare("SELECT state_json FROM strategy_cycles WHERE closed_at IS NULL AND state_json IS NOT NULL")
        .map_err(err)?;
    let rows = st.query_map([], |r| r.get::<_, String>(0)).map_err(err)?;
    Ok(rows
        .filter_map(|r| r.ok())
        .filter_map(|s| serde_json::from_str::<CycleState>(&s).ok())
        .collect())
}

pub fn save_orders(conn: &Connection, bot_id: &str, seq: u32, orders: &[SimOrder], now: u64) -> Result<(), String> {
    for o in orders {
        conn.execute(
            "INSERT INTO strategy_orders (client_id, bot_id, seq, role, side, type, price, qty, state,
                active_from_leg, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)
             ON CONFLICT(client_id) DO UPDATE SET price = excluded.price, qty = excluded.qty,
                state = excluded.state, updated_at = excluded.updated_at",
            params![
                o.client_id,
                bot_id,
                seq,
                o.role.as_str(),
                o.side.as_str(),
                o.kind.as_str(),
                o.price,
                o.qty,
                o.state.as_str(),
                o.active_from_leg,
                now as i64,
            ],
        )
        .map_err(err)?;
    }
    Ok(())
}

fn order_from_row(r: &rusqlite::Row) -> rusqlite::Result<Option<SimOrder>> {
    let role: String = r.get(1)?;
    let side: String = r.get(2)?;
    let kind: String = r.get(3)?;
    let state: String = r.get(6)?;
    Ok((|| {
        Some(SimOrder {
            client_id: r.get(0).ok()?,
            role: OrderRole::parse(&role)?,
            side: OrderSide::parse(&side)?,
            kind: OrderType::parse(&kind)?,
            price: r.get::<_, Option<f64>>(4).ok()?.unwrap_or(0.0),
            qty: r.get(5).ok()?,
            active_from_leg: r.get(7).ok()?,
            state: OrderState::parse(&state)?,
        })
    })())
}

const ORDER_COLS: &str = "client_id, role, side, type, price, qty, state, active_from_leg";

pub fn load_open_orders(conn: &Connection, bot_id: &str) -> Result<Vec<SimOrder>, String> {
    let mut st = conn
        .prepare(&format!("SELECT {ORDER_COLS} FROM strategy_orders WHERE bot_id = ?1 AND state = 'open'"))
        .map_err(err)?;
    let rows = st.query_map(params![bot_id], order_from_row).map_err(err)?;
    Ok(rows.filter_map(|r| r.ok().flatten()).collect())
}

pub fn insert_fills(conn: &Connection, bot_id: &str, fills: &[(u32, Fill)]) -> Result<(), String> {
    for (seq, f) in fills {
        conn.execute(
            "INSERT INTO strategy_fills (client_id, bot_id, seq, ts, price, qty, liquidity, fee_quote,
                realized_quote, kind)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                f.client_id,
                bot_id,
                seq,
                f.ts as i64,
                f.price,
                f.qty,
                f.liquidity.as_str(),
                f.fee_quote,
                f.realized_quote,
                f.kind.as_str(),
            ],
        )
        .map_err(err)?;
    }
    Ok(())
}

pub fn insert_equity(conn: &Connection, bot_id: &str, points: &[EquityPoint]) -> Result<(), String> {
    for p in points {
        conn.execute(
            "INSERT OR REPLACE INTO strategy_equity (bot_id, ts, equity_quote, margin_quote, reserved_quote)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![bot_id, p.ts as i64, p.equity, p.margin, p.reserved],
        )
        .map_err(err)?;
    }
    Ok(())
}

/// Keeps the last row of each hour for rows older than `before_ms`.
pub fn compact_equity(conn: &Connection, before_ms: u64) -> Result<usize, String> {
    conn.execute(
        "DELETE FROM strategy_equity WHERE ts < ?1 AND (bot_id, ts) NOT IN (
            SELECT bot_id, MAX(ts) FROM strategy_equity WHERE ts < ?1 GROUP BY bot_id, ts / 3600000)",
        params![before_ms as i64],
    )
    .map_err(err)
}

pub fn get_cursor(conn: &Connection, exchange: &str, market: &str, symbol: &str) -> Result<Option<(u64, f64)>, String> {
    conn.query_row(
        "SELECT last_close_ms, last_close FROM strategy_cursors WHERE exchange_id = ?1 AND market = ?2 AND symbol = ?3",
        params![exchange, market, symbol],
        |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, f64>(1)?)),
    )
    .optional()
    .map_err(err)
}

pub fn set_cursor(conn: &Connection, exchange: &str, market: &str, symbol: &str, last_close_ms: u64, last_close: f64) -> Result<(), String> {
    conn.execute(
        "INSERT INTO strategy_cursors (exchange_id, market, symbol, last_close_ms, last_close)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(exchange_id, market, symbol) DO UPDATE SET last_close_ms = excluded.last_close_ms,
            last_close = excluded.last_close",
        params![exchange, market, symbol, last_close_ms as i64, last_close],
    )
    .map(|_| ())
    .map_err(err)
}

/// Everything one bot produced in one tick, written in ONE transaction.
pub struct TickWrite<'a> {
    pub bot: &'a StrategyBot,
    pub cycles: Vec<&'a CycleState>,
    pub orders: Vec<(u32, SimOrder)>,
    pub fills: Vec<(u32, Fill)>,
    pub equity: Vec<EquityPoint>,
}

pub fn persist_tick(conn: &Connection, w: &TickWrite, now: u64) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(err)?;
    save_bot(&tx, w.bot, now)?;
    for c in &w.cycles {
        save_cycle(&tx, c, w.bot.cfg.budget)?;
    }
    for (seq, o) in &w.orders {
        save_orders(&tx, &w.bot.id, *seq, std::slice::from_ref(o), now)?;
    }
    insert_fills(&tx, &w.bot.id, &w.fills)?;
    insert_equity(&tx, &w.bot.id, &w.equity)?;
    tx.commit().map_err(err)
}

// ------------------------------------------------------------- read models

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CycleRow {
    pub bot_id: String,
    pub seq: u32,
    pub opened_at: u64,
    pub closed_at: Option<u64>,
    pub exit_reason: Option<String>,
    pub anchor_price: f64,
    pub size_factor: f64,
    pub avg_entry: Option<f64>,
    pub max_notional: Option<f64>,
    pub so_filled: Option<u32>,
    pub grid_closing_fills: Option<u32>,
    pub realized_quote: f64,
    pub fees_quote: f64,
    pub funding_quote: f64,
    pub pnl_pct_budget: Option<f64>,
    pub max_adverse_pct: Option<f64>,
    pub funding_unknown: bool,
}

const CYCLE_COLS: &str = "bot_id, seq, opened_at, closed_at, exit_reason, anchor_price, size_factor,
    avg_entry, max_notional, so_filled, grid_closing_fills, realized_quote, fees_quote,
    funding_quote, pnl_pct_budget, max_adverse_pct, funding_unknown";

fn cycle_row(r: &rusqlite::Row) -> rusqlite::Result<CycleRow> {
    Ok(CycleRow {
        bot_id: r.get(0)?,
        seq: r.get(1)?,
        opened_at: r.get::<_, i64>(2)? as u64,
        closed_at: r.get::<_, Option<i64>>(3)?.map(|v| v as u64),
        exit_reason: r.get(4)?,
        anchor_price: r.get(5)?,
        size_factor: r.get(6)?,
        avg_entry: r.get(7)?,
        max_notional: r.get(8)?,
        so_filled: r.get(9)?,
        grid_closing_fills: r.get(10)?,
        realized_quote: r.get(11)?,
        fees_quote: r.get(12)?,
        funding_quote: r.get(13)?,
        pnl_pct_budget: r.get(14)?,
        max_adverse_pct: r.get(15)?,
        funding_unknown: r.get::<_, i64>(16)? != 0,
    })
}

pub fn list_cycles(conn: &Connection, bot_id: Option<&str>, limit: u32) -> Result<Vec<CycleRow>, String> {
    let sql = format!(
        "SELECT {CYCLE_COLS} FROM strategy_cycles WHERE (?1 IS NULL OR bot_id = ?1)
         ORDER BY opened_at DESC, seq DESC LIMIT ?2"
    );
    let mut st = conn.prepare(&sql).map_err(err)?;
    let rows = st
        .query_map(params![bot_id, i64::from(limit)], cycle_row)
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OrderRow {
    pub seq: u32,
    pub order: SimOrder,
    pub updated_at: u64,
}

/// Open orders plus the last 50 terminal ones.
pub fn list_orders(conn: &Connection, bot_id: &str) -> Result<Vec<OrderRow>, String> {
    let sql = format!(
        "SELECT {ORDER_COLS}, seq, updated_at FROM strategy_orders WHERE bot_id = ?1 AND state = 'open'
         UNION ALL
         SELECT * FROM (SELECT {ORDER_COLS}, seq, updated_at FROM strategy_orders
                        WHERE bot_id = ?1 AND state != 'open' ORDER BY updated_at DESC LIMIT 50)"
    );
    let mut st = conn.prepare(&sql).map_err(err)?;
    let rows = st
        .query_map(params![bot_id], |r| {
            Ok(order_from_row(r)?.map(|o| OrderRow {
                order: o,
                seq: r.get(8).unwrap_or(0),
                updated_at: r.get::<_, i64>(9).unwrap_or(0) as u64,
            }))
        })
        .map_err(err)?;
    Ok(rows.filter_map(|r| r.ok().flatten()).collect())
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FillRow {
    pub seq: u32,
    pub client_id: String,
    pub ts: u64,
    pub price: f64,
    pub qty: f64,
    pub liquidity: String,
    pub fee_quote: f64,
    pub realized_quote: f64,
    pub kind: String,
}

pub fn list_fills(conn: &Connection, bot_id: Option<&str>, limit: u32) -> Result<Vec<FillRow>, String> {
    let mut st = conn
        .prepare(
            "SELECT seq, client_id, ts, price, qty, liquidity, fee_quote, realized_quote, kind, bot_id
             FROM strategy_fills WHERE (?1 IS NULL OR bot_id = ?1) ORDER BY ts DESC, id DESC LIMIT ?2",
        )
        .map_err(err)?;
    let rows = st
        .query_map(params![bot_id, i64::from(limit)], |r| {
            Ok(FillRow {
                seq: r.get(0)?,
                client_id: r.get(1)?,
                ts: r.get::<_, i64>(2)? as u64,
                price: r.get(3)?,
                qty: r.get(4)?,
                liquidity: r.get(5)?,
                fee_quote: r.get(6)?,
                realized_quote: r.get(7)?,
                kind: r.get(8)?,
            })
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EquityRow {
    pub ts: u64,
    pub equity: f64,
    pub margin: f64,
    pub reserved: f64,
}

pub fn list_equity(conn: &Connection, bot_id: &str, from_ms: u64) -> Result<Vec<EquityRow>, String> {
    let mut st = conn
        .prepare(
            "SELECT ts, equity_quote, margin_quote, reserved_quote FROM strategy_equity
             WHERE bot_id = ?1 AND ts >= ?2 ORDER BY ts",
        )
        .map_err(err)?;
    let rows = st
        .query_map(params![bot_id, from_ms as i64], |r| {
            Ok(EquityRow {
                ts: r.get::<_, i64>(0)? as u64,
                equity: r.get(1)?,
                margin: r.get(2)?,
                reserved: r.get(3)?,
            })
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// Cycles CSV + fills CSV (two files).
pub fn export_csv(conn: &Connection) -> Result<(String, String), String> {
    let mut cycles = String::from(
        "bot_id,seq,opened_at,closed_at,exit_reason,anchor_price,size_factor,avg_entry,max_notional,so_filled,grid_closing_fills,realized_quote,fees_quote,funding_quote,pnl_pct_budget,max_adverse_pct,funding_unknown,paper\n",
    );
    let opt = |v: Option<f64>| v.map(|v| v.to_string()).unwrap_or_default();
    let optu = |v: Option<u32>| v.map(|v| v.to_string()).unwrap_or_default();
    for c in list_cycles(conn, None, u32::MAX)? {
        cycles.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},1\n",
            c.bot_id,
            c.seq,
            c.opened_at,
            c.closed_at.map(|v| v.to_string()).unwrap_or_default(),
            c.exit_reason.unwrap_or_default(),
            c.anchor_price,
            c.size_factor,
            opt(c.avg_entry),
            opt(c.max_notional),
            optu(c.so_filled),
            optu(c.grid_closing_fills),
            c.realized_quote,
            c.fees_quote,
            c.funding_quote,
            opt(c.pnl_pct_budget),
            opt(c.max_adverse_pct),
            c.funding_unknown,
        ));
    }
    let mut fills = String::from("bot_id,seq,client_id,ts,price,qty,liquidity,fee_quote,realized_quote,kind,paper\n");
    let mut st = conn
        .prepare(
            "SELECT bot_id, seq, client_id, ts, price, qty, liquidity, fee_quote, realized_quote, kind
             FROM strategy_fills ORDER BY ts, id",
        )
        .map_err(err)?;
    let rows = st
        .query_map([], |r| {
            Ok(format!(
                "{},{},{},{},{},{},{},{},{},{},1\n",
                r.get::<_, String>(0)?,
                r.get::<_, u32>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, f64>(4)?,
                r.get::<_, f64>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, f64>(7)?,
                r.get::<_, f64>(8)?,
                r.get::<_, String>(9)?,
            ))
        })
        .map_err(err)?;
    for r in rows {
        fills.push_str(&r.map_err(err)?);
    }
    Ok((cycles, fills))
}

// ------------------------------------------------------- manager wrappers

impl StoreManager {
    pub fn strategy<T>(&self, dir: &Path, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
        self.with_conn(dir, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::strategy::costs::FUTURES_SIM;
    use crate::bot::strategy::cycle::{step_bar, CycleStart};
    use crate::bot::strategy::driver::tests::{minute_bars, sample_bot};
    use crate::bot::strategy::model::StrategyParams;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c).unwrap();
        c
    }

    #[test]
    fn migrate_is_idempotent() {
        let c = db();
        migrate(&c).unwrap();
        migrate(&c).unwrap();
    }

    #[test]
    fn schema_refuses_a_live_row() {
        let c = db();
        let r = c.execute(
            "INSERT INTO strategy_bots (id, kind, name, exchange_id, market, symbol, side, config_json,
                run_state, budget, peak_equity, paper, created_at, updated_at)
             VALUES ('x','dca','n','binance','futures','BTCUSDT','long','{}','stopped',1,1,0,0,0)",
            [],
        );
        assert!(r.is_err(), "CHECK(paper = 1) must reject a live row");
    }

    #[test]
    fn bot_round_trip_and_archive_keeps_cycles() {
        let c = db();
        let mut b = sample_bot();
        b.chain_cycles = 3;
        b.next_decision_ms = 42;
        save_bot(&c, &b, 1).unwrap();
        let loaded = load_bots(&c).unwrap();
        assert_eq!(loaded, vec![b.clone()]);

        let StrategyParams::Dca(p) = &b.cfg.params else { panic!() };
        let bars = minute_bars(&[(100.0, 100.5, 99.5, 100.2)]);
        let st = CycleStart {
            bot_id: &b.id,
            seq: 1,
            side: b.cfg.side,
            budget: 1000.0,
            leverage: 1.0,
            size_factor: 1.0,
            costs: FUTURES_SIM,
            max_duration_min: None,
        };
        let mut cyc = CycleState::open_dca(&st, p, &bars[0]).unwrap();
        let fills: Vec<(u32, Fill)> = step_bar(&mut cyc, &bars[0], true).into_iter().map(|f| (1, f)).collect();
        let orders: Vec<(u32, SimOrder)> = cyc.open_orders().into_iter().map(|o| (1, o)).collect();
        persist_tick(
            &c,
            &TickWrite { bot: &b, cycles: vec![&cyc], orders, fills, equity: vec![EquityPoint { ts: 5, equity: 999.0, margin: 100.0, reserved: 300.0 }] },
            2,
        )
        .unwrap();
        let open = load_open_cycles(&c).unwrap();
        assert_eq!(open, vec![cyc.clone()], "state_json round-trips exactly");
        assert_eq!(load_open_orders(&c, &b.id).unwrap(), cyc.open_orders());
        assert_eq!(list_fills(&c, Some(&b.id), 10).unwrap().len(), 1);
        assert_eq!(list_equity(&c, &b.id, 0).unwrap().len(), 1);

        let mut archived = b.clone();
        archived.archived_at = Some(9);
        save_bot(&c, &archived, 3).unwrap();
        assert!(load_bots(&c).unwrap().is_empty());
        assert_eq!(list_cycles(&c, Some(&b.id), 10).unwrap().len(), 1, "history kept");
        let (cy, fi) = export_csv(&c).unwrap();
        assert_eq!(cy.lines().count(), 2);
        assert_eq!(fi.lines().count(), 2);
    }

    #[test]
    fn cursor_and_compaction() {
        let c = db();
        assert_eq!(get_cursor(&c, "binance", "futures", "BTCUSDT").unwrap(), None);
        set_cursor(&c, "binance", "futures", "BTCUSDT", 59_999, 101.5).unwrap();
        set_cursor(&c, "binance", "futures", "BTCUSDT", 119_999, 102.5).unwrap();
        assert_eq!(get_cursor(&c, "binance", "futures", "BTCUSDT").unwrap(), Some((119_999, 102.5)));
        let pts: Vec<EquityPoint> = (0..120u64)
            .map(|i| EquityPoint { ts: i * 60_000 + 59_999, equity: 1.0, margin: 0.0, reserved: 0.0 })
            .collect();
        insert_equity(&c, "b", &pts).unwrap();
        compact_equity(&c, 7_200_000).unwrap();
        assert_eq!(list_equity(&c, "b", 0).unwrap().len(), 2, "one row per hour kept");
    }

    #[test]
    fn compaction_keeps_one_row_per_hour_per_bot() {
        // Bots share bar-close timestamps: bot "y" stopped recording 10
        // minutes before bot "x" in hour 0. Each bot keeps ITS last row of
        // the hour, not every row at another bot's last timestamp.
        let c = db();
        let pts = |n: u64| -> Vec<EquityPoint> {
            (0..n).map(|i| EquityPoint { ts: i * 60_000 + 59_999, equity: 1.0, margin: 0.0, reserved: 0.0 }).collect()
        };
        insert_equity(&c, "x", &pts(60)).unwrap();
        insert_equity(&c, "y", &pts(50)).unwrap();
        compact_equity(&c, 7_200_000).unwrap();
        let y = list_equity(&c, "y", 0).unwrap();
        assert_eq!(y.len(), 1, "{:?}", y.iter().map(|r| r.ts).collect::<Vec<_>>());
        assert_eq!(y[0].ts, 49 * 60_000 + 59_999);
        assert_eq!(list_equity(&c, "x", 0).unwrap().len(), 1);
    }
}
