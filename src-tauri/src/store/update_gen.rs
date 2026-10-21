//! Fixture generator for the update-persistence proof. Copied into an OLDER
//! release tree (0.3.16, 0.3.18) and run there:
//!
//!   AE_UPDATE_FIXTURE_OUT=<empty scratch dir> cargo test --lib generate_update_fixture -- --ignored
//!
//! It writes, with that release's own code, the data dir a paper user has at
//! the moment they click Install: `edge.db` (signal bots, open positions, a
//! closed trade, six DCA / Grid bots mid-cycle, settings in `meta`),
//! `config.json` (risk settings), `desk-id`, and `expected.json` (what that
//! release itself holds in memory at that moment, plus the walk it would have
//! made had there been no update). The vault is written by
//! `vault::update_vault`. Never touches a real app data dir.

use serde_json::{json, Value};

use crate::bot::model::{BotConfig, BotKind, OpenPosition};
use crate::bot::strategy::cycle::CycleState;
use crate::bot::strategy::venue::PaperVenue;
use crate::risk::model::RiskLevel;
use crate::risk::RiskManager;
use crate::store::strategy as db;
use crate::store::update_common::*;
use crate::store::StoreManager;

const OPEN_POSITION: &str = include_str!("fixtures/v0_2_0/open_position.json");
const BOT_CONFIG_FUTURES: &str = include_str!("fixtures/v0_2_0/bot_config_futures.json");
const BOT_CONFIG_SPOT: &str = include_str!("fixtures/v0_2_0/bot_config_spot.json");

fn position(signal_id: &str, kind: &str, symbol: &str, live: bool, patch: Value) -> OpenPosition {
    let mut v: Value = serde_json::from_str(OPEN_POSITION).unwrap();
    v["signalId"] = json!(signal_id);
    v["botKind"] = json!(kind);
    v["symbol"] = json!(symbol);
    v["live"] = json!(live);
    v["openedAt"] = json!(T0 + 30 * 60_000);
    v["horizonMs"] = json!(T0 + 7 * 86_400_000);
    for (k, val) in patch.as_object().unwrap() {
        v[k] = val.clone();
    }
    serde_json::from_value(v).expect("position")
}

