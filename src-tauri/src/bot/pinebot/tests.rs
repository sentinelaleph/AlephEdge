//! The Pine bot's rules, config and entry gates (spec section 12).

use crate::bot::live_terms::signal_terms;
use crate::bot::model::{BotConfig, BotKind, PineAction, PineSettings, PineTimeframe, TradeMode};
use crate::pine::{PineBar, PineErrorInfo, RunVerdict};
use crate::risk::model::RiskLevel;
use crate::signal::model::Direction;

use super::rules::*;
use super::{normalize_config, script_terms_changed};

const H: u64 = 3_600_000;
const T: u64 = 1_761_897_600_000; // 31 Oct 2026 08:00 UTC, an hour boundary

fn settings() -> PineSettings {
    let mut p = PineSettings { script: "//@version=5\nindicator(\"x\")\n".into(), symbols: vec!["BTCUSDT".into()], ..Default::default() };
    p.triggers.insert("alert#0".into(), PineAction::Buy);
    p.triggers.insert("alert#1".into(), PineAction::Sell);
    p.triggers.insert("shape#0".into(), PineAction::Close);
    p
}

fn form() -> BotConfig {
    let mut c = BotConfig::new_default(BotKind::Pine, "binance");
    c.max_positions = 4;
    c.pine = Some(settings());
    c
}

pub(super) fn saved(mode: TradeMode, live: bool) -> BotConfig {
    let mut c = normalize_config(None, form()).unwrap();
    (c.mode, c.live) = (mode, live);
    c
}

#[test]
fn the_pine_kind_is_a_user_driven_futures_bot_and_old_configs_still_read() {
    assert_eq!(BotKind::parse("pine"), Some(BotKind::Pine));
    assert_eq!(BotKind::Pine.as_str(), "pine");
    assert!(BotKind::Pine.uses_futures_market() && BotKind::Pine.live_capable() && BotKind::Pine.is_user_driven());
    assert!(crate::bot::ALL_KINDS.contains(&BotKind::Pine));
    // A config saved before the Pine bot existed: no `pine` key, reads as None,
    // and an other kind's config is written without one.
    let fut = BotConfig::new_default(BotKind::Futures, "binance");
    let json = serde_json::to_string(&fut).unwrap();
    assert!(!json.contains("\"pine\""), "{json}");
    let back: BotConfig = serde_json::from_str(&json).unwrap();
    assert!(back.pine.is_none());
    let pine = saved(TradeMode::Paper, false);
    let back: BotConfig = serde_json::from_str(&serde_json::to_string(&pine).unwrap()).unwrap();
    assert_eq!(back.pine, pine.pine);
    assert_eq!(serde_json::to_value(PineTimeframe::M15).unwrap(), "15m");
    let tv = serde_json::to_value(back.pine.unwrap()).unwrap();
    assert_eq!(tv["triggers"]["alert#0"], "buy");
    // Real money can be switched on for it on Binance futures.
    assert_eq!(crate::bot::commands::live_enable_refusal(true, BotKind::Pine, "binance", "LIVE", false), None);
}

