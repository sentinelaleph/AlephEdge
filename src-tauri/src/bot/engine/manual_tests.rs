//! "Take this signal now": refusals, the filter bypass, the checks a manual
//! entry still meets, the manual flag, and parity of the loop's gate.

use super::btc_break::BtcRegime;
use super::entry::{pre_price_gate, EntryMode, GateInput};
use super::entry_rules::{
    exchange_minimum_gate, plan_entry, plan_entry_in, pre_price_gate_in, SKIP_STOP_PASSED, SKIP_TARGET_PASSED,
};
use super::manual::{outcome_usdt, preview_of, reward_risk, refusal, skip_code, PAPER_ONLY, SIGNAL_NOT_FOUND};
use super::sizing::size_position;
use crate::bot::model::{SizingMode, TakeProfitTarget};
use super::precheck::{judge_budget, Budget};
use super::test_fixtures::{cfg, position, signal, signal_at};
use crate::bot::model::{BotConfig, BotKind};
use crate::bot::BotManager;
use crate::risk::model::RiskLevel;
use crate::signal::model::Signal;
use crate::signal::time::parse_rfc3339_ms;

fn input<'a>(cfg: &'a BotConfig, sig: &'a Signal) -> GateInput<'a> {
    GateInput {
        cfg,
        sig,
        regime: BtcRegime::Normal,
        now_ms: parse_rfc3339_ms("2026-09-16T10:01:00Z").unwrap(),
        invalidated: false,
        regime_vetoed: false,
        pump_allowed: true,
        futures_supported: true,
        holds_market: false,
    }
}

fn key(skip: Option<super::precheck::Skip>) -> Option<&'static str> {
    skip.map(|s| s.key)
}

#[test]
fn refusals_come_in_order_with_their_codes() {
    let c = cfg();
    let sig = signal("long", 100.0, 106.0, 90.0);
    assert_eq!(refusal(None, false, Some(&sig), false), Err("botNotConfigured".into()));
    let mut live = c.clone();
    live.live = true;
    assert_eq!(refusal(Some(&live), false, Some(&sig), false), Err(PAPER_ONLY.into()));
    assert_eq!(PAPER_ONLY, "manualTakePaperOnly");
    // Real money is refused before anything else, the daily stop too.
    assert_eq!(refusal(Some(&live), true, None, true), Err(PAPER_ONLY.into()));
    assert_eq!(refusal(Some(&c), true, Some(&sig), false), Err("botDailyLossTripped".into()));
    let mut bad = c.clone();
    bad.max_positions = 0;
    assert!(refusal(Some(&bad), false, Some(&sig), false).is_err(), "an invalid saved config");
    assert_eq!(refusal(Some(&c), false, None, false), Err(SIGNAL_NOT_FOUND.into()));
    assert_eq!(refusal(Some(&c), false, Some(&sig), true), Err("marketHeld".into()));
    assert_eq!(refusal(Some(&c), false, Some(&sig), false), Ok(()));
}

// The user decided: the bot's own filters do not apply to a manual entry.
#[test]
fn the_users_filters_are_skipped_by_hand_only() {
    let sig = signal("long", 100.0, 106.0, 90.0);
    let filtered: [fn(&mut BotConfig); 5] = [
        |c| c.min_confidence = Some(0.95),
        |c| c.direction = Some("short".into()),
        |c| c.symbols = vec!["BTCUSDT".into()],
        |c| c.combos = vec!["msb+ob".into()],
        |c| c.engines = vec!["fvg".into()],
    ];
    for (i, f) in filtered.iter().enumerate() {
        let mut c = cfg();
        f(&mut c);
        assert!(key(pre_price_gate(&input(&c, &sig))).is_some(), "case {i}: the loop refuses");
        assert_eq!(key(pre_price_gate_in(&input(&c, &sig), EntryMode::Manual)), None, "case {i}: by hand passes");
    }
    // The age limit: two hours old against a one-hour limit.
    let old = signal_at("long", 100.0, 106.0, 90.0, "2026-09-16T08:00:00Z");
    let mut c = cfg();
    c.max_signal_age_min = 60;
    assert_eq!(key(pre_price_gate(&input(&c, &old))), Some("tooOld"));
    assert_eq!(key(pre_price_gate_in(&input(&c, &old), EntryMode::Manual)), None);
    // The Pump bot's pump rule.
    let mut pump = cfg();
    pump.kind = BotKind::Pump;
    assert_eq!(key(pre_price_gate(&input(&pump, &sig))), Some("notPumpSignal"));
    assert_eq!(key(pre_price_gate_in(&input(&pump, &sig), EntryMode::Manual)), None);
}