#[test]
#[ignore]
fn generate_update_fixture() {
    let out = std::path::PathBuf::from(std::env::var("AE_UPDATE_FIXTURE_OUT").expect("AE_UPDATE_FIXTURE_OUT"));
    assert_scratch_dir(&out);
    std::fs::create_dir_all(&out).unwrap();
    assert!(!out.join("edge.db").exists(), "output dir must be fresh");
    let store = StoreManager::new();

    // ---- DCA / Grid: walk to SPLIT, persisting every pass like the engine.
    let mut held: Vec<Held> = Vec::new();
    for (i, (id, sym, scale, grid, armed)) in BOTS.iter().enumerate() {
        let cfg = if *grid { grid_config("Upd grid", sym) } else { dca_config(&format!("Upd {id}"), sym, *armed) };
        let mut h = Held { bot: new_bot(id, cfg, T0 - 3_600_000 + i as u64), cycle: None, venue: PaperVenue::from_open(Vec::new()) };
        let b = bars(*grid, *scale);
        let mut log = Vec::new();
        store
            .strategy(&out, |c| {
                db::save_bot(c, &h.bot, T0)?;
                walk(&mut h, &b[..SPLIT], Some(c), &mut log)
            })
            .unwrap();
        held.push(h);
    }
    // The user stopped C (its open cycle stays managed); D's feed went stale.
    use crate::bot::strategy::model::BotRunState;
    held[2].bot.state = BotRunState::Stopped;
    held[3].bot.state = BotRunState::Paused;
    let split_ms = T0 + SPLIT as u64 * 60_000;
    store
        .strategy(&out, |c| {
            db::save_bot(c, &held[2].bot, split_ms)?;
            db::save_bot(c, &held[3].bot, split_ms)
        })
        .unwrap();

    // Scenario sanity (in THIS release's terms).
    let a = held[0].cycle.as_ref().expect("A holds an open cycle");
    let so = a.so_filled().unwrap_or(0);
    assert!(so >= 1 && so < 5, "A: partial safety-order ladder, got {so}");
    assert!(held[1].cycle.is_some() && !held[1].cycle.as_ref().unwrap().open_orders().is_empty(), "B: grid with live levels");
    assert!(held[4].cycle.is_none() && held[4].bot.state == BotRunState::Armed, "E: armed, no cycle");
    for i in [2, 3, 5] {
        assert!(held[i].cycle.is_some(), "bot {i} holds an open cycle");
    }

    // ---- signal bots, positions, a closed trade.
    let mut fut: BotConfig = serde_json::from_str(BOT_CONFIG_FUTURES).unwrap();
    fut.capital = 150.0;
    let spot: BotConfig = serde_json::from_str(BOT_CONFIG_SPOT).unwrap();
    let mut pump = BotConfig::new_default(BotKind::Pump, "binance");
    pump.live = true; // a live build saved it switched to real money
    let configs = [(BotKind::Futures, &fut), (BotKind::Spot, &spot), (BotKind::Pump, &pump)];
    for (kind, cfg) in configs {
        store
            .set_meta(&out, &format!("bot_config.{}", kind.as_str()), &serde_json::to_string(cfg).unwrap())
            .unwrap();
    }
    // Futures and spot running, pump stopped (the user's paused signal bot).
    store.set_meta(&out, "running_kinds", "futures,spot").unwrap();
    store.set_meta(&out, "tripped_day", &(T0 - 86_400_000 * 3).to_string()).unwrap();

    let positions = vec![
        position("upd-sig-futures-1", "futures", "KASUSDT", false, json!({})),
        position(
            "upd-sig-spot-1",
            "spot",
            "SOLUSDT",
            false,
            json!({"direction": "long", "entry": 150.0, "tp": 160.0, "sl": 145.0, "signalEntry": 149.8,
                   "riskR": 5.0, "leverage": 1, "qty": 0.5, "partialFraction": 0.5, "partialPrice": 155.0,
                   "partialQty": 0.5, "breakevenArmed": true, "stopAtBreakeven": true, "timeframe": "1h"}),
        ),
        // A real position a live build opened: a paper build must keep it out
        // of its book and leave the row untouched.
        position("upd-sig-live-1", "futures", "BTCUSDT", true, json!({"entryOrderId": 111, "stopAlgoId": 222})),
    ];
    for p in &positions {
        store
            .upsert_position(&out, &p.signal_id, p.bot_kind.as_str(), &serde_json::to_string(p).unwrap(), T0 + 31 * 60_000)
            .unwrap();
    }
    let closed = position("upd-sig-closed-1", "futures", "DOGEUSDT", false, json!({}));
    store
        .upsert_position(&out, &closed.signal_id, "futures", &serde_json::to_string(&closed).unwrap(), T0)
        .unwrap();
    let rec = crate::bot::engine::book::trade_record(&closed, closed.tp, "tp", T0 + 90 * 60_000);
    store.settle_close(&out, &rec, &closed.signal_id, "futures").unwrap();

    // ---- settings kept in meta.
    store.set_meta(&out, "strategy_risk", r#"{"portfolioDdPct":12.0,"budgetCapPct":40.0}"#).unwrap();
    store.set_meta(&out, "strategy_portfolio_peak", "1234.5").unwrap();
    store.set_meta(&out, "strategy_portfolio_tripped_day", "0").unwrap();
    let mut live = std::collections::BTreeMap::new();
    // F: switched to real money in a live build (must stay stopped here).
    live.insert(
        BOT_F_DCA_LIVE_TIED.to_string(),
        crate::bot::strategy_live::LiveState { enabled: true, seq: 1, symbol: Some("UPDFUSDT".into()), venue: "binance".into(), ..Default::default() },
    );
    // A: once live, switched off and flat (must resume on paper).
    live.insert(
        BOT_A_DCA_RUNNING.to_string(),
        crate::bot::strategy_live::LiveState { enabled: false, symbol: Some("UPDAUSDT".into()), venue: "binance".into(), ..Default::default() },
    );
    store.set_meta(&out, "strategy_live_state", &serde_json::to_string(&live).unwrap()).unwrap();
    let tg = crate::telegram::Settings {
        mode: Some(crate::telegram::Mode::Official),
        enabled: true,
        chat_id: Some(424_242_424),
        chat_label: Some("Upd Test Chat".into()),
        official_linked: true,
        official_chat_label: Some("Upd Test Chat".into()),
        official_linked_at: Some(T0),
        update_offset: 77,
        ..Default::default()
    };
    store.set_meta(&out, "telegram.settings", &serde_json::to_string(&tg).unwrap()).unwrap();
    store.set_meta(&out, "tvlink.state", r#"{"opaque":"kept byte for byte"}"#).unwrap();

    // ---- config.json (risk level, balance, daily stop) and desk-id.
    let risk = RiskManager::load(Some(out.clone()));
    risk.set_level(&out, RiskLevel::Balanced);
    risk.set_balance(&out, 2500.0);
    risk.set_close_on_stop(&out, false);
    risk.set_daily_loss_override(&out, Some(3.0));
    let desk = crate::app::endpoints::desk_id(&out);

    // ---- what THIS release reads back from its own store.
    let (open_orders, bots_back, cycles_back, counts, columns, meta, trades, stats, pos_rows) = store
        .strategy(&out, |c| {
            let mut oo = serde_json::Map::new();
            for h in &held {
                oo.insert(h.bot.id.clone(), json!(sorted_orders(db::load_open_orders(c, &h.bot.id)?)));
            }
            Ok((
                Value::Object(oo),
                json!(db::load_bots(c)?),
                json!(db::load_open_cycles(c)?),
                table_counts(c)?,
                table_columns(c)?,
                meta_dump(c)?,
                json!(super::trades::list(c, 100)?),
                json!(super::trades::stats_scoped(c, T0, None)?),
                super::positions::load_all_keyed(c)?,
            ))
        })
        .unwrap();
    let mut cursors = serde_json::Map::new();
    store
        .strategy(&out, |c| {
            for (_, sym, ..) in BOTS {
                cursors.insert(sym.to_string(), json!(db::get_cursor(c, "binance", "futures", sym)?));
            }
            Ok(())
        })
        .unwrap();

    // ---- the walk this release makes with NO update (uninterrupted), all
    // bots but F (a live-tied bot is stopped by any restart in a paper build).
    let mut cont_log = Vec::new();
    let mut final_bots = serde_json::Map::new();
    let mut final_cycles = serde_json::Map::new();
    let mut final_orders = serde_json::Map::new();
    for (h, (id, _, scale, grid, _)) in held.iter().zip(BOTS.iter()) {
        if *id == BOT_F_DCA_LIVE_TIED {
            continue;
        }
        let mut h = h.clone();
        let b = bars(*grid, *scale);
        walk(&mut h, &b[SPLIT..], None, &mut cont_log).unwrap();
        final_bots.insert(id.to_string(), json!(h.bot));
        final_cycles.insert(id.to_string(), json!(h.cycle));
        final_orders.insert(id.to_string(), json!(sorted_orders(h.cycle.as_ref().map(CycleState::open_orders).unwrap_or_default())));
    }

    let expected = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "memory": {
            "bots": held.iter().map(|h| json!(h.bot)).collect::<Vec<_>>(),
            "cycles": held.iter().map(|h| (h.bot.id.clone(), json!(h.cycle))).collect::<serde_json::Map<_, _>>(),
        },
        "readBack": {
            "bots": bots_back,
            "openCycles": cycles_back,
            "openOrders": open_orders,
            "cursors": Value::Object(cursors),
            "tableCounts": counts,
            "tableColumns": columns,
            "meta": meta,
            "trades": trades,
            "stats": stats,
            "positionRows": pos_rows.iter().map(|(s, k, b)| json!([s, k, b])).collect::<Vec<_>>(),
        },
        "signal": {
            "configs": {"futures": json!(fut), "spot": json!(spot), "pump": json!(pump)},
            "positions": positions.iter().map(|p| json!(p)).collect::<Vec<_>>(),
        },
        "settings": {
            "risk": json!(RiskManager::load(Some(out.clone())).state()),
            "telegram": json!(tg),
            "deskId": desk,
        },
        "continuation": {
            "fills": cont_log,
            "bots": Value::Object(final_bots),
            "cycles": Value::Object(final_cycles),
            "openOrders": Value::Object(final_orders),
        },
    });
    std::fs::write(out.join("expected.json"), serde_json::to_string_pretty(&expected).unwrap()).unwrap();
    eprintln!("fixture written to {} by {}", out.display(), env!("CARGO_PKG_VERSION"));
}

