//! Paper/live parity on liquidation: the paper book refuses the entries the
//! live path refuses (stop beyond the isolated liquidation price), and no
//! paper close books a loss beyond the isolated margin.

use super::entry_rules::plan_entry;
use super::management::{close_step, EXIT_LIQUIDATION};
use super::precheck::is_final;
use super::record::trade_record;
use super::sizing::{
    apply_size, isolated_margin_pct, liquidation_price, size_position,
    SKIP_STOP_BEYOND_LIQUIDATION,
};
use super::entry::new_position;
use super::take_profit::TpResolution;
use super::test_fixtures::{cfg, signal};
use crate::bot::model::{BotConfig, BotKind, OpenPosition, SizingMode};
use crate::signal::time::parse_rfc3339_ms;

fn now() -> u64 {
    parse_rfc3339_ms("2026-09-16T10:01:00Z").unwrap()
}

fn fixed(leverage: u8) -> BotConfig {
    let mut c = cfg();
    c.leverage = leverage;
    c
}

/// A paper position sized exactly as the engine sizes it.
fn sized_position(c: &BotConfig, dir: &str, sl: f64, leverage: u8) -> OpenPosition {
    let tp = if dir == "long" { 110.0 } else { 90.0 };
    let sig = signal(dir, 100.0, tp, sl);
    let size = size_position(c, leverage, 100.0, sl).unwrap();
    let mut pos = new_position(c, &sig, 100.0, leverage, 0, &TpResolution::signal_tp1(&sig));
    apply_size(&mut pos, &size);
    pos
}

// The reported case: fixed sizing at 20x with a 7% stop. Live refuses it
// (liquidation ~2.5% away); the paper book used to record it and then book
// −140% of capital at the stop.
#[test]
fn paper_refuses_the_entry_live_refuses() {
    for (dir, sl) in [("long", 93.0), ("short", 107.0)] {
        let tp = if dir == "long" { 110.0 } else { 90.0 };
        let sig = signal(dir, 100.0, tp, sl);
        let skip = plan_entry(&fixed(20), &sig, Some(100.0), now(), 20).unwrap_err();
        assert_eq!(skip.key, SKIP_STOP_BEYOND_LIQUIDATION, "{dir}");
        assert!(is_final(skip.key), "judged once, not retried every tick");
    }
}

#[test]
fn entries_with_the_stop_inside_liquidation_still_open() {
    let sig = signal("long", 100.0, 110.0, 93.0);
    // 7% stop at 5x: liquidation ~17.5% away.
    assert!(plan_entry(&fixed(5), &sig, Some(100.0), now(), 5).unwrap().is_some());
    // Risk sizing at a 20x cap: 1% risk on a 7% stop is ~0.14x notional, so
    // the exchange leverage is 1x and liquidation is nowhere near.
    let mut risk = fixed(20);
    risk.sizing = SizingMode::Risk;
    assert!(plan_entry(&risk, &sig, Some(100.0), now(), 20).unwrap().is_some());
    // Spot is unlevered: never liquidation-gated.
    let mut spot = fixed(20);
    spot.kind = BotKind::Spot;
    assert!(plan_entry(&spot, &sig, Some(100.0), now(), 1).unwrap().is_some());
}

// Every gated entry has its stop strictly before the exact liquidation price,
// so for it the stop always fires first.
#[test]
fn a_gated_stop_always_sits_before_liquidation() {
    for lev in 1..=30u8 {
        for stop_pct in [0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 12.0, 20.0, 40.0] {
            for dir in ["long", "short"] {
                let sl = if dir == "long" { 100.0 - stop_pct } else { 100.0 + stop_pct };
                let tp = if dir == "long" { 100.0 + 2.0 * stop_pct } else { 100.0 - stop_pct / 2.0 };
                let sig = signal(dir, 100.0, tp, sl);
                let Ok(Some(_)) = plan_entry(&fixed(lev), &sig, Some(100.0), now(), lev) else {
                    continue;
                };
                let pos = sized_position(&fixed(lev), dir, sl, lev);
                if let Some(liq) = liquidation_price(&pos) {
                    let inside = if dir == "long" { sl > liq } else { sl < liq };
                    assert!(inside, "{dir} {lev}x stop {stop_pct}%: sl {sl} liq {liq}");
                }
            }
        }
    }
}

// A position already in the book from before the gate (or restored from
// disk) that price carries past its liquidation: the paper book liquidates
// it, at the liquidation price, for exactly the isolated margin.
#[test]
fn a_legacy_position_past_liquidation_is_liquidated_for_its_margin() {
    let c = fixed(20);
    for (dir, sl, gap) in [("long", 93.0, 96.0), ("short", 107.0, 104.0)] {
        let mut pos = sized_position(&c, dir, sl, 20);
        let liq = liquidation_price(&pos).unwrap();
        assert!((liq - 100.0).abs() < 3.0, "{dir}: liq {liq} ~2.5% away");
        let step = close_step(&mut pos, gap, 1, false, None);
        let exit = step.exit.expect("liquidated");
        assert_eq!(exit.reason, EXIT_LIQUIDATION, "{dir}");
        assert!((exit.price - liq).abs() < 1e-9, "{dir}: exits at the liquidation price");
        let rec = trade_record(&pos, exit.price, exit.reason, 2);
        assert!((rec.pnl_pct + 100.0).abs() < 1e-9, "{dir}: {}", rec.pnl_pct);
        assert!((rec.pnl_usdt + 100.0).abs() < 1e-9, "{dir}: the whole 100 USDT margin");
    }
}