// Market fit, validity, geometry, the BTC guard and the held market are not
// the user's filters: a manual entry meets them like the loop does.
#[test]
fn a_manual_entry_keeps_every_other_gate() {
    let long = signal("long", 100.0, 106.0, 90.0);
    let manual = |i: &GateInput| key(pre_price_gate_in(i, EntryMode::Manual));
    let mut spot = cfg();
    spot.kind = BotKind::Spot;
    let short = signal("short", 100.0, 94.0, 110.0);
    assert_eq!(manual(&input(&spot, &short)), Some("spotLongOnly"));
    let c = cfg();
    let mut i = input(&c, &long);
    i.invalidated = true;
    assert_eq!(manual(&i), Some("invalidated"));
    let mut i = input(&c, &long);
    i.regime_vetoed = true;
    assert_eq!(manual(&i), Some("vetoedByRegime"));
    let mut i = input(&c, &long);
    i.now_ms = parse_rfc3339_ms("2099-01-02T00:00:00Z").unwrap();
    assert_eq!(manual(&i), Some("expired"));
    let mut i = input(&c, &long);
    i.futures_supported = false;
    assert_eq!(manual(&i), Some("spotOnlyExchange"));
    let inverted = signal("long", 100.0, 95.0, 90.0);
    assert_eq!(manual(&input(&c, &inverted)), Some("incoherentGeometry"));
    let mut i = input(&c, &long);
    i.regime = BtcRegime::Break;
    assert_eq!(manual(&i), Some("btcBreakGuard"));
    i.regime = BtcRegime::Unknown;
    assert_eq!(manual(&i), Some("btcGuardUnknown"));
    let mut i = input(&c, &long);
    i.holds_market = true;
    assert_eq!(manual(&i), Some("marketHeld"));
}

// After the gate a manual entry runs the same plan and prechecks: fill model,
// sizing, liquidation gate, exchange minimum, position and capital caps.
#[test]
fn risk_checks_still_apply_after_the_gate() {
    let sig = signal("long", 100.0, 106.0, 90.0);
    let late = parse_rfc3339_ms("2026-09-16T10:30:00Z").unwrap();
    let c = cfg();
    // Price above entry, past the marketable window: the fill model waits.
    assert!(plan_entry(&c, &sig, Some(100.3), late, 1).unwrap().is_none());
    assert_eq!(plan_entry(&c, &sig, None, late, 1).unwrap_err().key, "priceUnavailable");
    // A stop beyond liquidation at 20x is refused.
    let mut hot = cfg();
    hot.leverage = 20;
    assert!(plan_entry(&hot, &sig, Some(99.9), late, 20).is_err());
    // Below the exchange minimum.
    let mut tiny = cfg();
    tiny.capital = 2.0;
    let plan = plan_entry(&tiny, &sig, Some(99.9), late, 1).unwrap().unwrap();
    let skip = exchange_minimum_gate(&plan).unwrap_err();
    assert_eq!(skip_code(&skip), "belowExchangeMinimum|2.00");
    // Position and capital caps.
    let limits = RiskLevel::Balanced.limits();
    let full = Budget { global_open: 0, bot_open: usize::from(c.max_positions), balance: 10_000.0, notional: 100.0 };
    assert_eq!(judge_budget(&limits, &c, &full).unwrap_err().key, "botMaxPositions");
}

