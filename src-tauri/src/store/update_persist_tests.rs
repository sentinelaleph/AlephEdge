//! Update-persistence proof, reader side. Every fixture under
//! `fixtures/update/<release>/` is a data dir written by that OLDER release's
//! own code at the moment its user clicks Install (see `update_gen.rs`). Each
//! test copies it to a fresh temp dir and opens it with THIS build, exactly as
//! the updated app does on its first start, then checks that:
//!
//! - no table, column, row or meta value is lost or changed by the migration;
//! - DCA / Grid bots, open cycles (safety-order ladders, grid levels) and open
//!   orders load identical, and each bot resumes in the state it left;
//! - the walk after the restart equals the walk the old build would have made
//!   with no update at all (no lost, doubled or moved fill), and a second
//!   restart restores that new state again;
//! - signal-bot configs, running set, open positions, the trade history and the
//!   settings (risk config.json, Telegram link, desk-id) survive;
//! - a process exit in the middle of a write (the Windows updater's hard exit)
//!   leaves the last committed state.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::bot::model::{BotConfig, BotKind, OpenPosition};
use crate::bot::strategy::cycle::CycleState;
use crate::bot::strategy::driver;
use crate::bot::strategy::model::{BotRunState, StrategyBot};
use crate::bot::strategy::venue::PaperVenue;
use crate::bot::strategy::{Feed, StrategyManager};
use crate::risk::RiskManager;
use crate::store::strategy as db;
use crate::store::update_common::*;
use crate::store::StoreManager;

fn fixtures() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/store/fixtures/update");
    let mut v: Vec<PathBuf> = std::fs::read_dir(&root)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.join("edge.db").exists()).collect())
        .unwrap_or_default();
    v.sort();
    assert!(!v.is_empty(), "no update fixtures under {}", root.display());
    v
}

fn name(fx: &Path) -> String {
    fx.file_name().unwrap().to_string_lossy().to_string()
}

