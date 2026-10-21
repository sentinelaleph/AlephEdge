//! Fixture generator for the LIVE-build update proof. Copied, with
//! `update_common.rs`, into the 0.3.18 release tree and run there as that
//! release's LIVE installer was built (`--features live`):
//!
//!   AE_UPDATE_FIXTURE_OUT=<empty scratch dir> cargo test --features live --lib generate_live_update_fixture -- --ignored
//!
//! It writes, with that release's own code, the data dir a LIVE-build user
//! has when they install the one build over it: the Futures signal bot on
//! real money (config saved before LIVE was typed, so `live: false` on disk,
//! as `bot_set_live` never saved it), one open REAL position with its
//! exchange stop and take-profit, one closed real trade with its fills and
//! commission, a paper Spot bot running with a paper position and a closed
//! paper trade, and one live DCA bot mid-cycle whose real position the
//! mirror holds (synced quantity, exchange stop, pilot) with its real fills.
//! `expected.json` is what that release itself holds at that moment.
//!
//! Never touches a real app data dir. Only APIs that exist unchanged in both
//! 0.3.18 and 0.3.19 are used, so this file compiles in both trees.

use serde_json::{json, Value};

use crate::bot::model::{BotConfig, BotKind, OpenPosition};
use crate::bot::strategy::venue::PaperVenue;
use crate::store::strategy as db;
use crate::store::update_common::*;
use crate::store::StoreManager;

const OPEN_POSITION: &str = include_str!("fixtures/v0_2_0/open_position.json");

/// The live DCA bot of this scenario.
pub const LIVE_DCA: &str = BOT_F_DCA_LIVE_TIED;
pub const REAL_OPEN: &str = "lv-sig-real-open";
pub const REAL_CLOSED: &str = "lv-sig-real-closed";
pub const PAPER_OPEN: &str = "lv-sig-paper-open";
pub const PAPER_CLOSED: &str = "lv-sig-paper-closed";

fn position(signal_id: &str, kind: &str, symbol: &str, live: bool, patch: Value) -> OpenPosition {
    let mut v: Value = serde_json::from_str(OPEN_POSITION).unwrap();
    v["signalId"] = json!(signal_id);
    v["botKind"] = json!(kind);
    v["symbol"] = json!(symbol);
    v["live"] = json!(live);
    v["openedAt"] = json!(T0 + 20 * 60_000);
    v["horizonMs"] = json!(T0 + 7 * 86_400_000);
    for (k, val) in patch.as_object().unwrap() {
        v[k] = val.clone();
    }
    serde_json::from_value(v).expect("position")
}

/// 0.3.18's `strategy_live_fills` table, as `strategy_live::ensure_table`
/// creates it on the first real fill (private there: the same statement).
const LIVE_FILLS_TABLE: &str = "CREATE TABLE IF NOT EXISTS strategy_live_fills (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            bot_id TEXT NOT NULL,
            seq INTEGER NOT NULL,
            ts INTEGER NOT NULL,
            client_id TEXT,
            order_id INTEGER,
            kind TEXT NOT NULL,
            signed_qty REAL NOT NULL,
            price REAL NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_strategy_live_fills_bot ON strategy_live_fills(bot_id, seq);";