#[test]
fn the_preview_counts_both_fees_into_loss_and_gain() {
    // 100 USDT at 1x on the futures fee (0.05% a side): the stop 10% away
    // loses 10 + 0.10, the target 6% away gains 6 − 0.10.
    let pos = position(&signal("long", 100.0, 106.0, 90.0), None);
    let (loss, gain, fees) = outcome_usdt(&pos);
    assert!((loss + 10.1).abs() < 1e-9, "{loss}");
    assert!((gain - 5.9).abs() < 1e-9, "{gain}");
    assert!((fees - 0.1).abs() < 1e-9, "{fees}");
    let p = preview_of(&pos, 106.0);
    assert_eq!((p.entry, p.stop, p.target), (100.0, 90.0, 106.0));
    assert_eq!(p.tp_target, "tp1");
}

#[test]
fn the_manual_flag_is_stored_with_the_position_and_its_trade() {
    let mut pos = position(&signal("long", 100.0, 106.0, 90.0), None);
    // Every non-manual row is written as before: no new key.
    let body = serde_json::to_string(&pos).unwrap();
    assert!(!body.contains("manual"), "{body}");
    let back: crate::bot::model::OpenPosition = serde_json::from_str(&body).unwrap();
    assert!(!back.manual);
    pos.manual = true;
    let body = serde_json::to_string(&pos).unwrap();
    assert!(body.contains("\"manual\":true"));
    let back: crate::bot::model::OpenPosition = serde_json::from_str(&body).unwrap();
    assert!(back.manual, "survives a restart");
    let record = super::record::trade_record(&back, 106.0, "tp", 9);
    assert!(record.manual);
    let auto = super::record::trade_record(&position(&signal("long", 100.0, 106.0, 90.0), None), 106.0, "tp", 9);
    assert!(!serde_json::to_string(&auto).unwrap().contains("manual"));
}

#[test]
fn the_book_never_holds_a_signal_or_symbol_twice_for_one_bot() {
    let m = BotManager::new();
    let pos = position(&signal("long", 100.0, 106.0, 90.0), None);
    assert!(m.add_position_if_free(pos.clone()));
    assert!(!m.add_position_if_free(pos.clone()), "the same signal");
    let mut other_tf = pos.clone();
    (other_tf.signal_id, other_tf.timeframe) = ("sig-2".into(), Some("1h".into()));
    assert!(!m.add_position_if_free(other_tf.clone()), "the same symbol, another timeframe");
    assert!(m.holds_symbol(BotKind::Futures, "solusdt"));
    let mut spot = other_tf;
    spot.bot_kind = BotKind::Spot;
    assert!(m.add_position_if_free(spot), "another bot");
    assert_eq!(m.open_count(), 2);
}

// Runner parity: the loop's gate (which the VDS paper runner compiles) is
// exactly the Auto mode, case for case.
#[test]
fn the_loops_gate_is_unchanged() {
    let signals = [
        signal("long", 100.0, 106.0, 90.0),
        signal("short", 100.0, 94.0, 110.0),
        signal("long", 100.0, 95.0, 90.0),
        signal_at("long", 100.0, 106.0, 90.0, "2026-09-16T08:00:00Z"),
    ];
    let configs: Vec<BotConfig> = {
        let mut v = vec![cfg()];
        let mut c = cfg();
        c.min_confidence = Some(0.95);
        v.push(c);
        let mut c = cfg();
        c.kind = BotKind::Spot;
        v.push(c);
        let mut c = cfg();
        c.kind = BotKind::Pump;
        v.push(c);
        let mut c = cfg();
        c.max_signal_age_min = 60;
        v.push(c);
        v
    };
    for c in &configs {
        for s in &signals {
            for regime in [BtcRegime::Normal, BtcRegime::Break, BtcRegime::Unknown] {
                for held in [false, true] {
                    let mut i = input(c, s);
                    (i.regime, i.holds_market) = (regime, held);
                    assert_eq!(key(pre_price_gate(&i)), key(pre_price_gate_in(&i, EntryMode::Auto)));
                }
            }
        }
    }
}

