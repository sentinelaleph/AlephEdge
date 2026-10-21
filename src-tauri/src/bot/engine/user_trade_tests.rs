//! "New trade": refusals, the synthetic signal through the manual entry
//! pipeline, the managed paper position closed by its take-profit and its
//! stop, and the manual record.

use super::btc_break::BtcRegime;
use super::entry::{market_mismatch, EntryMode, GateInput};
use super::entry_rules::{exchange_minimum_gate, finish_position, plan_entry_in, pre_price_gate_in};
use super::management::close_step;
use super::precheck::{judge_still_open, PrecheckContext};
use super::record::trade_record;
use super::test_fixtures::{cfg, position, signal};
use super::user_trade::{
    is_user_trade_id, level_refusal, mark_user_position, new_id, normalize_symbol, preview_of, refusal, rfc3339_utc,
    round_trip_fee, synthetic_signal, trade_config, TradeRequest, BOT_KIND, ID_PREFIX, LEVELS_INVALID, SIZE_INVALID,
    STOP_INSIDE_FEES, STOP_WRONG_SIDE, SYMBOL_INVALID, TARGET_INSIDE_FEES, TARGET_WRONG_SIDE,
};
use crate::bot::model::{BotConfig, BotKind, OpenPosition, SizingMode, TakeProfitTarget};
use crate::bot::BotManager;
use crate::signal::model::{Direction, OpenStatus};
use crate::signal::time::parse_rfc3339_ms;

const NOW: u64 = 1_790_000_000_000;

fn req(direction: Direction, tp: f64, sl: f64) -> TradeRequest {
    TradeRequest {
        kind: BotKind::Futures,
        symbol: "solusdt".into(),
        direction,
        notional_usdt: None,
        take_profit: tp,
        stop_loss: sl,
        mode: Default::default(),
        confirmation: None,
        confirmed_margin_usdt: None,
    }
}

/// The position `req` opens at `price` with `c`, as `user_trade::plan` builds
/// it after the network: synthetic signal → pre-price gate → manual plan →
/// exchange minimum → finish → manual flag.
fn open_at(c: &BotConfig, r: &TradeRequest, price: f64) -> OpenPosition {
    let symbol = refusal(Some(c), false, r, false).expect("no refusal");
    level_refusal(r.direction, price, r.take_profit, r.stop_loss, round_trip_fee(r.kind)).expect("levels");
    let tcfg = trade_config(c, r, c.leverage);
    let sig = synthetic_signal(r, &symbol, price, NOW, new_id(NOW, 7));
    assert!(market_mismatch(tcfg.kind, &sig).is_none());
    let input = GateInput {
        cfg: &tcfg,
        sig: &sig,
        regime: BtcRegime::Normal,
        now_ms: NOW + 1_000,
        invalidated: false,
        regime_vetoed: false,
        pump_allowed: true,
        futures_supported: true,
        holds_market: false,
    };
    assert!(pre_price_gate_in(&input, EntryMode::Manual).is_none(), "the manual gate passes");
    let plan = plan_entry_in(&tcfg, &sig, Some(price), NOW + 1_000, tcfg.leverage, EntryMode::Manual)
        .unwrap()
        .expect("a manual entry fills now");
    exchange_minimum_gate(&plan).unwrap();
    let ctx = PrecheckContext { fr_at_open: None, ld_at_open: None };
    let mut pos = finish_position(&tcfg, &sig, plan.price, &plan.tp, &plan, NOW + 1_000, &ctx);
    mark_user_position(&mut pos);
    pos
}

#[test]
fn refusals_come_in_order_with_their_codes() {
    let c = cfg();
    let r = req(Direction::Long, 110.0, 95.0);
    assert_eq!(refusal(None, false, &r, false), Err("botNotConfigured".into()));
    let mut live = c.clone();
    live.live = true;
    // Real money first, before anything else.
    assert_eq!(refusal(Some(&live), true, &r, true), Err("manualTakePaperOnly".into()));
    assert_eq!(refusal(Some(&c), true, &r, false), Err("botDailyLossTripped".into()));
    let mut bad = c.clone();
    bad.max_positions = 0;
    assert!(refusal(Some(&bad), false, &r, false).is_err(), "an invalid saved config");
    let pump = TradeRequest { kind: BotKind::Pump, ..r.clone() };
    assert_eq!(refusal(Some(&c), false, &pump, false), Err(BOT_KIND.into()));
    let spot_short = TradeRequest { kind: BotKind::Spot, ..req(Direction::Short, 90.0, 105.0) };
    assert_eq!(refusal(Some(&c), false, &spot_short, false), Err("spotLongOnly".into()));
    for s in ["", "SOL", "SOLBTC", "SOL-USDT", "USDT"] {
        let x = TradeRequest { symbol: s.into(), ..r.clone() };
        assert_eq!(refusal(Some(&c), false, &x, false), Err(SYMBOL_INVALID.into()), "{s:?}");
    }
    for (tp, sl) in [(0.0, 95.0), (110.0, -1.0), (f64::NAN, 95.0), (110.0, f64::INFINITY)] {
        let x = req(Direction::Long, tp, sl);
        assert_eq!(refusal(Some(&c), false, &x, false), Err(LEVELS_INVALID.into()));
    }
    let zero = TradeRequest { notional_usdt: Some(0.0), ..r.clone() };
    assert_eq!(refusal(Some(&c), false, &zero, false), Err(SIZE_INVALID.into()));
    assert_eq!(refusal(Some(&c), false, &r, true), Err("marketHeld".into()));
    assert_eq!(refusal(Some(&c), false, &r, false), Ok("SOLUSDT".into()));
    assert_eq!(normalize_symbol(" ethusdt "), Ok("ETHUSDT".into()));
}

