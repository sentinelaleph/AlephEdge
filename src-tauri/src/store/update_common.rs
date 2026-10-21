//! Shared by the update-persistence fixture generator (`update_gen.rs`, run on
//! an OLDER release) and the reader tests (`update_persist_tests.rs`, run on a
//! NEWER build). Uses only APIs that exist unchanged from 0.3.16 on, so the
//! same file compiles in every release tree it is copied into.
//!
//! The scenario: six paper DCA / Grid bots walked through deterministic 1m
//! bars the way the engine walks them (TICK bars per pass, one SQLite
//! transaction per bot per pass, cursor after), stopped at SPLIT (the moment
//! the user clicks Install), then walked on to TOTAL. The generator records
//! the uninterrupted walk; the reader restores from the old database with the
//! new code and must produce the identical walk.

use std::collections::HashSet;

use rusqlite::Connection;
use serde_json::{json, Value};

use crate::bot::strategy::cycle::CycleState;
use crate::bot::strategy::driver;
use crate::bot::strategy::model::{Bar, BotRunState, FillKind, SimOrder, StrategyBot, StrategyConfig, Utilisation};
use crate::bot::strategy::venue::PaperVenue;
use crate::store::strategy as db;

/// First bar open (minute aligned, 2026-10).
pub const T0: u64 = 1_792_000_000_000 - (1_792_000_000_000 % 60_000);
/// Bars walked before the update.
pub const SPLIT: usize = 180;
/// Bars in the whole scenario.
pub const TOTAL: usize = 420;
/// Bars per engine pass.
pub const TICK: usize = 15;

pub const BOT_A_DCA_RUNNING: &str = "sb_arundcaxxxxx";
pub const BOT_B_GRID_RUNNING: &str = "sb_brungridxxxx";
pub const BOT_C_DCA_STOPPED: &str = "sb_cstopdcaxxxx";
pub const BOT_D_DCA_PAUSED: &str = "sb_dpausedcxxxx";
pub const BOT_E_DCA_ARMED: &str = "sb_earmeddcxxxx";
pub const BOT_F_DCA_LIVE_TIED: &str = "sb_flivedcaxxxx";

/// (id, symbol, price scale, grid?, armed-only?)
pub const BOTS: [(&str, &str, f64, bool, bool); 6] = [
    (BOT_A_DCA_RUNNING, "UPDAUSDT", 1.0, false, false),
    (BOT_B_GRID_RUNNING, "UPDBUSDT", 0.5, true, false),
    (BOT_C_DCA_STOPPED, "UPDCUSDT", 2.0, false, false),
    (BOT_D_DCA_PAUSED, "UPDDUSDT", 0.25, false, false),
    (BOT_E_DCA_ARMED, "UPDEUSDT", 1.0, false, true),
    (BOT_F_DCA_LIVE_TIED, "UPDFUSDT", 3.0, false, false),
];

fn restart() -> Value {
    json!({"cooldownMin": 0, "maxCycles": null, "afterStop": true, "afterLiquidation": false, "priceBand": null, "endAtMs": null})
}

pub fn dca_config(name: &str, symbol: &str, armed_only: bool) -> StrategyConfig {
    let start = if armed_only {
        // Never reached: the bot stays Armed, waiting.
        json!({"type": "priceCross", "price": 1_000_000.0, "direction": "up"})
    } else {
        json!({"type": "immediately"})
    };
    serde_json::from_value(json!({
        "schemaVersion": 1, "name": name, "exchangeId": "binance", "market": "futures",
        "symbol": symbol, "side": "long", "budget": 1000.0, "leverage": 2, "start": start,
        "restart": restart(), "maxDrawdownPct": null, "pauseOnBtcBreak": false, "portfolioBreaker": false,
        "params": {"kind": "dca", "baseOrder": null, "safetyOrder": null, "baseWeight": 1.0,
            "safetyWeight": 1.0, "maxSo": 5, "soStepPct": 1.0, "stepScale": 1.0, "volumeScale": 1.2,
            "tpPct": 1.5, "trailingPct": null, "slPct": null, "maxDurationMin": null},
        "presetId": null
    }))
    .expect("dca config")
}

