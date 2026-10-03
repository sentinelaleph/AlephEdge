//! Entry-side tests: geometry acceptance at live TP1≈0.6R, the fill model,
//! the direction-aware BTC-break guard, and final-vs-transient skips.

use super::btc_break::{entry_block, BtcRegime};
use super::entry::{pre_price_gate, price_gate, settle_skip, GateInput};
use super::geometry::{fill_acceptable, fill_decision, geometry_coherent, FillDecision};
use super::precheck::is_final;
use super::test_fixtures::{cfg, signal, signal_at};
use crate::bot::judged::JudgeLedger;
use crate::bot::model::{BotConfig, BotKind};
use crate::signal::model::{Direction, Signal};
use crate::signal::time::parse_rfc3339_ms;

fn gate_input<'a>(cfg: &'a BotConfig, sig: &'a Signal, regime: BtcRegime) -> GateInput<'a> {
    GateInput {
        cfg,
        sig,
        regime,
        now_ms: parse_rfc3339_ms("2026-09-16T10:01:00Z").unwrap(),
        invalidated: false,
        regime_vetoed: false,
        pump_allowed: true,
        futures_supported: true,
        holds_market: false,
    }
}

// Live geometry: 2.5×ATR stop, TP1 ≈0.6R. The old `rr < 1.0` floor refused
// nearly every published signal; both directions must now pass.
#[test]
fn tp1_at_0_6r_is_accepted_long_and_short() {
    let long = signal("long", 100.0, 106.0, 90.0);
    let short = signal("short", 100.0, 94.0, 110.0);
    assert!(geometry_coherent(&long) && geometry_coherent(&short));
    let c = cfg();
    assert!(pre_price_gate(&gate_input(&c, &long, BtcRegime::Normal)).is_none());
    assert!(pre_price_gate(&gate_input(&c, &short, BtcRegime::Normal)).is_none());
}

#[test]
fn incoherent_geometry_is_refused_finally() {
    let c = cfg();
    let inverted = signal("long", 100.0, 95.0, 90.0);
    let mut no_tp = signal("short", 100.0, 94.0, 110.0);
    no_tp.tp.clear();
    for sig in [&inverted, &no_tp] {
        let skip = pre_price_gate(&gate_input(&c, sig, BtcRegime::Normal)).expect("refused");
        assert_eq!(skip.key, "incoherentGeometry");
        assert!(is_final(skip.key));
    }
}

#[test]
fn fill_drift_limits_and_crossed_levels() {
    let long = signal("long", 100.0, 106.0, 90.0);
    assert!(fill_acceptable(&long, 100.4));
    assert!(!fill_acceptable(&long, 100.6));
    assert!(!fill_acceptable(&long, 106.5), "crossed TP");
    let short = signal("short", 100.0, 94.0, 110.0);
    assert!(fill_acceptable(&short, 99.6));
    assert!(!fill_acceptable(&short, 99.4));
    assert!(!fill_acceptable(&short, 93.0), "crossed TP");
}

#[test]
fn marketable_only_within_five_minutes_else_wait_for_touch() {
    let sig = signal_at("long", 100.0, 106.0, 90.0, "2026-09-16T10:00:00Z");
    let at = |t: &str| parse_rfc3339_ms(t).unwrap();
    assert_eq!(
        fill_decision(&sig, 100.3, at("2026-09-16T10:04:00Z")),
        FillDecision::Fill
    );
    assert_eq!(
        fill_decision(&sig, 100.3, at("2026-09-16T10:06:00Z")),
        FillDecision::Wait
    );
    assert_eq!(
        fill_decision(&sig, 99.9, at("2026-09-16T10:06:00Z")),
        FillDecision::Fill
    );
    // Touched but already through the stop: unusable, final.
    assert_eq!(
        fill_decision(&sig, 89.0, at("2026-09-16T10:06:00Z")),
        FillDecision::Refuse
    );
}

