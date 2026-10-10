//! Data written by release 0.2.0 must still load (audit 2026-10-08,
//! persistence 4). The updater delivers every later version onto these rows,
//! and restore skips a row it cannot read, so a struct change that breaks
//! them would surface as bots that vanish or resume over lost state.
//!
//! The fixtures under `fixtures/v0_2_0/` are real rows of a TESTNET paper
//! desk (read-only export, 8 Oct 2026). Their serialized shape is the 0.2.0
//! one: `git diff 85cd8ae` (Release 0.2.0) adds no field to OpenPosition,
//! BotConfig, StrategyConfig or CycleState. A failing test here means a
//! change no longer reads what users already have on disk.

use rusqlite::{params, Connection};

use crate::bot::model::{BotConfig, BotKind, OpenPosition};
use crate::bot::strategy::cycle::CycleState;
use crate::bot::strategy::model::StrategyConfig;

use super::{positions, restore_check, strategy, trades};

const OPEN_POSITION: &str = include_str!("fixtures/v0_2_0/open_position.json");
const BOT_CONFIG_FUTURES: &str = include_str!("fixtures/v0_2_0/bot_config_futures.json");
const BOT_CONFIG_SPOT: &str = include_str!("fixtures/v0_2_0/bot_config_spot.json");
const DCA_CONFIG: &str = include_str!("fixtures/v0_2_0/strategy_dca_config.json");
const DCA_RUNTIME: &str = include_str!("fixtures/v0_2_0/strategy_dca_runtime.json");
const DCA_CYCLE: &str = include_str!("fixtures/v0_2_0/strategy_dca_cycle.json");
const GRID_CONFIG: &str = include_str!("fixtures/v0_2_0/strategy_grid_config.json");
const GRID_RUNTIME: &str = include_str!("fixtures/v0_2_0/strategy_grid_runtime.json");
const GRID_CYCLE: &str = include_str!("fixtures/v0_2_0/strategy_grid_cycle.json");

/// Parses `raw` as `T` and checks that writing it back loses nothing.
fn round_trips<T: serde::de::DeserializeOwned + serde::Serialize>(name: &str, raw: &str) -> T {
    let parsed: T = serde_json::from_str(raw).unwrap_or_else(|e| panic!("{name}: {e}"));
    let original: serde_json::Value = serde_json::from_str(raw).unwrap();
    let again = serde_json::to_value(&parsed).unwrap();
    assert_eq!(again, original, "{name}: a field changed or was dropped on the round trip");
    parsed
}

#[test]
fn every_0_2_0_row_parses_and_round_trips() {
    let pos: OpenPosition = round_trips("open position", OPEN_POSITION);
    assert_eq!((pos.bot_kind, pos.symbol.as_str()), (BotKind::Futures, "KASUSDT"));
    for (name, raw) in [("futures", BOT_CONFIG_FUTURES), ("spot", BOT_CONFIG_SPOT)] {
        let cfg: BotConfig = round_trips(name, raw);
        assert!(cfg.validate().is_ok(), "{name}: today's validation refuses a 0.2.0 config");
    }
    for (name, cfg, cycle) in [("dca", DCA_CONFIG, DCA_CYCLE), ("grid", GRID_CONFIG, GRID_CYCLE)] {
        round_trips::<StrategyConfig>(name, cfg);
        let c: CycleState = round_trips(name, cycle);
        assert!(c.is_open(), "{name}: the stored cycle is open");
    }
}

/// An in-memory DB with today's migrations (a superset of 0.2.0's tables).
fn db() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    trades::migrate(&c).unwrap();
    positions::migrate(&c).unwrap();
    strategy::migrate(&c).unwrap();
    c
}

fn insert_bot(c: &Connection, id: &str, kind: &str, cfg: &str, runtime: &str, archived: Option<i64>) {
    c.execute(
        "INSERT INTO strategy_bots (id, kind, name, exchange_id, market, symbol, side, config_json,
            runtime_json, run_state, budget, peak_equity, cycles_done, created_at, updated_at, archived_at)
         VALUES (?1, ?2, 'n', 'binance', 'futures', 'BTCUSDT', 'long', ?3, ?4, 'inCycle', 1000, 1000, 0, 1, 1, ?5)",
        params![id, kind, cfg, runtime, archived],
    )
    .unwrap();
}

fn insert_open_cycle(c: &Connection, id: &str, seq: u32, state: Option<&str>) {
    c.execute(
        "INSERT INTO strategy_cycles (bot_id, seq, opened_at, anchor_price, size_factor, state_json)
         VALUES (?1, ?2, 1, 100, 1, ?3)",
        params![id, seq, state],
    )
    .unwrap();
}

// The rows go through the very functions restore calls, and the restore
// check finds nothing to report.
#[test]
fn a_0_2_0_database_restores_everything() {
    let c = db();
    positions::upsert(&c, "0a242b31-fc67-4852-8f9e-758d3384c41b", "futures", OPEN_POSITION, 1).unwrap();
    insert_bot(&c, "sb_ax4hvhjwitwy", "dca", DCA_CONFIG, DCA_RUNTIME, None);
    insert_bot(&c, "sb_mc7jcjnxqntd", "grid", GRID_CONFIG, GRID_RUNTIME, None);
    insert_open_cycle(&c, "sb_ax4hvhjwitwy", 1, Some(DCA_CYCLE));
    insert_open_cycle(&c, "sb_mc7jcjnxqntd", 1, Some(GRID_CYCLE));

    let rows = positions::load_all_keyed(&c).unwrap();
    assert_eq!(rows.len(), 1);
    assert!(serde_json::from_str::<OpenPosition>(&rows[0].2).is_ok());
    assert_eq!(strategy::load_bots(&c).unwrap().len(), 2);
    assert_eq!(strategy::load_open_cycles(&c).unwrap().len(), 2);
    assert!(restore_check::unreadable_rows(&c).unwrap().is_empty());
}

// What restore used to skip in silence is now reported: a config that no
// longer parses, an open cycle whose state is corrupt or missing. Archived
// bots are not restored, so they are not reported either.
#[test]
fn rows_restore_cannot_use_are_reported() {
    let c = db();
    insert_bot(&c, "sb_ok", "dca", DCA_CONFIG, DCA_RUNTIME, None);
    insert_bot(&c, "sb_badcfg", "dca", "{\"schemaVersion\":1}", DCA_RUNTIME, None);
    insert_bot(&c, "sb_badcycle", "grid", GRID_CONFIG, GRID_RUNTIME, None);
    insert_bot(&c, "sb_nostate", "dca", DCA_CONFIG, DCA_RUNTIME, None);
    insert_bot(&c, "sb_archived", "dca", "{", DCA_RUNTIME, Some(5));
    insert_open_cycle(&c, "sb_ok", 1, Some(DCA_CYCLE));
    insert_open_cycle(&c, "sb_badcycle", 3, Some("{\"core\":{}}"));
    insert_open_cycle(&c, "sb_nostate", 2, None);

    let got = restore_check::unreadable_rows(&c).unwrap();
    assert_eq!(got.configs, vec!["sb_badcfg".to_string()]);
    assert_eq!(got.cycles, vec![("sb_badcycle".to_string(), 3), ("sb_nostate".to_string(), 2)]);
    // The load functions still skip them (restore keeps those bots stopped).
    assert_eq!(strategy::load_bots(&c).unwrap().len(), 3);
    assert_eq!(strategy::load_open_cycles(&c).unwrap().len(), 1);
}
