//! LIVE build -> one build. `fixtures/update_live/<release>/` is the data dir
//! a LIVE-build user has when they install 0.3.19 over it, written by that
//! release's own code with `--features live` (`update_gen_live.rs`). Each test
//! opens a throwaway copy with THIS build, through the restore code the app
//! runs (`bot::restore::plan`, `strategy_live::parse_stored`), and checks:
//!
//! - the real-money bots become Live bots, never Paper: the Futures bot (its
//!   file said paper; its real position says otherwise) and the live DCA bot;
//! - they come back stopped and unarmed, and start again only after the
//!   typed LIVE; the paper Spot bot stays Paper and resumes;
//! - no real position, real fill, real trade or exchange-stop id is lost or
//!   changed, and real and paper results stay apart;
//! - a second start keeps the Live mode (the mapping runs once) and the
//!   schema is stamped.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::bot::model::{BotConfig, BotKind, OpenPosition, TradeMode};
use crate::bot::restore::{plan, MODE_MAP_KEY};
use crate::bot::strategy_live::{parse_stored, start_refusal_of};
use crate::bot::{start_mode_refusal, BotManager};
use crate::store::strategy as db;
use crate::store::update_common::*;
use crate::store::update_gen_live::{LIVE_DCA, PAPER_OPEN, REAL_CLOSED, REAL_OPEN};
use crate::store::StoreManager;

fn fixtures() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/store/fixtures/update_live");
    let mut v: Vec<PathBuf> = std::fs::read_dir(&root)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.join("edge.db").exists()).collect())
        .unwrap_or_default();
    v.sort();
    assert!(!v.is_empty(), "no LIVE-build fixtures under {}", root.display());
    v
}

fn name(fx: &Path) -> String {
    fx.file_name().unwrap().to_string_lossy().to_string()
}