#[test]
fn a_save_hashes_the_script_and_refuses_out_of_range_values() {
    let mut f = form();
    {
        let p = f.pine.as_mut().unwrap();
        p.symbols = vec![" btcusdt ".into(), "ETHUSDT".into(), "btcusdt".into(), "".into()];
        p.triggers.insert("shape#1".into(), PineAction::Ignore);
        p.script_hash = "forged".into();
        p.stop_input = Some("  ".into());
    }
    let c = normalize_config(None, f.clone()).unwrap();
    let p = c.pine.as_ref().unwrap();
    assert_eq!(p.script_hash, crate::pine::script_hash(&p.script), "recomputed from the text");
    assert_eq!(p.symbols, vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()]);
    assert_eq!(c.symbols, p.symbols);
    assert!(!p.triggers.contains_key("shape#1"), "Ignore is the default, not stored");
    assert_eq!(p.stop_input, None);
    assert_eq!(c.capital, 25.0, "margin per position = budget / positions");
    let bad = |edit: fn(&mut PineSettings)| {
        let mut f = form();
        edit(f.pine.as_mut().unwrap());
        normalize_config(None, f).unwrap_err()
    };
    assert_eq!(bad(|p| p.symbols = (0..11).map(|i| format!("A{i}USDT")).collect()), "pineSymbolsMax|10");
    assert_eq!(bad(|p| p.symbols.clear()), "pineSymbolsMin|1");
    assert_eq!(bad(|p| p.symbols = vec!["BTC-USD".into()]), "pineSymbolInvalid|BTC-USD");
    assert_eq!(bad(|p| p.script = " ".into()), "pineScriptEmpty");
    assert_eq!(bad(|p| p.warmup_bars = 299), "pineWarmupRange|300–5000");
    assert_eq!(bad(|p| p.warmup_bars = 5_001), "pineWarmupRange|300–5000");
    assert_eq!(bad(|p| p.max_opens_per_day = 201), "pineOpensPerDayRange|1–200");
    assert_eq!(bad(|p| p.stale_s = 14), "pineStaleRange|15–300");
    assert_eq!(bad(|p| p.price_guard_pct = 10.5), "pinePriceGuardRange|0–10");
    assert_eq!(bad(|p| p.stop_pct = Some(0.0)), "pineStopRange|0.1–50");
    assert_eq!(bad(|p| p.stop_pct = None), "pineStopRequired");
    assert_eq!(bad(|p| p.take_profit_pct = None), "pineTargetRequired");
    assert_eq!(bad(|p| p.max_size_usdt = 4.0), "pineMaxSizeInvalid|5");
    // A script stop alone is enough for Paper.
    let mut f = form();
    (f.pine.as_mut().unwrap().stop_pct, f.pine.as_mut().unwrap().stop_input) = (None, Some("Stop %".into()));
    assert!(normalize_config(None, f).is_ok());
    // Any other kind carries no Pine settings.
    let mut fut = BotConfig::new_default(BotKind::Futures, "binance");
    fut.pine = Some(settings());
    assert!(normalize_config(None, fut).unwrap().pine.is_none());
}

#[test]
fn live_without_a_bot_level_stop_is_refused() {
    let mut live = saved(TradeMode::Live, true);
    assert!(live.validate().is_ok());
    let p = live.pine.as_mut().unwrap();
    (p.stop_pct, p.stop_input) = (None, Some("Stop %".into()));
    assert_eq!(live.validate().unwrap_err(), "pineLiveStopRequired", "the script cannot turn the stop off");
    assert_eq!(live.pine.as_ref().unwrap().live_refusal(), Some("pineLiveStopRequired"));
    // A save of a Live bot that drops its stop is refused.
    let prev = saved(TradeMode::Live, false);
    let mut next = form();
    (next.pine.as_mut().unwrap().stop_pct, next.pine.as_mut().unwrap().stop_input) = (None, Some("Stop %".into()));
    assert_eq!(normalize_config(Some(&prev), next.clone()).unwrap_err(), "pineLiveStopRequired");
    assert!(normalize_config(Some(&saved(TradeMode::Paper, false)), next).is_ok(), "Paper may read its stop from the script");
}

#[test]
fn an_edit_of_the_script_triggers_inputs_or_bars_disarms_a_live_bot() {
    let armed = saved(TradeMode::Live, true);
    let same = normalize_config(Some(&armed), BotConfig { live: true, mode: TradeMode::Live, ..armed.clone() }).unwrap();
    assert!(same.live, "an unchanged save keeps the arm");
    assert!(!script_terms_changed(&armed, &same));
    assert!(!crate::bot::commands::live_terms_changed(&armed, &same));
    let edits: [fn(&mut PineSettings); 7] = [
        |p| p.script.push_str("plot(close)\n"),
        |p| _ = p.triggers.insert("alert#0".into(), PineAction::Sell),
        |p| _ = p.inputs.insert("0".into(), serde_json::json!(14)),
        |p| p.timeframe = PineTimeframe::H4,
        |p| p.symbols.push("ETHUSDT".into()),
        |p| p.stop_pct = Some(3.0),
        |p| p.max_size_usdt = 60.0,
    ];
    for (i, edit) in edits.iter().enumerate() {
        let mut next = BotConfig { live: true, ..armed.clone() };
        edit(next.pine.as_mut().unwrap());
        let out = normalize_config(Some(&armed), next).unwrap();
        assert!(!out.live, "edit {i} disarms");
        assert!(crate::bot::commands::live_terms_changed(&armed, &out), "edit {i}: apply_config disarms and stops");
    }
    // The money the arm confirms is the budget, the largest position the size.
    let t = signal_terms(&armed, &RiskLevel::Balanced.limits());
    assert_eq!((t.margin_at_risk_usdt, t.max_notional_usdt), (100.0, 50.0));
}