#[test]
fn below_the_exchange_minimum_is_refused_with_the_skip_code() {
    let c = cfg();
    let tiny = TradeRequest { notional_usdt: Some(4.99), ..req(Direction::Long, 110.0, 95.0) };
    assert_eq!(refusal(Some(&c), false, &tiny, false), Err("belowExchangeMinimum|4.99".into()));
    let five = TradeRequest { notional_usdt: Some(5.0), ..tiny.clone() };
    assert!(refusal(Some(&c), false, &five, false).is_ok());
    // The bot's own sizing below the minimum: the shared gate refuses it.
    let mut small = cfg();
    small.capital = 2.0;
    let r = req(Direction::Long, 110.0, 95.0);
    let sig = synthetic_signal(&r, "SOLUSDT", 100.0, NOW, new_id(NOW, 1));
    let plan = plan_entry_in(&small, &sig, Some(100.0), NOW, 1, EntryMode::Manual).unwrap().unwrap();
    let skip = exchange_minimum_gate(&plan).unwrap_err();
    assert_eq!(super::manual::skip_code(&skip), "belowExchangeMinimum|2.00");
}

#[test]
fn levels_on_the_wrong_side_of_the_live_price_are_refused() {
    let fee = round_trip_fee(BotKind::Futures);
    assert_eq!(level_refusal(Direction::Long, 100.0, 99.0, 95.0, fee), Err(TARGET_WRONG_SIDE.into()));
    assert_eq!(level_refusal(Direction::Long, 100.0, 100.0, 95.0, fee), Err(TARGET_WRONG_SIDE.into()));
    assert_eq!(level_refusal(Direction::Long, 100.0, 110.0, 101.0, fee), Err(STOP_WRONG_SIDE.into()));
    assert_eq!(level_refusal(Direction::Short, 100.0, 101.0, 105.0, fee), Err(TARGET_WRONG_SIDE.into()));
    assert_eq!(level_refusal(Direction::Short, 100.0, 90.0, 99.0, fee), Err(STOP_WRONG_SIDE.into()));
    assert_eq!(level_refusal(Direction::Long, 100.0, 110.0, 95.0, fee), Ok(()));
    assert_eq!(level_refusal(Direction::Short, 100.0, 90.0, 105.0, fee), Ok(()));
}

#[test]
fn a_stop_or_target_inside_the_fees_is_refused() {
    // Futures: 0.05% a side, 0.10% round trip.
    let fee = round_trip_fee(BotKind::Futures);
    assert!((fee - 0.001).abs() < 1e-12);
    assert_eq!(level_refusal(Direction::Long, 100.0, 110.0, 99.95, fee), Err(format!("{STOP_INSIDE_FEES}|0.10%")));
    assert_eq!(level_refusal(Direction::Long, 100.0, 110.0, 99.9, fee), Err(format!("{STOP_INSIDE_FEES}|0.10%")));
    assert_eq!(level_refusal(Direction::Long, 100.0, 110.0, 99.8, fee), Ok(()));
    assert_eq!(level_refusal(Direction::Short, 100.0, 99.95, 105.0, fee), Err(format!("{TARGET_INSIDE_FEES}|0.10%")));
    // Spot: 0.10% a side, 0.20% round trip.
    let spot = round_trip_fee(BotKind::Spot);
    assert_eq!(level_refusal(Direction::Long, 100.0, 110.0, 99.85, spot), Err(format!("{STOP_INSIDE_FEES}|0.20%")));
}

