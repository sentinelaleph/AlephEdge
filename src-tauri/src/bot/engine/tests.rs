//! Close-decision tests: the management plan (long and short), horizon,
//! invalidation, the user's max-loss cap, and the ledger-parity cost field.

use super::book::{trade_record, veto_trade_record};
use super::management::close_step;
use super::pnl::{breaches_cap, unlevered_net_pct, unrealized_net_pnl_pct};
use super::test_fixtures::{plan, position, signal};
use crate::signal::model::RegimeVetoEvent;

const NOW: u64 = 1_000;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

// Server invalidation used to close first ("the emergency brake"). The
// server's BTC guard closing filled positions was measured at −163 pts and is
// OFF upstream, so a mid-range invalidated position now stays open and only
// raises a note.
#[test]
fn invalidation_closes_immediately() {
    let mut p = position(&signal("long", 100.0, 110.0, 95.0), None);
    let step = close_step(&mut p, 102.0, NOW, true, None);
    assert_eq!(step.exit, None, "invalidation must no longer close");
    assert!(step.note_invalidation);
}

// Invalidation no longer outranks the ledger exits: at SL it is "sl", at TP
// it is "tp" — never "invalidated" (−163 pts measurement, see above).
#[test]
fn invalidation_beats_tp_and_sl() {
    let mut p = position(&signal("long", 100.0, 110.0, 95.0), None);
    assert_eq!(
        close_step(&mut p, 94.0, NOW, true, None)
            .exit
            .unwrap()
            .reason,
        "sl"
    );
    let mut p = position(&signal("long", 100.0, 110.0, 95.0), None);
    assert_eq!(
        close_step(&mut p, 111.0, NOW, true, None)
            .exit
            .unwrap()
            .reason,
        "tp"
    );
}

#[test]
fn long_tp_sl_without_plan() {
    let sig = signal("long", 100.0, 110.0, 95.0);
    assert_eq!(
        close_step(&mut position(&sig, None), 110.0, NOW, false, None)
            .exit
            .unwrap()
            .reason,
        "tp"
    );
    assert_eq!(
        close_step(&mut position(&sig, None), 95.0, NOW, false, None)
            .exit
            .unwrap()
            .reason,
        "sl"
    );
    assert_eq!(
        close_step(&mut position(&sig, None), 102.0, NOW, false, None).exit,
        None
    );
}

#[test]
fn short_tp_sl_without_plan() {
    let sig = signal("short", 100.0, 90.0, 105.0);
    assert_eq!(
        close_step(&mut position(&sig, None), 90.0, NOW, false, None)
            .exit
            .unwrap()
            .reason,
        "tp"
    );
    assert_eq!(
        close_step(&mut position(&sig, None), 105.0, NOW, false, None)
            .exit
            .unwrap()
            .reason,
        "sl"
    );
    assert_eq!(
        close_step(&mut position(&sig, None), 98.0, NOW, false, None).exit,
        None
    );
}

/// Runs (direction, prices) through one position; returns reason + pnl.
fn run(dir: &str, tp_r: f64, prices: &[f64]) -> (Option<&'static str>, f64, bool) {
    // R = 10; long levels above 100, short levels mirrored below.
    let s = if dir == "long" { 1.0 } else { -1.0 };
    let sig = signal(dir, 100.0, 100.0 + s * tp_r * 10.0, 100.0 - s * 10.0);
    let mut p = position(&sig, Some(plan(0.5, 1.0, 0.5)));
    for &px in prices {
        let px = 100.0 + s * (px - 100.0);
        if let Some(exit) = close_step(&mut p, px, NOW, false, None).exit {
            return (
                Some(exit.reason),
                unrealized_net_pnl_pct(&p, exit.price),
                p.partial_price.is_some(),
            );
        }
    }
    (None, 0.0, p.partial_price.is_some())
}

const FEES_1X: f64 = 2.0 * crate::bot::model::FUTURES_FEE_RATE * 100.0; // round-trip taker at 1x, in %

// (a) TP1 0.72R is nearer than the 1R partial, so the partial can never fire:
// a tick through both exits the WHOLE position at TP (Sentinel's fixed bug).
#[test]
fn plan_a_tp_nearer_than_partial_exits_whole_at_tp() {
    for dir in ["long", "short"] {
        let (reason, pnl, partial) = run(dir, 0.72, &[103.0, 111.0]);
        assert_eq!(reason, Some("tp"), "{dir}");
        assert!(!partial, "{dir}: no phantom partial");
        assert!(close(pnl, 11.0 - FEES_1X), "{dir}: {pnl}");
    }
}