#[test]
fn close_comes_first_buy_and_sell_conflict_and_a_side_flips() {
    let p = settings();
    let f = |ids: &[&str]| fired(&ids.iter().map(|s| s.to_string()).collect::<Vec<_>>(), &p);
    let (long, short) = (Some(Direction::Long), Some(Direction::Short));
    assert!(!f(&["unmapped#9"]).any(), "an unmapped trigger is Ignore");
    assert_eq!(plan(&f(&["shape#0"]), long), PinePlan { close: true, open: None, note: None });
    assert_eq!(plan(&f(&["shape#0"]), None), PinePlan { close: false, open: None, note: Some(NOTHING_TO_CLOSE) });
    assert_eq!(plan(&f(&["alert#0", "alert#1"]), None), PinePlan { close: false, open: None, note: Some(CONFLICT) });
    assert_eq!(plan(&f(&["alert#0", "alert#1", "shape#0"]), short), PinePlan { close: true, open: None, note: Some(CONFLICT) }, "Close still runs first");
    assert_eq!(plan(&f(&["alert#0"]), None), PinePlan { close: false, open: long, note: None });
    assert_eq!(plan(&f(&["alert#1"]), long), PinePlan { close: true, open: short, note: None }, "flip: close then open");
    assert_eq!(plan(&f(&["alert#0"]), long), PinePlan { close: false, open: None, note: Some(ALREADY_OPEN) });
    assert_eq!(plan(&f(&["alert#0", "shape#0"]), long), PinePlan { close: true, open: long, note: None }, "closed, then the Buy opens");
    assert_eq!(f(&["alert#1", "alert#0"]).ids.iter().map(|(i, _)| i.as_str()).collect::<Vec<_>>(), vec!["alert#1", "alert#0"]);
}

#[test]
fn a_bar_runs_once_after_its_close_and_is_dropped_when_stale() {
    assert_eq!(last_close(T + 59_000, H), T);
    assert_eq!(last_close(T, H), T, "at the boundary the bar just closed");
    assert!(!due(T + 4_999, T) && due(T + 5_000, T));
    assert!(!stale(T + 60_000, T, 60) && stale(T + 60_001, T, 60));
    assert!(stale(T + 16_000, T, 15));
    assert_eq!(event_key("ab12", "BTCUSDT", T), format!("pine:ab12:BTCUSDT:{T}"));
    assert_eq!(event_key("ab12", "BTCUSDT", T), event_key("ab12", "BTCUSDT", T), "stable");
    // The entry's client id is derived from it (manual-pine-, never re-sent).
    let id = crate::bot::engine::user_entry::signal_id(BotKind::Pine, &event_key("ab12", "BTCUSDT", T));
    assert!(id.starts_with("manual-pine-"));
}

fn bars(n: u64, last_open: u64) -> Vec<PineBar> {
    (0..n).map(|i| PineBar { t: last_open - (n - 1 - i) * H, o: 1.0, h: 1.0, l: 1.0, c: 100.0, v: 1.0 }).collect()
}