/// Downgrade: an OLDER build opens a store a NEWER build has written to
/// (after the reader's walk). Run on the older tree:
///   AE_UPDATE_DOWNGRADE_IN=<dir> cargo test --lib older_build_reads_newer_store -- --ignored
#[test]
#[ignore]
fn older_build_reads_newer_store() {
    let root = std::path::PathBuf::from(std::env::var("AE_UPDATE_DOWNGRADE_IN").expect("AE_UPDATE_DOWNGRADE_IN"));
    let mut n = 0;
    for e in std::fs::read_dir(&root).unwrap() {
        let src = e.unwrap().path();
        if !src.join("edge.db").exists() {
            continue;
        }
        let exp: Value = serde_json::from_str(&std::fs::read_to_string(src.join("expected.json")).unwrap()).unwrap();
        let dir = std::env::temp_dir().join(format!("ae-upd-down-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(src.join("edge.db"), dir.join("edge.db")).unwrap();
        let store = StoreManager::new();
        let (bots, cycles, unreadable) = store
            .strategy(&dir, |c| Ok((db::load_bots(c)?, db::load_open_cycles(c)?, super::restore_check::unreadable_rows(c)?)))
            .unwrap();
        assert!(unreadable.is_empty(), "{}: {unreadable:?}", src.display());
        assert_eq!(bots.len(), BOTS.len(), "{}: bots lost", src.display());
        for b in &bots {
            if b.id == BOT_F_DCA_LIVE_TIED {
                continue;
            }
            assert_eq!(json!(b), exp["continuation"]["bots"][&b.id], "{}: {}", src.display(), b.id);
            let c = cycles.iter().find(|c| c.core.bot_id == b.id);
            assert_eq!(json!(c), exp["continuation"]["cycles"][&b.id], "{}: cycle {}", src.display(), b.id);
        }
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
        n += 1;
    }
    assert!(n > 0, "no store in {}", root.display());
}