// (b) TP1 2R: partial banks half at 1R, the rest exits at TP → blended.
#[test]
fn plan_b_partial_then_tp_blends() {
    for dir in ["long", "short"] {
        let (reason, pnl, partial) = run(dir, 2.0, &[110.0, 120.0]);
        assert_eq!(reason, Some("tp"), "{dir}");
        assert!(partial, "{dir}");
        assert!(
            close(pnl, 0.5 * 10.0 + 0.5 * 20.0 - FEES_1X),
            "{dir}: {pnl}"
        );
    }
}

// (c) Breakeven arms at 0.5R but only protects from the NEXT evaluation.
#[test]
fn plan_c_breakeven_protects_from_next_evaluation() {
    for dir in ["long", "short"] {
        let s = if dir == "long" { 1.0 } else { -1.0 };
        let sig = signal(dir, 100.0, 100.0 + s * 20.0, 100.0 - s * 10.0);
        let mut p = position(&sig, Some(plan(0.5, 1.0, 0.5)));
        // Before arming, a return to entry is not an exit.
        assert_eq!(close_step(&mut p, 100.0, NOW, false, None).exit, None);
        let arming = close_step(&mut p, 100.0 + s * 5.0, NOW, false, None);
        assert_eq!(arming.exit, None, "{dir}: arming evaluation never exits");
        assert!(arming.changed && p.breakeven_armed, "{dir}");
        let next = close_step(&mut p, 100.0, NOW, false, None)
            .exit
            .expect("breakeven");
        assert_eq!(next.reason, "breakeven", "{dir}");
        assert!(
            close(unrealized_net_pnl_pct(&p, next.price), -FEES_1X),
            "{dir}: scratch"
        );
    }
}

// (d) Stop before anything: full loss, no partial.
#[test]
fn plan_d_stop_first_is_full_loss() {
    for dir in ["long", "short"] {
        let (reason, pnl, partial) = run(dir, 2.0, &[97.0, 90.0]);
        assert_eq!(reason, Some("sl"), "{dir}");
        assert!(!partial);
        assert!(close(pnl, -10.0 - FEES_1X), "{dir}: {pnl}");
    }
}

// (e) Partial at 1R (which also arms breakeven), then back to entry: a
// breakeven exit that is a small win thanks to the banked half.
#[test]
fn plan_e_partial_then_breakeven_is_small_win() {
    for dir in ["long", "short"] {
        let (reason, pnl, partial) = run(dir, 2.0, &[110.0, 100.0]);
        assert_eq!(reason, Some("breakeven"), "{dir}");
        assert!(partial);
        assert!(close(pnl, 5.0 - FEES_1X) && pnl > 0.0, "{dir}: {pnl}");
    }
}

#[test]
fn horizon_closes_untouched_position_at_live_price() {
    let mut p = position(&signal("short", 100.0, 90.0, 105.0), None);
    p.horizon_ms = 5_000;
    assert_eq!(close_step(&mut p, 99.0, 4_999, false, None).exit, None);
    let exit = close_step(&mut p, 99.0, 5_000, false, None).exit.unwrap();
    assert_eq!((exit.reason, exit.price), ("horizon", 99.0));
}

// The user's cap takes no regime input at all: it fires on the loss alone.
#[test]
fn max_loss_closes_regardless_of_btc_regime() {
    let mut p = position(&signal("long", 100.0, 110.0, 90.0), None);
    p.leverage = 5;
    assert_eq!(close_step(&mut p, 99.0, NOW, false, None).exit, None);
    let exit = close_step(&mut p, 99.0, NOW, false, Some(2.0))
        .exit
        .unwrap();
    assert_eq!(exit.reason, "max_loss");
}

#[test]
fn cap_uses_leveraged_net_pnl_and_ignores_profit_or_off() {
    let mut p = position(&signal("long", 100.0, 110.0, 90.0), None);
    assert!(breaches_cap(&p, 98.5, 1.5)); // −1.5% − round-trip fees
    assert!(!breaches_cap(&p, 98.5, 2.0));
    assert!(!breaches_cap(&p, 103.0, 2.0));
    assert!(!breaches_cap(&p, 90.0, 0.0));
    p.leverage = 5;
    assert!(breaches_cap(&p, 98.5, 2.0));
}