// The headline defect: no paper close may record a loss beyond the margin,
// whatever the exit reason (a stop gapped through, a daily stop, a veto).
#[test]
fn no_paper_close_loses_more_than_the_isolated_margin() {
    let c = fixed(20);
    let pos = sized_position(&c, "long", 93.0, 20);
    assert_eq!(isolated_margin_pct(&pos), Some(100.0));
    for reason in ["sl", "dailyStop", "veto", "horizon"] {
        let rec = trade_record(&pos, 93.0, reason, 2);
        assert!(rec.pnl_pct >= -100.0 - 1e-9, "{reason}: {}", rec.pnl_pct);
        assert!(rec.pnl_usdt >= -100.0 - 1e-9, "{reason}: {}", rec.pnl_usdt);
    }
    // Risk sizing: margin = notional at 1x, so the floor is that notional.
    let mut risk = fixed(20);
    risk.sizing = SizingMode::Risk;
    let small = sized_position(&risk, "long", 93.0, 20);
    let margin = isolated_margin_pct(&small).unwrap();
    assert!((margin - small.notional_usdt).abs() < 1e-9, "1x: margin = notional");
    // A loss inside the margin is untouched: the stop still costs ~1%.
    let at_stop = trade_record(&small, 93.0, "sl", 2).pnl_pct;
    assert!((at_stop + 1.0).abs() < 0.02, "{at_stop}");
}

// Within the margin the stop still wins: a gated position gapping through
// both its stop and its liquidation price records a stop, capped.
#[test]
fn a_gated_position_gapping_through_both_levels_is_a_capped_stop() {
    let c = fixed(5);
    let mut pos = sized_position(&c, "long", 93.0, 5);
    let liq = liquidation_price(&pos).unwrap();
    let step = close_step(&mut pos, liq - 5.0, 1, false, None);
    assert_eq!(step.exit.unwrap().reason, "sl");
    let rec = trade_record(&pos, liq - 5.0, "sl", 2);
    assert!((rec.pnl_pct + 100.0).abs() < 1e-9, "capped at the margin: {}", rec.pnl_pct);
}

// Live positions are liquidated (or stopped) by the exchange itself; the
// book never invents a liquidation exit for one.
#[test]
fn live_positions_get_no_simulated_liquidation() {
    let mut pos = sized_position(&fixed(20), "long", 93.0, 20);
    pos.live = true;
    let liq = liquidation_price(&pos).unwrap();
    let step = close_step(&mut pos, liq - 0.5, 1, false, None);
    assert!(step.exit.is_none(), "{:?}", step.exit);
}

#[test]
fn spot_and_one_x_longs_have_no_liquidation() {
    let mut spot = fixed(1);
    spot.kind = BotKind::Spot;
    let pos = sized_position(&spot, "long", 93.0, 1);
    assert_eq!(liquidation_price(&pos), None);
    assert_eq!(isolated_margin_pct(&pos), None);
    let one_x = sized_position(&fixed(1), "long", 93.0, 1);
    assert_eq!(liquidation_price(&one_x), None, "a fully funded long");
    let short = sized_position(&fixed(1), "short", 107.0, 1);
    assert!(liquidation_price(&short).unwrap() > 190.0, "1x short liquidates near 2x");
}

// 9 Oct 2026: a 100 USDT balance at Cautious caps a position at 2 USDT. Binance
// refuses orders under ~5 USDT, so the app's paper book refuses them too
// instead of filling trades the exchange never would. plan_entry itself keeps
// planning them: the VDS paper runner shares it and its arms must not shift.
#[test]
fn entries_below_the_exchange_minimum_are_refused_on_paper_too() {
    use super::entry_rules::exchange_minimum_gate;
    let sig = signal("long", 100.0, 110.0, 93.0);
    let mut small = fixed(1);
    small.capital = 2.0;
    let plan = plan_entry(&small, &sig, Some(100.0), now(), 1).unwrap().expect("runner path unchanged");
    let skip = exchange_minimum_gate(&plan).unwrap_err();
    assert_eq!(skip.key, "belowExchangeMinimum");
    assert_eq!(skip.detail.as_deref(), Some("2.00"));

    let mut enough = fixed(1);
    enough.capital = crate::bot::model::EXCHANGE_MIN_ORDER_USDT;
    let plan = plan_entry(&enough, &sig, Some(100.0), now(), 1).unwrap().unwrap();
    assert!(exchange_minimum_gate(&plan).is_ok());
}