fn stage(fx: &Path) -> PathBuf {
    let tmp = std::env::temp_dir().join(format!("ae-upd-live-{}-{}", name(fx), uuid::Uuid::new_v4()));
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

fn cfg_of(configs: &[BotConfig], kind: BotKind) -> BotConfig {
    configs.iter().find(|c| c.kind == kind).cloned().unwrap_or_else(|| panic!("{kind:?} config lost"))
}

#[test]
fn the_fixture_is_a_live_builds_data() {
    for fx in fixtures() {
        let exp = expected(&fx);
        assert_eq!(exp["liveBuild"], json!(true), "[{}] written by a --features live build", name(&fx));
        // The file a LIVE build saved says paper: only the real row tells.
        assert_eq!(exp["signal"]["configs"]["futures"]["live"], json!(false));
    }
}

#[test]
fn real_money_bots_become_live_bots_stopped_until_their_typed_live() {
    for fx in fixtures() {
        let exp = expected(&fx);
        let dir = stage(&fx);
        let store = StoreManager::new();
        let p = plan(&store, &dir);

        // Signal bots: Futures held real money -> Live, unarmed; Spot Paper.
        let fut = cfg_of(&p.configs, BotKind::Futures);
        assert_eq!(fut.mode, TradeMode::Live, "[{}] the LIVE build's Futures bot is not silently Paper", name(&fx));
        assert!(!fut.live, "the arm is never restored");
        assert_eq!(start_mode_refusal(&fut), Some("liveArmRequired"), "stopped until the typed LIVE");
        let spot = cfg_of(&p.configs, BotKind::Spot);
        assert_eq!(spot.mode, TradeMode::Paper, "a paper bot stays paper");
        assert_eq!(start_mode_refusal(&spot), None, "and resumes");
        assert_eq!(p.mapped, vec![BotKind::Futures]);
        // Every other setting is the one the old build saved.
        let mut want = exp["signal"]["configs"]["futures"].clone();
        want["mode"] = json!("live");
        assert_eq!(json!(fut), want, "[{}] Futures settings changed", name(&fx));
        assert_eq!(json!(spot), exp["signal"]["configs"]["spot"]);
        // The resumed set: the old LIVE build saved only the paper bot.
        let running = store.get_meta(&dir, "running_kinds").unwrap().unwrap();
        assert_eq!(running, "spot");

        // Positions: the real one is in the book, byte for byte, still real;
        // nothing is held out, nothing is converted.
        assert!(p.held_labels.is_empty(), "[{}] held: {:?}", name(&fx), p.held_labels);
        assert!(p.blocked.is_empty());
        let mut got: Vec<Value> = p.positions.iter().map(|x| json!(x)).collect();
        let mut want: Vec<Value> = exp["signal"]["positions"].as_array().unwrap().clone();
        let key = |v: &Value| v["signalId"].as_str().unwrap().to_string();
        got.sort_by_key(key);
        want.sort_by_key(key);
        assert_eq!(got, want, "[{}] open positions differ", name(&fx));
        let real: &OpenPosition = p.positions.iter().find(|x| x.signal_id == REAL_OPEN).unwrap();
        assert!(real.live && real.is_live(), "the real position is managed as real");
        assert_eq!((real.entry_order_id, real.stop_algo_id, real.tp_algo_id), (Some(111), Some(222), Some(223)));
        assert!(!p.positions.iter().find(|x| x.signal_id == PAPER_OPEN).unwrap().live);

        // The desk counts it as money in play.
        let bots = BotManager::new();
        bots.add_position(real.clone());
        assert!(bots.has_live_exposure());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_live_dca_bot_stays_live_stopped_and_keeps_its_real_position() {
    for fx in fixtures() {
        let exp = expected(&fx);
        let dir = stage(&fx);
        let store = StoreManager::new();
        let raw = store.get_meta(&dir, "strategy_live_state").unwrap();
        let stored = parse_stored(Ok(raw.as_deref()));
        assert!(!stored.whole_unreadable && stored.unreadable.is_empty());
        let st = stored.states.get(LIVE_DCA).expect("live state kept");
        assert_eq!(json!(st), exp["strategyLive"][LIVE_DCA], "[{}] real-money state changed", name(&fx));
        assert!(st.enabled, "Live, never silently Paper");
        // Restored: not armed this session, so no new cycle starts.
        assert_eq!(start_refusal_of(st, false, true, true), Some("liveArmRequired"));
        // The strategy restore keeps it Stopped (it is tied to real money).
        let tied: HashSet<String> = stored
            .states
            .iter()
            .filter(|(_, s)| s.enabled || s.synced_qty != 0.0 || s.last_real_qty != 0.0)
            .map(|(id, _)| id.clone())
            .collect();
        assert!(tied.contains(LIVE_DCA));
        // Its simulated cycle (the decision book) and real fills are intact.
        let (bots, cycles, fills) = store
            .strategy(&dir, |c| {
                let n: i64 = c.query_row("SELECT COUNT(*) FROM strategy_live_fills WHERE bot_id = ?1", [LIVE_DCA], |r| r.get(0)).map_err(|e| e.to_string())?;
                Ok((json!(db::load_bots(c)?), json!(db::load_open_cycles(c)?), n))
            })
            .unwrap();
        assert_eq!(bots, exp["readBack"]["bots"]);
        assert_eq!(cycles, exp["readBack"]["openCycles"]);
        assert_eq!(fills, exp["readBack"]["liveFills"].as_array().unwrap().len() as i64);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn real_and_paper_results_stay_apart_and_nothing_is_lost() {
    for fx in fixtures() {
        let exp = expected(&fx);
        let dir = stage(&fx);
        let store = StoreManager::new();
        let _ = plan(&store, &dir);
        let rb = &exp["readBack"];
        let (counts, trades, real, paper) = store
            .strategy(&dir, |c| {
                Ok((
                    table_counts(c)?,
                    json!(super::trades::list(c, 100)?),
                    json!(super::trades::stats_scoped(c, T0, Some(true))?),
                    json!(super::trades::stats_scoped(c, T0, Some(false))?),
                ))
            })
            .unwrap();
        for (t, n) in rb["tableCounts"].as_object().unwrap() {
            if t == "meta" {
                continue; // + the once-key; checked value by value below
            }
            assert_eq!(&counts[t], n, "[{}] row count of {t} changed", name(&fx));
        }
        let meta = store.strategy(&dir, meta_dump).unwrap();
        for (k, v) in rb["meta"].as_object().unwrap() {
            if k == "bot_config.futures" {
                continue; // now carries "mode":"live" (checked above)
            }
            assert_eq!(&meta[k], v, "[{}] meta {k} changed", name(&fx));
        }
        assert_eq!(meta[MODE_MAP_KEY], json!("1"));
        assert_eq!(trades, rb["trades"], "[{}] trade history changed", name(&fx));
        assert_eq!(real, rb["statsReal"], "real results");
        assert_eq!(paper, rb["statsPaper"], "paper results");
        let real_trades: Vec<&Value> = trades.as_array().unwrap().iter().filter(|t| t["live"] == json!(true)).collect();
        assert_eq!(real_trades.len(), 1);
        assert_eq!(real_trades[0]["signalId"], json!(REAL_CLOSED));
        assert_eq!(super::SCHEMA_VERSION, store.strategy(&dir, |c| c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).map_err(|e| e.to_string())).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn the_mapping_runs_once_and_a_second_start_keeps_the_live_mode() {
    for fx in fixtures() {
        let dir = stage(&fx);
        let store = StoreManager::new();
        let first = plan(&store, &dir);
        assert_eq!(store.get_meta(&dir, MODE_MAP_KEY).unwrap().as_deref(), Some("1"));
        drop(store);
        let store = StoreManager::new();
        let second = plan(&store, &dir);
        assert!(second.mapped.is_empty(), "mapped once");
        assert_eq!(cfg_of(&second.configs, BotKind::Futures).mode, TradeMode::Live, "stored, not re-derived");
        assert_eq!(json!(first.positions), json!(second.positions));
        // Once mapped, a bot the user made Paper stays Paper: a real row
        // appearing on it is held out of the book, not followed.
        let mut paper = cfg_of(&second.configs, BotKind::Futures);
        paper.mode = TradeMode::Paper;
        store.set_meta(&dir, "bot_config.futures", &serde_json::to_string(&paper).unwrap()).unwrap();
        let third = plan(&store, &dir);
        assert_eq!(cfg_of(&third.configs, BotKind::Futures).mode, TradeMode::Paper);
        assert_eq!(third.held_labels, vec![format!("futures {REAL_OPEN}")]);
        assert!(third.blocked.contains(&BotKind::Futures));
        assert!(third.positions.iter().all(|x| !x.live), "no real row reaches the book of a Paper bot");
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