#[test]
#[ignore]
#[allow(clippy::assertions_on_constants)] // false in a paper tree: run it with --features live
fn generate_live_update_fixture() {
    let out = std::path::PathBuf::from(std::env::var("AE_UPDATE_FIXTURE_OUT").expect("AE_UPDATE_FIXTURE_OUT"));
    assert_scratch_dir(&out);
    assert!(crate::bot::model::LIVE_TRADING_ENABLED, "run with --features live: this is the LIVE build's data");
    std::fs::create_dir_all(&out).unwrap();
    assert!(!out.join("edge.db").exists(), "output dir must be fresh");
    let store = StoreManager::new();

    // ---- the live DCA bot, walked to SPLIT like the engine does.
    let (_, sym, scale, grid, armed) = BOTS.iter().copied().find(|b| b.0 == LIVE_DCA).unwrap();
    let mut h = Held { bot: new_bot(LIVE_DCA, dca_config("Live DCA", sym, armed), T0 - 3_600_000), cycle: None, venue: PaperVenue::from_open(Vec::new()) };
    let b = bars(grid, scale);
    let mut log = Vec::new();
    store
        .strategy(&out, |c| {
            db::save_bot(c, &h.bot, T0)?;
            walk(&mut h, &b[..SPLIT], Some(c), &mut log)
        })
        .unwrap();
    let cycle = h.cycle.as_ref().expect("the live DCA bot holds an open cycle");
    let seq = cycle.core.seq;
    // What the mirror sent for that cycle, and holds now.
    let live = crate::bot::strategy_live::LiveState {
        enabled: true,
        synced_qty: 0.75,
        seq,
        order_n: 2,
        prepared_seq: seq,
        stop_algo_id: Some(333),
        stop_px: Some(2.61),
        last_real_qty: 0.75,
        last_entry_price: 2.94,
        last_unrealized: -0.4,
        last_sync_ms: T0 + SPLIT as u64 * 60_000,
        symbol: Some(sym.to_string()),
        pilot_cycles_left: 2,
        cycle_factor: 0.5,
        venue: "binance".into(),
        ..Default::default()
    };
    let mut states = std::collections::BTreeMap::new();
    states.insert(LIVE_DCA.to_string(), live.clone());
    store.set_meta(&out, "strategy_live_state", &serde_json::to_string(&states).unwrap()).unwrap();
    store
        .strategy(&out, |c| {
            c.execute_batch(LIVE_FILLS_TABLE).map_err(|e| e.to_string())?;
            for (n, (q, px)) in [(0.5, 2.97), (0.25, 2.88)].into_iter().enumerate() {
                c.execute(
                    "INSERT INTO strategy_live_fills(bot_id, seq, ts, client_id, order_id, kind, signed_qty, price) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                    rusqlite::params![LIVE_DCA, seq, (T0 + 60_000 * (10 + n as u64)) as i64, format!("cl-{n}"), 900 + n as i64, "open", q, px],
                )
                .map_err(|e| e.to_string())?;
            }
            Ok(())
        })
        .unwrap();

    // ---- signal bots. Futures on real money: its config was saved before
    // LIVE was typed (bot_set_live never saves), so the file says paper.
    let mut fut = BotConfig::new_default(BotKind::Futures, "binance");
    fut.capital = 60.0;
    fut.max_positions = 2;
    fut.leverage = 3;
    let spot = BotConfig::new_default(BotKind::Spot, "binance");
    for cfg in [&fut, &spot] {
        store.set_meta(&out, &format!("bot_config.{}", cfg.kind.as_str()), &serde_json::to_string(cfg).unwrap()).unwrap();
    }
    // A LIVE build saves only paper bots as running.
    store.set_meta(&out, "running_kinds", "spot").unwrap();

    let real_open = position(
        REAL_OPEN,
        "futures",
        "BTCUSDT",
        true,
        json!({"direction": "long", "entry": 67_000.0, "tp": 69_000.0, "sl": 65_500.0, "signalEntry": 66_950.0,
               "riskR": 1_500.0, "leverage": 3, "capital": 60.0, "qty": 0.002, "notionalUsdt": 134.0,
               "effectiveLeverage": 2.233, "entryOrderId": 111, "stopAlgoId": 222, "tpAlgoId": 223,
               "breakevenArmed": false, "sizingMode": "fixed", "tpTarget": "tp1"}),
    );
    let paper_open = position(
        PAPER_OPEN,
        "spot",
        "SOLUSDT",
        false,
        json!({"direction": "long", "entry": 150.0, "tp": 160.0, "sl": 145.0, "signalEntry": 149.8,
               "riskR": 5.0, "leverage": 1, "qty": 0.5, "sizingMode": "fixed", "tpTarget": "tp1"}),
    );
    for p in [&real_open, &paper_open] {
        store.upsert_position(&out, &p.signal_id, p.bot_kind.as_str(), &serde_json::to_string(p).unwrap(), T0 + 21 * 60_000).unwrap();
    }
    // A real round trip, settled on the exchange's fills and commission.
    let real_closed = position(
        REAL_CLOSED,
        "futures",
        "ETHUSDT",
        true,
        json!({"direction": "short", "entry": 2_500.0, "tp": 2_400.0, "sl": 2_560.0, "leverage": 3, "capital": 60.0,
               "qty": 0.06, "notionalUsdt": 150.0, "entryOrderId": 121, "stopAlgoId": 122, "sizingMode": "fixed"}),
    );
    store.upsert_position(&out, REAL_CLOSED, "futures", &serde_json::to_string(&real_closed).unwrap(), T0).unwrap();
    let settlement = crate::bot::engine::live_close::Settlement { entry: 2_499.5, exit: 2_401.2, qty: 0.06, commission_usdt: Some(0.148), exit_is_fill: true };
    let rec = crate::bot::engine::live_close::live_trade_record(&real_closed, &settlement, "tp", T0 + 80 * 60_000);
    assert!(rec.live, "a live build records a real trade as live");
    store.settle_close(&out, &rec, REAL_CLOSED, "futures").unwrap();
    let paper_closed = position(PAPER_CLOSED, "spot", "DOGEUSDT", false, json!({"direction": "long", "leverage": 1}));
    store.upsert_position(&out, PAPER_CLOSED, "spot", &serde_json::to_string(&paper_closed).unwrap(), T0).unwrap();
    let paper_rec = crate::bot::engine::book::trade_record(&paper_closed, paper_closed.tp, "tp", T0 + 85 * 60_000);
    store.settle_close(&out, &paper_rec, PAPER_CLOSED, "spot").unwrap();

    // ---- risk settings and the desk identity.
    let risk = crate::risk::RiskManager::load(Some(out.clone()));
    risk.set_level(&out, crate::risk::model::RiskLevel::Balanced);
    risk.set_balance(&out, 1_800.0);
    let desk = crate::app::endpoints::desk_id(&out);

    // ---- what THIS release reads back from its own store.
    let (counts, meta, trades, stats_real, stats_paper, pos_rows, fills, bots_back, cycles_back) = store
        .strategy(&out, |c| {
            let mut st = c
                .prepare("SELECT bot_id, seq, ts, client_id, order_id, kind, signed_qty, price FROM strategy_live_fills ORDER BY id")
                .map_err(|e| e.to_string())?;
            let fills: Vec<Value> = st
                .query_map([], |r| {
                    Ok(json!([
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<i64>>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, f64>(6)?,
                        r.get::<_, f64>(7)?
                    ]))
                })
                .map_err(|e| e.to_string())?
                .filter_map(Result::ok)
                .collect();
            Ok((
                table_counts(c)?,
                meta_dump(c)?,
                json!(super::trades::list(c, 100)?),
                json!(super::trades::stats_scoped(c, T0, Some(true))?),
                json!(super::trades::stats_scoped(c, T0, Some(false))?),
                super::positions::load_all_keyed(c)?,
                fills,
                json!(db::load_bots(c)?),
                json!(db::load_open_cycles(c)?),
            ))
        })
        .unwrap();

    let expected = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "liveBuild": crate::bot::model::LIVE_TRADING_ENABLED,
        "readBack": {
            "tableCounts": counts,
            "meta": meta,
            "trades": trades,
            "statsReal": stats_real,
            "statsPaper": stats_paper,
            "positionRows": pos_rows.iter().map(|(s, k, b)| json!([s, k, b])).collect::<Vec<_>>(),
            "liveFills": fills,
            "bots": bots_back,
            "openCycles": cycles_back,
        },
        "signal": {
            "configs": {"futures": json!(fut), "spot": json!(spot)},
            "positions": [json!(real_open), json!(paper_open)],
        },
        "strategyLive": json!(states),
        "deskId": desk,
    });
    std::fs::write(out.join("expected.json"), serde_json::to_string_pretty(&expected).unwrap()).unwrap();
    eprintln!("LIVE fixture written to {} by {}", out.display(), env!("CARGO_PKG_VERSION"));
}