pub fn grid_config(name: &str, symbol: &str) -> StrategyConfig {
    serde_json::from_value(json!({
        "schemaVersion": 1, "name": name, "exchangeId": "binance", "market": "futures",
        "symbol": symbol, "side": "neutral", "budget": 1000.0, "leverage": 2,
        "start": {"type": "immediately"}, "restart": restart(), "maxDrawdownPct": null,
        "pauseOnBtcBreak": false, "portfolioBreaker": false,
        "params": {"kind": "grid", "range": {"type": "relative", "lowerPct": 6.0, "upperPct": 6.0},
            "nGrids": 12, "spacing": "geom", "stopOutPct": 5.0, "trailingUp": false,
            "trailUpLimit": null, "takeProfitPct": null, "maxDurationMin": null},
        "presetId": null
    }))
    .expect("grid config")
}

pub fn new_bot(id: &str, cfg: StrategyConfig, created_at: u64) -> StrategyBot {
    StrategyBot {
        id: id.to_string(),
        peak_equity: cfg.budget,
        cfg,
        state: BotRunState::Armed,
        created_at,
        cycles_done: 0,
        realized_quote: 0.0,
        max_dd_quote: 0.0,
        dead_reason: None,
        next_decision_ms: 0,
        chain_cycles: 0,
        start_triggered: false,
        util: Utilisation::default(),
        archived_at: None,
    }
}

/// DCA path: down 2.4% (two of five safety orders fill), flat until SPLIT,
/// then up through the take-profit and a wave that drives further cycles.
fn dca_close(i: usize) -> f64 {
    let x = i as f64;
    let base = if i < 20 {
        100.0
    } else if i < 120 {
        100.0 - 2.4 * (x - 20.0) / 100.0
    } else if i < 200 {
        97.6
    } else if i < 260 {
        97.6 + 3.4 * (x - 200.0) / 60.0
    } else {
        101.0 + 1.6 * ((x - 260.0) / 11.0).sin()
    };
    base + 0.08 * (x / 3.0).sin()
}

/// Grid path: a +-3% wave inside a +-6% band, so levels fill both ways.
fn grid_close(i: usize) -> f64 {
    let x = i as f64;
    100.0 * (1.0 + 0.03 * (x / 7.0).sin()) + 0.05 * (x / 2.0).cos()
}

pub fn bars(grid: bool, scale: f64) -> Vec<Bar> {
    let mut out = Vec::with_capacity(TOTAL);
    let mut prev = if grid { grid_close(0) } else { dca_close(0) } * scale;
    for i in 0..TOTAL {
        let c = if grid { grid_close(i) } else { dca_close(i) } * scale;
        let o = prev;
        let pad = 0.03 * scale;
        out.push(Bar {
            open_ms: T0 + i as u64 * 60_000,
            close_ms: T0 + i as u64 * 60_000 + 59_999,
            o,
            h: o.max(c) + pad,
            l: o.min(c) - pad,
            c,
            funding_rate: None,
            funding_unknown: false,
        });
        prev = c;
    }
    out
}

/// One bot as the engine holds it in memory.
#[derive(Clone)]
pub struct Held {
    pub bot: StrategyBot,
    pub cycle: Option<CycleState>,
    pub venue: PaperVenue,
}