#[test]
fn unlevered_net_pct_matches_ledger_costs() {
    let long = position(&signal("long", 100.0, 110.0, 90.0), None);
    assert!(close(unlevered_net_pct(&long, 102.0), 2.0 - 0.10 - 0.02));
    let short = position(&signal("short", 100.0, 90.0, 110.0), None);
    assert!(close(unlevered_net_pct(&short, 98.0), 2.0 - 0.10 + 0.02));
    let rec = trade_record(&short, 98.0, "tp", 7);
    assert!(close(rec.unlevered_net_pct.unwrap(), 1.92));
}

fn veto() -> RegimeVetoEvent {
    RegimeVetoEvent {
        signal_id: "sig-1".into(),
        symbol: "SOLUSDT".into(),
        direction: None,
        reason_code: "btc_regime_turn_bullish".into(),
        reason_text: "BTC turned bullish for 60 min (strength 0.21)".into(),
        trigger: "regime_turn".into(),
        btc_price: 77_541.9,
        btc_trend: "bullish".into(),
        btc_strength: 0.21,
        btc_change_1h_pct: 1.4,
        vetoed_at: "2026-09-18T11:02:03Z".into(),
    }
}

// A veto close is a normal exit (same PnL formula, same fields) stamped with
// Sentinel's reason — never a different code path that could disagree with
// the ledger's own numbers.
#[test]
fn veto_trade_record_matches_an_ordinary_close_and_carries_the_reason() {
    let short = position(&signal("short", 100.0, 90.0, 110.0), None);
    let ordinary = trade_record(&short, 98.0, "tp", 7);
    let vetoed = veto_trade_record(&short, 98.0, &veto(), 7);

    assert_eq!(vetoed.exit_reason, "veto");
    assert_eq!(
        vetoed.pnl_pct, ordinary.pnl_pct,
        "same PnL formula as any exit"
    );
    assert_eq!(vetoed.pnl_quote, ordinary.pnl_quote);
    assert_eq!(vetoed.unlevered_net_pct, ordinary.unlevered_net_pct);
    assert_eq!(
        vetoed.veto_reason_code.as_deref(),
        Some("btc_regime_turn_bullish")
    );
    assert_eq!(
        vetoed.veto_reason_text.as_deref(),
        Some("BTC turned bullish for 60 min (strength 0.21)")
    );
    // An ordinary close never carries a veto reason.
    assert!(ordinary.veto_reason_code.is_none() && ordinary.veto_reason_text.is_none());
}

// close_phase saved a paper position's changed state (breakeven armed,
// partial banked) WITHOUT the close claim. A veto close running at the same
// moment could delete the stored row between that save's in-memory check and
// its write, and the write put the closed position back on disk: restored on
// the next start, closed and recorded a second time.
#[test]
fn a_state_save_takes_the_close_claim_like_a_close() {
    let sig = signal("long", 100.0, 110.0, 95.0);
    let mut p = position(&sig, Some(plan(0.5, 2.0, 0.5)));
    let quiet = close_step(&mut p, 101.0, NOW, false, None);
    assert!(!quiet.changed && quiet.exit.is_none());
    assert!(!quiet.needs_claim(false), "nothing to write: no claim");
    let armed = close_step(&mut p, 103.0, NOW, false, None);
    assert!(armed.changed && armed.exit.is_none(), "breakeven armed");
    assert!(armed.needs_claim(false), "a save must hold the claim");
    assert!(close_step(&mut p, 111.0, NOW, false, None).needs_claim(false));
    assert!(quiet.needs_claim(true), "a live position always does");
}

// The daily stop closed open positions only on the tick it tripped; every
// later tick returned early, so a position that close could not take (no
// price that tick, a failed live close) kept trading past the stop.
#[test]
fn a_tripped_daily_stop_keeps_closing_what_it_could_not_close() {
    use super::retry_daily_stop_close;
    assert!(retry_daily_stop_close(true, true, 1), "left over: close again");
    assert!(!retry_daily_stop_close(true, false, 1), "user keeps positions");
    assert!(!retry_daily_stop_close(true, true, 0), "nothing left");
    assert!(!retry_daily_stop_close(false, true, 1), "not tripped");
}

// A real position must be flattenable when the public quote is missing: the
// live close sizes from the exchange, so the entry is a safe fallback. A
// paper close keeps needing a real quote (it books at that price).
#[test]
fn a_missing_quote_never_keeps_a_live_position_open() {
    use crate::bot::engine::close_reference;
    assert_eq!(close_reference(Some(101.0), true, 100.0), Some(101.0));
    assert_eq!(close_reference(None, true, 100.0), Some(100.0));
    assert_eq!(close_reference(None, false, 100.0), None);
    assert_eq!(close_reference(None, true, 0.0), None);
    assert_eq!(close_reference(None, true, f64::NAN), None);
}