/// A throwaway copy of the fixture's data dir (never the committed files).
fn stage(fx: &Path) -> PathBuf {
    let tmp = std::env::temp_dir().join(format!("ae-upd-{}-{}", name(fx), uuid::Uuid::new_v4()));
    assert_scratch_dir(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    for f in ["edge.db", "config.json", "desk-id"] {
        std::fs::copy(fx.join(f), tmp.join(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
    }
    tmp
}

fn expected(fx: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(fx.join("expected.json")).unwrap()).unwrap()
}

fn keyed_cycles(cycles: &[CycleState]) -> Value {
    let mut m = serde_json::Map::new();
    for c in cycles {
        m.insert(c.core.bot_id.clone(), json!(c));
    }
    Value::Object(m)
}

fn keyed_cycles_json(v: &Value) -> Value {
    let mut m = serde_json::Map::new();
    for c in v.as_array().unwrap() {
        m.insert(c["core"]["botId"].as_str().unwrap().to_string(), c.clone());
    }
    Value::Object(m)
}

/// Pretty diff context on failure.
fn same(what: &str, fx: &Path, got: &Value, want: &Value) {
    if got != want {
        panic!(
            "{} [{}]: differs after the update\n--- old build wrote:\n{}\n--- this build reads:\n{}",
            what,
            name(fx),
            serde_json::to_string_pretty(want).unwrap(),
            serde_json::to_string_pretty(got).unwrap()
        );
    }
}

#[test]
fn an_older_store_opens_with_every_row_intact() {
    for fx in fixtures() {
        let exp = expected(&fx);
        let rb = &exp["readBack"];
        let dir = stage(&fx);
        let store = StoreManager::new();
        // First open = this build's migrations over the old file.
        let (counts, columns, meta, bots, cycles, orders, trades, stats, rows, unreadable) = store
            .strategy(&dir, |c| {
                let mut oo = serde_json::Map::new();
                for (id, ..) in BOTS {
                    oo.insert(id.to_string(), json!(sorted_orders(db::load_open_orders(c, id)?)));
                }
                Ok((
                    table_counts(c)?,
                    table_columns(c)?,
                    meta_dump(c)?,
                    json!(db::load_bots(c)?),
                    db::load_open_cycles(c)?,
                    Value::Object(oo),
                    json!(super::trades::list(c, 100)?),
                    json!(super::trades::stats_scoped(c, T0, None)?),
                    super::positions::load_all_keyed(c)?,
                    super::restore_check::unreadable_rows(c)?,
                ))
            })
            .unwrap();
        // Schema: every old table and column still there, no row lost.
        for (t, cols) in rb["tableColumns"].as_object().unwrap() {
            let now = columns.get(t).unwrap_or_else(|| panic!("[{}] table {t} dropped", name(&fx)));
            for col in cols.as_array().unwrap() {
                assert!(now.as_array().unwrap().contains(col), "[{}] column {t}.{col} dropped", name(&fx));
            }
            assert_eq!(counts[t], rb["tableCounts"][t], "[{}] row count of {t} changed", name(&fx));
        }
        same("meta table", &fx, &meta, &rb["meta"]);
        same("strategy bots", &fx, &bots, &rb["bots"]);
        same("strategy bots vs memory", &fx, &bots, &exp["memory"]["bots"]);
        same("open cycles", &fx, &keyed_cycles(&cycles), &keyed_cycles_json(&rb["openCycles"]));
        let mem_cycles: serde_json::Map<String, Value> = exp["memory"]["cycles"]
            .as_object()
            .unwrap()
            .iter()
            .filter(|(_, v)| !v.is_null())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        same("open cycles vs memory", &fx, &keyed_cycles(&cycles), &Value::Object(mem_cycles));
        same("open orders", &fx, &orders, &rb["openOrders"]);
        same("trade history", &fx, &trades, &rb["trades"]);
        same("trade stats", &fx, &stats, &rb["stats"]);
        same("position rows", &fx, &json!(rows.iter().map(|(s, k, b)| json!([s, k, b])).collect::<Vec<_>>()), &rb["positionRows"]);
        assert!(unreadable.is_empty(), "[{}] rows this build cannot use: {unreadable:?}", name(&fx));
        // The ladder / grid really is mid-flight in the fixture.
        let a = cycles.iter().find(|c| c.core.bot_id == BOT_A_DCA_RUNNING).unwrap();
        let so = a.so_filled().unwrap_or(0);
        assert!((1..5).contains(&so), "[{}] A safety orders filled: {so}", name(&fx));
        assert!(!orders[BOT_B_GRID_RUNNING].as_array().unwrap().is_empty(), "grid levels");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The restore of `strategy::engine::restore`, without the Tauri handle:
/// load (every bot Stopped), then resume the bots that were running unless
/// the stored real-money state ties them to the exchange. The Start re-checks
/// (`resume_refusal`: balance, budget cap, leverage ceiling) need the app and
/// are not run here.
fn restore(store: &StoreManager, dir: &Path) -> StrategyManager {
    let live_raw = store.get_meta(dir, "strategy_live_state").unwrap();
    let stored = crate::bot::strategy_live::parse_stored(Ok(live_raw.as_deref()));
    assert!(!stored.whole_unreadable && stored.unreadable.is_empty(), "live state unreadable");
    // Same filter as strategy_live::live_ids.
    let live: HashSet<String> = stored
        .states
        .iter()
        .filter(|(_, s)| s.enabled || s.synced_qty != 0.0 || s.last_real_qty != 0.0)
        .map(|(id, _)| id.clone())
        .collect();
    let (bots, cycles, venues, cursors) = store
        .strategy(dir, |c| {
            let bots = db::load_bots(c)?;
            let cycles = db::load_open_cycles(c)?;
            let mut venues = Vec::new();
            let mut cursors = Vec::new();
            for b in &bots {
                venues.push((b.id.clone(), PaperVenue::from_open(db::load_open_orders(c, &b.id)?)));
                if let Some((ms, px)) = db::get_cursor(c, &b.cfg.exchange_id, b.cfg.market.as_str(), &b.cfg.symbol)? {
                    cursors.push(((b.cfg.market, b.cfg.symbol.clone()), Feed { last_close_ms: ms, last_close: px, last_ok_ms: 0 }));
                }
            }
            Ok((bots, cycles, venues, cursors))
        })
        .unwrap();
    let was_running: Vec<StrategyBot> = bots.iter().filter(|b| driver::is_running(b.state)).cloned().collect();
    let mgr = StrategyManager::new();
    mgr.load(bots, cycles, venues, cursors);
    for b in was_running.iter().filter(|b| !live.contains(&b.id)) {
        mgr.resume(&b.id, b.state);
    }
    mgr
}

#[test]
fn bots_resume_in_the_state_they_left() {
    for fx in fixtures() {
        let exp = expected(&fx);
        let dir = stage(&fx);
        let store = StoreManager::new();
        let mgr = restore(&store, &dir);
        let want = [
            (BOT_A_DCA_RUNNING, BotRunState::InCycle, true),
            (BOT_B_GRID_RUNNING, BotRunState::InCycle, true),
            (BOT_C_DCA_STOPPED, BotRunState::Stopped, true),
            (BOT_D_DCA_PAUSED, BotRunState::Paused, true),
            (BOT_E_DCA_ARMED, BotRunState::Armed, false),
            // Tied to real money by a live build: stopped, its cycle kept.
            (BOT_F_DCA_LIVE_TIED, BotRunState::Stopped, true),
        ];
        for (id, state, cycle) in want {
            let b = mgr.bot(id).unwrap_or_else(|| panic!("[{}] bot {id} vanished", name(&fx)));
            assert_eq!(b.state, state, "[{}] {id} resumed as {:?}", name(&fx), b.state);
            assert_eq!(mgr.has_open_cycle(id), cycle, "[{}] {id} open cycle", name(&fx));
            same(
                &format!("{id} open orders after restore"),
                &fx,
                &json!(sorted_orders(mgr.open_orders(id))),
                &exp["readBack"]["openOrders"][id],
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn the_walk_after_the_update_is_the_walk_without_it() {
    for fx in fixtures() {
        let exp = expected(&fx);
        let cont = &exp["continuation"];
        let dir = stage(&fx);
        let store = StoreManager::new();
        let mgr = restore(&store, &dir);
        let mut log = Vec::new();
        let mut finals: Vec<Held> = Vec::new();
        for (id, _, scale, grid, _) in BOTS {
            if id == BOT_F_DCA_LIVE_TIED {
                continue;
            }
            let (bot, cycle, venue) = mgr.take_state(id).expect("restored state");
            let mut h = Held { bot, cycle, venue };
            let b = bars(grid, scale);
            store.strategy(&dir, |c| walk(&mut h, &b[SPLIT..], Some(c), &mut log)).unwrap();
            same(&format!("{id} bot after the walk"), &fx, &json!(h.bot), &cont["bots"][id]);
            same(&format!("{id} cycle after the walk"), &fx, &json!(h.cycle), &cont["cycles"][id]);
            same(
                &format!("{id} open orders after the walk"),
                &fx,
                &json!(sorted_orders(h.cycle.as_ref().map(CycleState::open_orders).unwrap_or_default())),
                &cont["openOrders"][id],
            );
            finals.push(h);
        }
        same("every fill after the update", &fx, &json!(log), &cont["fills"]);
        assert!(log.len() > 10, "[{}] the continuation must trade: {} fills", name(&fx), log.len());

        // Second restart (the next app start): what this build wrote restores.
        drop(store);
        let store2 = StoreManager::new();
        let mgr2 = restore(&store2, &dir);
        for h in &finals {
            let b = mgr2.bot(&h.bot.id).unwrap();
            let mut want = h.bot.clone();
            // A Stopped bot stays Stopped; the rest resume as they were.
            want.state = b.state;
            same(&format!("{} after a second restart", h.bot.id), &fx, &json!(b), &json!(want));
            assert_eq!(b.state == BotRunState::Stopped, h.bot.state == BotRunState::Stopped || h.bot.state == BotRunState::Dead);
            let (_, c2, _) = mgr2.take_state(&h.bot.id).unwrap();
            same(&format!("{} cycle after a second restart", h.bot.id), &fx, &json!(c2), &json!(h.cycle));
        }
        let unreadable = store2.strategy(&dir, super::restore_check::unreadable_rows).unwrap();
        assert!(unreadable.is_empty());

        // Downgrade input: this build's store, for the older build to read.
        if let Ok(dg) = std::env::var("AE_UPDATE_DOWNGRADE_OUT") {
            let d = PathBuf::from(dg).join(name(&fx));
            std::fs::create_dir_all(&d).unwrap();
            let snap = d.join("edge.db");
            let _ = std::fs::remove_file(&snap);
            store2.backup_into(&dir, &snap).unwrap();
            std::fs::copy(fx.join("expected.json"), d.join("expected.json")).unwrap();
        }
        drop(store2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn signal_bots_positions_and_settings_survive() {
    for fx in fixtures() {
        let exp = expected(&fx);
        let dir = stage(&fx);
        let desk_before = std::fs::read(dir.join("desk-id")).unwrap();
        let store = StoreManager::new();
        // Configs: every kind reads back; `live` is never restored (the same
        // rule as BotManager::restore_configs).
        for kind in [BotKind::Futures, BotKind::Spot, BotKind::Pump] {
            let raw = store.get_meta(&dir, &format!("bot_config.{}", kind.as_str())).unwrap().expect("config row");
            let cfg: BotConfig = serde_json::from_str(&raw).unwrap_or_else(|e| panic!("[{}] {kind:?}: {e}", name(&fx)));
            same(&format!("{} config", kind.as_str()), &fx, &json!(cfg), &exp["signal"]["configs"][kind.as_str()]);
            assert_eq!(cfg.kind, kind);
            let mut restored = cfg.clone();
            restored.live = false;
            assert!(restored.validate().is_ok(), "[{}] {kind:?} config fails today's validation", name(&fx));
        }
        let running = store.get_meta(&dir, "running_kinds").unwrap().unwrap();
        let kinds: Vec<BotKind> = running.split(',').filter_map(|s| BotKind::parse(s.trim())).collect();
        assert_eq!(kinds, vec![BotKind::Futures, BotKind::Spot], "[{}] running signal bots", name(&fx));
        // Open positions: every row parses to the very position written.
        let rows = store.load_positions(&dir).unwrap();
        let got: Vec<Value> = rows
            .iter()
            .map(|(_, _, b)| json!(serde_json::from_str::<OpenPosition>(b).unwrap_or_else(|e| panic!("[{}] {e}", name(&fx)))))
            .collect();
        let mut want: Vec<Value> = exp["signal"]["positions"].as_array().unwrap().clone();
        let key = |v: &Value| v["signalId"].as_str().unwrap().to_string();
        let mut got_sorted = got.clone();
        got_sorted.sort_by_key(key);
        want.sort_by_key(key);
        same("open positions", &fx, &json!(got_sorted), &json!(want));
        let live_rows = got.iter().filter(|p| p["live"] == json!(true)).count();
        assert_eq!(live_rows, 1, "the live build's real position row is still on disk");
        assert!(store.is_settled(&dir, "upd-sig-closed-1", "futures", T0 + 30 * 60_000).unwrap(), "the closed trade is still settled");
        // Settings.
        same("risk settings (config.json)", &fx, &json!(RiskManager::load(Some(dir.clone())).state()), &exp["settings"]["risk"]);
        let tg_raw = store.get_meta(&dir, "telegram.settings").unwrap().unwrap();
        let mut tg: crate::telegram::Settings = serde_json::from_str(&tg_raw).unwrap();
        same("telegram settings", &fx, &json!(tg), &exp["settings"]["telegram"]);
        assert!(!tg.migrate(false), "a moded row is not migrated");
        assert_eq!(crate::app::endpoints::desk_id(&dir), exp["settings"]["deskId"].as_str().unwrap());
        assert_eq!(std::fs::read(dir.join("desk-id")).unwrap(), desk_before, "desk-id rewritten");
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Child-process half of `an_exit_mid_write_leaves_the_last_committed_state`:
/// opens the store, starts one bot's tick write and exits the process before
/// the commit, as the Windows updater's `process::exit` would.
#[test]
#[ignore]
fn crash_helper_exits_mid_write() {
    let Ok(dir) = std::env::var("AE_UPDATE_CRASH_DIR") else { return };
    let dir = PathBuf::from(dir);
    assert_scratch_dir(&dir);
    let c = rusqlite::Connection::open(dir.join("edge.db")).unwrap();
    let tx = c.unchecked_transaction().unwrap();
    let mut bots = db::load_bots(&tx).unwrap();
    let b = bots.iter_mut().find(|b| b.id == BOT_A_DCA_RUNNING).unwrap();
    b.state = BotRunState::Stopped;
    b.cycles_done += 99;
    db::save_bot(&tx, b, 1).unwrap();
    tx.execute("DELETE FROM strategy_orders", []).unwrap();
    tx.execute("UPDATE strategy_cycles SET state_json = NULL", []).unwrap();
    tx.execute("DELETE FROM open_positions", []).unwrap();
    std::process::exit(0);
}

#[test]
fn an_exit_mid_write_leaves_the_last_committed_state() {
    for fx in fixtures() {
        let exp = expected(&fx);
        let dir = stage(&fx);
        // Migrate once with this build (the updated app's first start).
        StoreManager::new().strategy(&dir, |_| Ok(())).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["store::update_persist_tests::crash_helper_exits_mid_write", "--exact", "--ignored", "--test-threads=1"])
            .env("AE_UPDATE_CRASH_DIR", &dir)
            .status()
            .unwrap();
        assert!(status.success());
        let store = StoreManager::new();
        let (bots, orders, unreadable, rows) = store
            .strategy(&dir, |c| {
                let mut oo = serde_json::Map::new();
                for (id, ..) in BOTS {
                    oo.insert(id.to_string(), json!(sorted_orders(db::load_open_orders(c, id)?)));
                }
                Ok((json!(db::load_bots(c)?), Value::Object(oo), super::restore_check::unreadable_rows(c)?, super::positions::load_all_keyed(c)?))
            })
            .unwrap();
        same("bots after an exit mid-write", &fx, &bots, &exp["readBack"]["bots"]);
        same("orders after an exit mid-write", &fx, &orders, &exp["readBack"]["openOrders"]);
        assert!(unreadable.is_empty(), "cycle state lost: {unreadable:?}");
        assert_eq!(rows.len(), exp["readBack"]["positionRows"].as_array().unwrap().len());
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