const LATE: &str = "2026-09-16T12:00:00Z";

fn manual_plan(c: &BotConfig, sig: &Signal, price: f64) -> Result<Option<super::entry_rules::PlannedEntry>, &'static str> {
    plan_entry_in(c, sig, Some(price), parse_rfc3339_ms(LATE).unwrap(), c.leverage, EntryMode::Manual).map_err(|s| s.key)
}

// Two hours after publication, price 3% above a long's entry: the loop waits
// for a touch, a manual entry fills at the market price.
#[test]
fn a_manual_entry_fills_at_market_inside_the_band() {
    let c = cfg();
    let sig = signal("long", 100.0, 106.0, 90.0);
    let late = parse_rfc3339_ms(LATE).unwrap();
    assert!(plan_entry(&c, &sig, Some(103.0), late, 1).unwrap().is_none(), "the loop waits");
    let plan = manual_plan(&c, &sig, 103.0).unwrap().expect("fills now");
    assert_eq!((plan.price, plan.tp.tp, plan.tp.target.as_str()), (103.0, 106.0, "tp1"));
    // Below the entry too, and on the short side.
    assert_eq!(manual_plan(&c, &sig, 95.0).unwrap().unwrap().price, 95.0);
    let short = signal("short", 100.0, 94.0, 110.0);
    assert_eq!(manual_plan(&c, &short, 97.0).unwrap().unwrap().price, 97.0);
    assert_eq!(manual_plan(&c, &short, 108.0).unwrap().unwrap().price, 108.0);
}

#[test]
fn a_manual_entry_refuses_a_passed_target_or_stop() {
    let c = cfg();
    let long = signal("long", 100.0, 106.0, 90.0);
    for price in [106.0, 107.0] {
        let skip = plan_entry_in(&c, &long, Some(price), 0, 1, EntryMode::Manual).unwrap_err();
        assert_eq!((skip.key, skip_code(&skip).as_str()), (SKIP_TARGET_PASSED, "targetPassedBeforeEntry|TP1"));
    }
    for price in [90.0, 85.0] {
        assert_eq!(manual_plan(&c, &long, price).unwrap_err(), SKIP_STOP_PASSED);
    }
    let short = signal("short", 100.0, 94.0, 110.0);
    assert_eq!(manual_plan(&c, &short, 94.0).unwrap_err(), SKIP_TARGET_PASSED);
    assert_eq!(manual_plan(&c, &short, 110.0).unwrap_err(), SKIP_STOP_PASSED);
    assert_eq!(
        plan_entry_in(&c, &long, None, 0, 1, EntryMode::Manual).unwrap_err().key,
        "priceUnavailable"
    );
    // The band ends at the bot's own target: TP2 beyond a passed TP1.
    let mut tp2 = cfg();
    tp2.take_profit = TakeProfitTarget::Tp2;
    let mut sig = signal("long", 100.0, 106.0, 90.0);
    sig.tp.push(112.0);
    assert_eq!(manual_plan(&tp2, &sig, 108.0).unwrap().unwrap().tp.tp, 112.0);
    assert_eq!(manual_plan(&tp2, &sig, 112.5).unwrap_err(), SKIP_TARGET_PASSED);
}