/// Client ids carry the cycle seq: "ae{bot8}-{seq}-{role}".
pub fn seq_of(client_id: &str) -> u32 {
    client_id.split('-').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// Walks `bars` through one bot TICK bars per pass, as the engine does
/// (bars the open cycle already walked are skipped). With `conn`, each pass
/// is written in one transaction (`persist_tick`) and the cursor after it.
/// Every fill is appended to `log` as [botId, seq, fill].
pub fn walk(h: &mut Held, bars: &[Bar], conn: Option<&Connection>, log: &mut Vec<Value>) -> Result<(), String> {
    for chunk in bars.chunks(TICK) {
        let mut fills = Vec::new();
        let mut closed = Vec::new();
        let mut equity = Vec::new();
        for bar in chunk {
            if h.cycle.as_ref().is_some_and(|c| bar.close_ms <= c.core.last_bar_close_ms) {
                continue;
            }
            let out = driver::on_bar(&mut h.bot, &mut h.cycle, bar, None);
            for (seq, f) in &out.events {
                log.push(json!([h.bot.id, seq, f]));
            }
            fills.extend(out.events);
            if let Some(c) = out.closed {
                closed.push(c);
            }
            if let Some(e) = out.equity {
                equity.push(e);
            }
        }
        // The venue sync of engine::persist.
        let filled: HashSet<String> = fills
            .iter()
            .filter(|(_, f)| f.kind == FillKind::Fill)
            .map(|(_, f)| f.client_id.clone())
            .collect();
        let desired = h.cycle.as_ref().map(|c| c.open_orders()).unwrap_or_default();
        let orders: Vec<(u32, SimOrder)> = h
            .venue
            .sync(desired, &filled)
            .into_iter()
            .map(|o| (seq_of(&o.client_id), o))
            .collect();
        if let Some(c) = conn {
            let last = chunk.last().expect("non-empty chunk");
            let mut cycles: Vec<&CycleState> = closed.iter().collect();
            if let Some(open) = h.cycle.as_ref() {
                cycles.push(open);
            }
            let w = db::TickWrite { bot: &h.bot, cycles, orders, fills, equity };
            db::persist_tick(c, &w, last.close_ms)?;
            db::set_cursor(c, &h.bot.cfg.exchange_id, h.bot.cfg.market.as_str(), &h.bot.cfg.symbol, last.close_ms, last.c)?;
        }
    }
    Ok(())
}

/// Open orders of a bot, sorted by client id (row order is not part of the contract).
pub fn sorted_orders(mut v: Vec<SimOrder>) -> Vec<SimOrder> {
    v.sort_by(|a, b| a.client_id.cmp(&b.client_id));
    v
}

/// Every user table and its row count.
pub fn table_counts(conn: &Connection) -> Result<Value, String> {
    let mut st = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .map_err(|e| e.to_string())?;
    let names: Vec<String> = st
        .query_map([], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();
    let mut out = serde_json::Map::new();
    for n in names {
        let count: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM \"{n}\""), [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        out.insert(n, json!(count));
    }
    Ok(Value::Object(out))
}

/// Column names per table (schema check: an update may add, never drop).
pub fn table_columns(conn: &Connection) -> Result<Value, String> {
    let counts = table_counts(conn)?;
    let mut out = serde_json::Map::new();
    for name in counts.as_object().expect("object").keys() {
        let mut st = conn.prepare(&format!("PRAGMA table_info(\"{name}\")")).map_err(|e| e.to_string())?;
        let cols: Vec<String> = st
            .query_map([], |r| r.get::<_, String>(1))
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect();
        out.insert(name.clone(), json!(cols));
    }
    Ok(Value::Object(out))
}

/// The whole meta table, key -> value, byte for byte.
pub fn meta_dump(conn: &Connection) -> Result<Value, String> {
    let mut st = conn.prepare("SELECT key, value FROM meta ORDER BY key").map_err(|e| e.to_string())?;
    let rows = st
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?;
    let mut out = serde_json::Map::new();
    for row in rows {
        let (k, v) = row.map_err(|e| e.to_string())?;
        out.insert(k, json!(v));
    }
    Ok(Value::Object(out))
}

/// Refuses any directory that could be a real Aleph Edge data dir.
pub fn assert_scratch_dir(p: &std::path::Path) {
    let s = p.to_string_lossy().to_lowercase().replace('/', "\\");
    assert!(!s.contains("com.sentinelaleph"), "refusing a real app data dir: {s}");
    assert!(!s.contains("\\appdata\\roaming\\"), "refusing a roaming app data dir: {s}");
}