#[test]
fn a_busy_symbol_is_refused() {
    let m = BotManager::new();
    let mut pos = position(&signal("long", 100.0, 106.0, 90.0), None);
    pos.symbol = "SOLUSDT".into();
    assert!(m.add_position_if_free(pos));
    let r = req(Direction::Long, 110.0, 95.0);
    let held = m.holds_symbol(r.kind, r.symbol.trim());
    assert!(held, "the Futures bot holds SOLUSDT (any case)");
    assert_eq!(refusal(Some(&cfg()), false, &r, held), Err("marketHeld".into()));
    assert!(!m.holds_symbol(BotKind::Spot, "solusdt"), "one position per symbol per bot");
}

#[test]
fn the_synthetic_signal_carries_the_users_levels_and_a_marked_id() {
    let r = req(Direction::Long, 110.0, 95.0);
    let id = new_id(NOW, 0xabc);
    assert_eq!(id, format!("manual-{NOW}-00000abc"));
    assert!(is_user_trade_id(&id) && id.starts_with(ID_PREFIX));
    assert!(!is_user_trade_id("4f1c2a7e-0d3b-4a51-9a0c-1e2f3a4b5c6d"), "a Sentinel id");
    let sig = synthetic_signal(&r, "SOLUSDT", 100.0, NOW, id);
    assert_eq!((sig.entry, sig.tp.clone(), sig.sl), (100.0, vec![110.0], 95.0));
    assert_eq!(sig.market_type.as_deref(), Some("futures"));
    assert!(sig.management_plan.is_none() && sig.timeframe.is_none() && sig.confluence.is_empty());
    assert_eq!(parse_rfc3339_ms(&sig.created_at), Some(NOW));
    assert_eq!(parse_rfc3339_ms(&sig.expires_at), Some(NOW + 3_600_000));
    let spot = synthetic_signal(&TradeRequest { kind: BotKind::Spot, ..r }, "SOLUSDT", 100.0, NOW, "manual-x".into());
    assert_eq!(spot.market_type.as_deref(), Some("spot"));
}

#[test]
fn rfc3339_round_trips() {
    for ms in [0, 951_782_400_000, 1_709_164_800_123, NOW, 4_102_444_799_999] {
        assert_eq!(parse_rfc3339_ms(&rfc3339_utc(ms)), Some(ms), "{}", rfc3339_utc(ms));
    }
    assert_eq!(rfc3339_utc(1_709_164_800_123), "2024-02-29T00:00:00.123Z");
}

// The bot's take-profit setting (TP2, a custom %, a per-symbol override)
// never moves the user's level; a USDT size is fixed sizing at the leverage.
#[test]
fn the_trade_uses_the_users_target_and_size() {
    let mut c = cfg();
    c.take_profit = TakeProfitTarget::Custom { pct: 40.0 };
    c.take_profit_overrides.insert("SOLUSDT".into(), TakeProfitTarget::Tp2);
    c.sizing = SizingMode::Risk;
    c.leverage = 4;
    let r = TradeRequest { notional_usdt: Some(200.0), ..req(Direction::Long, 110.0, 95.0) };
    let pos = open_at(&c, &r, 100.0);
    assert_eq!((pos.entry, pos.tp, pos.sl), (100.0, 110.0, 95.0));
    assert_eq!(pos.tp_target, "tp1");
    assert_eq!(pos.sizing_mode, SizingMode::Fixed);
    assert!((pos.notional_usdt - 200.0).abs() < 1e-9);
    assert!((pos.capital - 50.0).abs() < 1e-9, "margin = 200 / 4x");
    assert!((pos.qty - 2.0).abs() < 1e-9);
    // Without a size the bot's own sizing applies (risk: 1% of 100 at a 5% stop).
    let own = open_at(&c, &req(Direction::Long, 110.0, 95.0), 100.0);
    assert_eq!(own.sizing_mode, SizingMode::Risk);
    assert_eq!(own.capital, 100.0);
}

#[test]
fn a_user_trade_skips_only_the_sentinel_status_lookup() {
    // `precheck::run` asks Sentinel's status only for a published signal; a
    // strict level refuses an unknown status, which a user trade always has.
    assert_eq!(judge_still_open(OpenStatus::Unknown, true).unwrap_err().key, "signalStatusUnknown");
    let r = req(Direction::Long, 110.0, 95.0);
    let sig = synthetic_signal(&r, "SOLUSDT", 100.0, NOW, new_id(NOW, 3));
    assert!(is_user_trade_id(&sig.id), "precheck::run skips the lookup for it");
    // Every other gate still applies to it: the BTC guard and a held market.
    let c = cfg();
    let mut input = GateInput {
        cfg: &c,
        sig: &sig,
        regime: BtcRegime::Break,
        now_ms: NOW,
        invalidated: false,
        regime_vetoed: false,
        pump_allowed: true,
        futures_supported: true,
        holds_market: false,
    };
    assert_eq!(pre_price_gate_in(&input, EntryMode::Manual).map(|s| s.key), Some("btcBreakGuard"));
    input.regime = BtcRegime::Normal;
    input.holds_market = true;
    assert_eq!(pre_price_gate_in(&input, EntryMode::Manual).map(|s| s.key), Some("marketHeld"));
    // A stop beyond liquidation at 20x is refused by the shared plan.
    let mut hot = cfg();
    hot.leverage = 20;
    assert!(plan_entry_in(&hot, &sig, Some(100.0), NOW, 20, EntryMode::Manual).is_err());
}