// Sizing and the liquidation gate run on the actual fill, not the entry.
#[test]
fn a_manual_fill_is_sized_on_the_fill_price() {
    let mut c = cfg();
    c.sizing = SizingMode::Risk;
    c.leverage = 10;
    let sig = signal("long", 100.0, 106.0, 90.0);
    let plan = manual_plan(&c, &sig, 103.0).unwrap().unwrap();
    assert_eq!(plan.size, size_position(&c, 10, 103.0, 90.0).unwrap());
    assert_ne!(plan.size, size_position(&c, 10, 100.0, 90.0).unwrap());
    let mut hot = cfg();
    hot.leverage = 20;
    assert!(manual_plan(&hot, &sig, 103.0).is_err(), "stop beyond liquidation at 20x");
}

#[test]
fn the_preview_compares_reward_risk_at_the_fill_with_the_published_one() {
    let sig = signal("long", 100.0, 106.0, 90.0);
    assert!((reward_risk(100.0, 106.0, 90.0) - 0.6).abs() < 1e-12);
    let mut pos = position(&sig, None);
    pos.entry = 103.0;
    let p = preview_of(&pos, 106.0);
    assert_eq!((p.entry, p.published_entry), (103.0, 100.0));
    assert!((p.rr_at_fill - 3.0 / 13.0).abs() < 1e-12);
    assert!((p.rr_published - 0.6).abs() < 1e-12);
    assert!(p.rr_low, "below 0.5: most of the reward is gone");
    pos.entry = 95.0;
    let p = preview_of(&pos, 106.0);
    assert!((p.rr_at_fill - 11.0 / 5.0).abs() < 1e-12);
    assert!(!p.rr_low);
    assert_eq!(reward_risk(90.0, 106.0, 90.0), 0.0, "no risk, no ratio");
}

// The loop's plan (which the paper runner compiles) is exactly Auto mode.
#[test]
fn the_loops_plan_is_unchanged() {
    let c = cfg();
    let sigs = [signal("long", 100.0, 106.0, 90.0), signal("short", 100.0, 94.0, 110.0)];
    let times = ["2026-09-16T10:02:00Z", LATE].map(|t| parse_rfc3339_ms(t).unwrap());
    let view = |r: Result<Option<super::entry_rules::PlannedEntry>, super::precheck::Skip>| match r {
        Ok(Some(p)) => Ok(Some((p.price, p.tp.tp, p.size))),
        Ok(None) => Ok(None),
        Err(s) => Err(s.key),
    };
    for sig in &sigs {
        for now in times {
            for price in [85.0, 90.0, 94.0, 95.0, 99.9, 100.0, 100.3, 100.6, 103.0, 106.0, 110.0, 115.0] {
                assert_eq!(
                    view(plan_entry(&c, sig, Some(price), now, 1)),
                    view(plan_entry_in(&c, sig, Some(price), now, 1, EntryMode::Auto))
                );
            }
        }
    }
}

// The level's position limit counts one signal bot: Spot full at Calm's 5
// leaves Futures free, and Spot itself stops at the level's 5 even when its
// own setting allows 10.
#[test]
fn the_level_position_limit_applies_per_signal_bot() {
    let m = BotManager::new();
    for i in 0..5 {
        let mut pos = position(&signal("long", 100.0, 106.0, 90.0), None);
        pos.bot_kind = BotKind::Spot;
        (pos.signal_id, pos.symbol) = (format!("spot-{i}"), format!("coin{i}usdt"));
        assert!(m.add_position_if_free(pos));
    }
    let limits = RiskLevel::Calm.limits();
    let mut spot = cfg();
    (spot.kind, spot.max_positions) = (BotKind::Spot, 10);
    let mut futures = cfg();
    (futures.kind, futures.max_positions) = (BotKind::Futures, 2);
    let full = super::entry::signal_bot_budget(&m, BotKind::Spot, 10_000.0, 100.0);
    assert_eq!(judge_budget(&limits, &spot, &full).unwrap_err().key, "maxPositions");
    let free = super::entry::signal_bot_budget(&m, BotKind::Futures, 10_000.0, 100.0);
    assert!(judge_budget(&limits, &futures, &free).is_ok());
}