#[test]
fn break_regime_blocks_long_allows_short() {
    assert_eq!(
        entry_block(BtcRegime::Break, Direction::Long),
        Some("btcBreakGuard")
    );
    assert_eq!(entry_block(BtcRegime::Break, Direction::Short), None);
    assert_eq!(
        entry_block(BtcRegime::Unknown, Direction::Short),
        Some("btcGuardUnknown")
    );
    let c = cfg();
    let short = signal("short", 100.0, 94.0, 110.0);
    assert!(pre_price_gate(&gate_input(&c, &short, BtcRegime::Break)).is_none());
    let long = signal("long", 100.0, 106.0, 90.0);
    let skip = pre_price_gate(&gate_input(&c, &long, BtcRegime::Break)).unwrap();
    assert_eq!(skip.key, "btcBreakGuard");
}

// A momentary condition must not refuse a signal forever: it is noted once,
// left unjudged, and the signal opens once the condition clears.
#[test]
fn transient_skip_then_retry_opens_later() {
    let c = cfg();
    let sig = signal("long", 100.0, 106.0, 90.0);
    let mut ledger = JudgeLedger::default();

    let skip = pre_price_gate(&gate_input(&c, &sig, BtcRegime::Unknown)).unwrap();
    assert!(!is_final(skip.key));
    assert!(
        settle_skip(&mut ledger, BotKind::Futures, &sig.id, &skip),
        "noted once"
    );
    assert!(
        !settle_skip(&mut ledger, BotKind::Futures, &sig.id, &skip),
        "not spammed"
    );
    let price_skip = price_gate(&sig, None, 0).unwrap_err();
    assert_eq!(price_skip.key, "priceUnavailable");
    assert!(settle_skip(
        &mut ledger,
        BotKind::Futures,
        &sig.id,
        &price_skip
    ));
    assert!(!ledger.is_judged(BotKind::Futures, &sig.id));

    // Next tick: feed back, price touches entry → opens.
    let input = gate_input(&c, &sig, BtcRegime::Normal);
    assert!(pre_price_gate(&input).is_none());
    assert!(matches!(
        price_gate(&sig, Some(99.95), input.now_ms),
        Ok(true)
    ));
}

#[test]
fn static_skip_stays_judged() {
    let mut c = cfg();
    c.direction = Some("short".into());
    let sig = signal("long", 100.0, 106.0, 90.0);
    let mut ledger = JudgeLedger::default();
    let skip = pre_price_gate(&gate_input(&c, &sig, BtcRegime::Normal)).unwrap();
    assert_eq!(skip.key, "directionFiltered");
    assert!(settle_skip(&mut ledger, BotKind::Futures, &sig.id, &skip));
    assert!(ledger.is_judged(BotKind::Futures, &sig.id));
    // Another bot kind is judged independently.
    assert!(!ledger.is_judged(BotKind::Spot, &sig.id));
}

#[test]
fn held_market_and_caps_are_transient() {
    let c = cfg();
    let sig = signal("short", 100.0, 94.0, 110.0);
    let mut input = gate_input(&c, &sig, BtcRegime::Normal);
    input.holds_market = true;
    assert_eq!(pre_price_gate(&input).unwrap().key, "marketHeld");
    for key in [
        "marketHeld",
        "maxPositions",
        "botMaxPositions",
        "capitalCap",
        "frUnavailable",
    ] {
        assert!(!is_final(key), "{key}");
    }
    for key in [
        "expired",
        "invalidated",
        "vetoedByRegime",
        "fillCrossedLevel",
        "comboFiltered",
    ] {
        assert!(is_final(key), "{key}");
    }
}

// A signal Sentinel vetoed (2026-09-18 — BTC's regime turned against it) must
// never be entered, same as legacy invalidation, but with its own key so the
// desk shows why. `invalidated` alone must not trip it (the two flags are
// independent — see `signal::veto_tests`).
#[test]
fn regime_veto_refuses_entry_finally() {
    let c = cfg();
    let sig = signal("long", 100.0, 106.0, 90.0);
    let mut input = gate_input(&c, &sig, BtcRegime::Normal);
    assert!(pre_price_gate(&input).is_none(), "not vetoed: passes");
    input.regime_vetoed = true;
    let skip = pre_price_gate(&input).expect("refused");
    assert_eq!(skip.key, "vetoedByRegime");
    assert!(is_final(skip.key));
}