#[test]
fn the_script_runs_only_on_bars_ending_with_the_one_just_closed() {
    let p = settings();
    let seen = std::cell::Cell::new((0usize, 0u64));
    let fake = |_: &str, b: &[PineBar], _: &crate::pine::InputOverrides, _: &str, iv: u64| -> Result<RunVerdict, PineErrorInfo> {
        seen.set((b.len(), iv));
        Ok(RunVerdict { fired: vec!["alert#0".into()], bars: b.len(), ms: 7, numeric_inputs: vec![] })
    };
    // The newest closed bar opened an hour before the boundary: run.
    match run_symbol(&p, &bars(300, T - H), T, "BTCUSDT", &fake) {
        RunOutcome::Ran(v) => assert_eq!(v.fired, vec!["alert#0".to_string()]),
        other => panic!("{other:?}"),
    }
    assert_eq!(seen.get(), (300, H));
    // The exchange has not published it yet: retried, nothing run.
    seen.set((0, 0));
    assert_eq!(run_symbol(&p, &bars(300, T - 2 * H), T, "BTCUSDT", &fake), RunOutcome::BarMissing);
    assert_eq!(run_symbol(&p, &[], T, "BTCUSDT", &fake), RunOutcome::BarMissing);
    assert_eq!(seen.get(), (0, 0), "the script never ran");
    // A run error orders nothing and is reported as is.
    let err = PineErrorInfo { kind: "runtime".into(), message: "boom".into(), line: 3, col: 1 };
    let failing = |_: &str, _: &[PineBar], _: &crate::pine::InputOverrides, _: &str, _: u64| Err(err.clone());
    assert_eq!(run_symbol(&p, &bars(300, T - H), T, "BTCUSDT", &failing), RunOutcome::Failed(err.clone()));
    assert_eq!(super::phase::failure_line("Trend", "BTCUSDT", "1h", &err), "Pine bot run failed: BTCUSDT 1h (Trend) · runtime: boom (line 3)");
}

#[test]
fn three_failed_runs_in_a_row_halt_a_live_bot_only() {
    let mut live = FailStreak::default();
    assert!(!live.record(false, true) && !live.record(false, true));
    assert!(!live.record(true, true), "a good run resets the streak");
    assert!(!live.record(false, true) && !live.record(false, true));
    assert!(live.record(false, true), "the third in a row halts");
    assert!(!live.record(false, true), "halted once");
    let mut paper = FailStreak::default();
    assert!((0..5).all(|_| !paper.record(false, false)), "Paper never halts");
    assert!(notice_due(None, T) && !notice_due(Some(T), T + NOTICE_EVERY_MS - 1) && notice_due(Some(T), T + NOTICE_EVERY_MS));
}

#[test]
fn the_price_guard_and_the_bots_limits_refuse_with_numbers() {
    assert_eq!(price_refusal(100.0, 100.5, 0.5), Ok(()));
    assert_eq!(price_refusal(100.0, 100.73, 0.5), Err("pinePriceMoved|0.73%".into()));
    assert_eq!(price_refusal(100.0, 98.0, 2.5), Ok(()));
    assert_eq!(price_refusal(0.0, 98.0, 2.5), Err("priceUnavailable".into()));
    let cfg = saved(TradeMode::Paper, false);
    let p = cfg.pine.clone().unwrap();
    let none = Usage::default();
    assert_eq!(limit_refusal(&cfg, &p, 50.0, 3, none), Ok(()));
    assert_eq!(limit_refusal(&cfg, &p, 50.0, 3, Usage { opens_today: 20, ..none }), Err("pineDailyOpensFull|20".into()));
    assert_eq!(limit_refusal(&cfg, &p, 50.0, 3, Usage { open: 4, ..none }), Err("pinePositionsFull|4".into()));
    assert_eq!(limit_refusal(&cfg, &p, 60.0, 1, Usage { open: 1, margin_usdt: 50.0, opens_today: 0 }), Err("pineBudgetFull|50.00".into()));
}

#[test]
fn the_bot_stop_is_the_floor_of_the_scripts_stop() {
    let mut p = settings();
    let inputs = vec![("Stop %".to_string(), 3.5), ("Target %".to_string(), 7.0), ("Bad".to_string(), 0.0)];
    assert_eq!(exits(&p, &inputs), Ok((2.0, 4.0)), "no inputs named: the bot's");
    (p.stop_input, p.target_input) = (Some("Stop %".into()), Some("Target %".into()));
    assert_eq!(exits(&p, &inputs), Ok((2.0, 7.0)), "a wider script stop never widens the bot's");
    p.stop_pct = Some(5.0);
    assert_eq!(exits(&p, &inputs), Ok((3.5, 7.0)), "a tighter one is used");
    p.stop_pct = None;
    assert_eq!(exits(&p, &inputs), Ok((3.5, 7.0)));
    p.stop_input = Some("Missing".into());
    assert_eq!(exits(&p, &inputs), Err("pineInputNotFound|Missing".into()));
    p.stop_input = Some("Bad".into());
    assert_eq!(exits(&p, &inputs), Err("pineInputLevelInvalid|Bad".into()));
}