#[test]
fn the_position_is_managed_and_closed_by_the_take_profit() {
    let pos = open_at(&cfg(), &req(Direction::Long, 110.0, 95.0), 100.0);
    assert!(pos.manual && pos.horizon_ms == 0 && pos.plan.is_none() && !pos.live);
    let mut p = pos.clone();
    assert_eq!(close_step(&mut p, 105.0, NOW + 10_000, false, None).exit, None, "between the levels");
    // No time limit: still open years later.
    assert_eq!(close_step(&mut p, 105.0, u64::MAX / 2, false, None).exit, None);
    let exit = close_step(&mut p, 110.5, NOW + 20_000, false, None).exit.expect("closes at TP");
    assert_eq!(exit.reason, "tp");
    let rec = trade_record(&p, exit.price, exit.reason, NOW + 20_000);
    assert!(rec.manual && rec.pnl_quote > 0.0);
    assert!(is_user_trade_id(&rec.signal_id));
}

#[test]
fn the_position_is_managed_and_closed_by_the_stop() {
    let mut p = open_at(&cfg(), &req(Direction::Short, 90.0, 105.0), 100.0);
    assert_eq!(p.direction, Direction::Short);
    assert_eq!(close_step(&mut p, 98.0, NOW + 10_000, false, None).exit, None);
    let exit = close_step(&mut p, 105.2, NOW + 20_000, false, None).exit.expect("closes at SL");
    assert_eq!(exit.reason, "sl");
    let rec = trade_record(&p, exit.price, exit.reason, NOW + 20_000);
    assert!(rec.manual && rec.pnl_quote < 0.0);
}

#[test]
fn the_preview_shows_risk_reward_liquidation_and_fees() {
    let mut c = cfg();
    c.leverage = 5;
    // 500 USDT at 5x: margin 100; stop 5% away loses 25 + fees 0.5.
    let r = TradeRequest { notional_usdt: Some(500.0), ..req(Direction::Long, 110.0, 95.0) };
    let pos = open_at(&c, &r, 100.0);
    let p = preview_of(&pos, c.capital);
    assert_eq!((p.entry, p.take_profit, p.stop_loss), (100.0, 110.0, 95.0));
    assert!((p.qty - 5.0).abs() < 1e-9);
    assert!((p.fees_usdt - 0.5).abs() < 1e-9, "{}", p.fees_usdt);
    assert!((p.loss_at_stop_usdt + 25.5).abs() < 1e-9, "{}", p.loss_at_stop_usdt);
    assert!((p.gain_at_target_usdt - 49.5).abs() < 1e-9, "{}", p.gain_at_target_usdt);
    assert!((p.risk_pct_of_capital - 25.5).abs() < 1e-9, "of the bot's 100 USDT");
    assert!((p.reward_risk - 2.0).abs() < 1e-12);
    assert_eq!(p.leverage, 5);
    let liq = p.liquidation_price.expect("a 5x long can be liquidated");
    assert!(liq < 95.0 && liq > 75.0, "{liq}");
    // Spot: no liquidation, 1x.
    let mut s = cfg();
    s.kind = BotKind::Spot;
    let spot = open_at(&s, &TradeRequest { kind: BotKind::Spot, ..req(Direction::Long, 110.0, 95.0) }, 100.0);
    let sp = preview_of(&spot, s.capital);
    assert_eq!((sp.liquidation_price, sp.leverage), (None, 1));
}

#[test]
fn the_request_reads_the_forms_json() {
    let r: TradeRequest = serde_json::from_value(serde_json::json!({
        "kind": "futures", "symbol": "ETHUSDT", "direction": "short",
        "takeProfit": 2900.0, "stopLoss": 3100.0
    }))
    .unwrap();
    assert_eq!((r.direction, r.notional_usdt), (Direction::Short, None));
    let body = serde_json::to_value(preview_of(&open_at(&cfg(), &req(Direction::Long, 110.0, 95.0), 100.0), 100.0)).unwrap();
    for key in ["riskPctOfCapital", "liquidationPrice", "rewardRisk", "feesUsdt", "lossAtStopUsdt", "qty"] {
        assert!(body.get(key).is_some(), "{key}");
    }
}
