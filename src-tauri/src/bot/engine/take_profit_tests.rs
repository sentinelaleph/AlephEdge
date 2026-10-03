//! Take-profit target tests: resolution at entry (TP1/TP2/TP3/custom,
//! fallback, refusals), config precedence and migration, TP1 byte-identity
//! with the pre-2026-09-22 book, and management against a farther target.

use super::book::trade_record;
use super::entry::new_position;
use super::management::close_step;
use super::pnl::unrealized_net_pnl_pct;
use super::precheck::is_final;
use super::take_profit::{
    resolve_tp, TpResolution, SKIP_TP_CUSTOM_INVALID, SKIP_TP_NOT_BEYOND_FILL,
};
use super::test_fixtures::{cfg, plan, signal};
use crate::bot::model::{BotConfig, OpenPosition, TakeProfitTarget};
use crate::signal::model::{ManagementPlan, Signal};

use TakeProfitTarget::{Custom, Tp1, Tp2, Tp3};

const NOW: u64 = 1_000;
const FEES_1X: f64 = 2.0 * crate::bot::model::FUTURES_FEE_RATE * 100.0; // round-trip taker at 1x, in %

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// A signal carrying `tps` as its targets (R = 10: sl 10 away from 100).
fn signal_tps(dir: &str, tps: &[f64]) -> Signal {
    let sl = if dir == "long" { 90.0 } else { 110.0 };
    let mut sig = signal(dir, 100.0, tps[0], sl);
    sig.tp = tps.to_vec();
    sig
}

fn long3() -> Signal {
    signal_tps("long", &[104.0, 107.0, 117.0])
}

fn short3() -> Signal {
    signal_tps("short", &[96.0, 93.0, 83.0])
}

/// A position opened at `fill` with `target`, as the entry path builds it.
fn position_with(
    sig: &Signal,
    target: TakeProfitTarget,
    fill: f64,
    plan: Option<ManagementPlan>,
) -> OpenPosition {
    let mut sig = sig.clone();
    sig.management_plan = plan;
    let res = resolve_tp(&sig, target, fill).expect("resolves");
    new_position(&cfg(), &sig, fill, 1, 0, &res)
}

// ---- resolution -------------------------------------------------------

#[test]
fn tp1_resolves_to_the_signal_tp1() {
    for sig in [long3(), short3()] {
        let r = resolve_tp(&sig, Tp1, 100.0).unwrap();
        assert_eq!(r, TpResolution::signal_tp1(&sig));
        assert_eq!((r.target.as_str(), r.fallback_from), ("tp1", None));
    }
}

#[test]
fn tp2_and_tp3_pick_their_own_levels() {
    let r = resolve_tp(&long3(), Tp3, 100.0).unwrap();
    assert_eq!(
        (r.tp, r.target.as_str(), r.fallback_from),
        (117.0, "tp3", None)
    );
    let r = resolve_tp(&short3(), Tp3, 100.0).unwrap();
    assert_eq!((r.tp, r.target.as_str()), (83.0, "tp3"));
    let r = resolve_tp(&long3(), Tp2, 100.0).unwrap();
    assert_eq!((r.tp, r.target.as_str()), (107.0, "tp2"));
    let r = resolve_tp(&short3(), Tp2, 100.0).unwrap();
    assert_eq!((r.tp, r.target.as_str()), (93.0, "tp2"));
}

#[test]
fn custom_is_a_percent_from_the_fill_in_the_trade_direction() {
    let r = resolve_tp(&long3(), Custom { pct: 40.0 }, 100.4).unwrap();
    assert!(close(r.tp, 100.4 * 1.4), "{}", r.tp);
    assert_eq!((r.target.as_str(), r.fallback_from), ("custom:40", None));

    let r = resolve_tp(&short3(), Custom { pct: 40.0 }, 99.6).unwrap();
    assert!(close(r.tp, 99.6 * 0.6), "{}", r.tp);
    assert_eq!(r.target, "custom:40");

    let r = resolve_tp(&long3(), Custom { pct: 12.5 }, 100.0).unwrap();
    assert_eq!(r.target, "custom:12.5");
}

#[test]
fn missing_target_falls_back_to_the_highest_the_signal_has() {
    let one = signal_tps("long", &[104.0]);
    let r = resolve_tp(&one, Tp3, 100.0).unwrap();
    assert_eq!(
        (r.tp, r.target.as_str(), r.fallback_from.as_deref()),
        (104.0, "tp1", Some("tp3"))
    );
    let r = resolve_tp(&one, Tp2, 100.0).unwrap();
    assert_eq!(
        (r.target.as_str(), r.fallback_from.as_deref()),
        ("tp1", Some("tp2"))
    );

    let two = signal_tps("short", &[96.0, 93.0]);
    let r = resolve_tp(&two, Tp3, 100.0).unwrap();
    assert_eq!(
        (r.tp, r.target.as_str(), r.fallback_from.as_deref()),
        (93.0, "tp2", Some("tp3"))
    );

    // An unusable (0 / NaN) TP3 counts as missing.
    let zero = signal_tps("long", &[104.0, 107.0, 0.0]);
    let r = resolve_tp(&zero, Tp3, 100.0).unwrap();
    assert_eq!(
        (r.target.as_str(), r.fallback_from.as_deref()),
        ("tp2", Some("tp3"))
    );

    // The fallback is recorded on the position for the UI.
    let p = position_with(&one, Tp3, 100.0, None);
    assert_eq!(
        (p.tp, p.tp_target.as_str(), p.tp_fallback_from.as_deref()),
        (104.0, "tp1", Some("tp3"))
    );
}

#[test]
fn invalid_custom_pct_is_refused_finally() {
    for pct in [0.0, 0.05, 1000.5, -5.0, f64::NAN, f64::INFINITY] {
        let skip = resolve_tp(&long3(), Custom { pct }, 100.0).unwrap_err();
        assert_eq!(skip.key, SKIP_TP_CUSTOM_INVALID, "{pct}");
        assert!(is_final(skip.key));
    }
    // Bounds are inclusive.
    assert!(resolve_tp(&long3(), Custom { pct: 0.1 }, 100.0).is_ok());
    assert!(resolve_tp(&long3(), Custom { pct: 1000.0 }, 100.0).is_ok());
    // A short cannot target a price at or below zero.
    for pct in [100.0, 250.0, 1000.0] {
        let skip = resolve_tp(&short3(), Custom { pct }, 100.0).unwrap_err();
        assert_eq!(skip.key, SKIP_TP_CUSTOM_INVALID, "{pct}");
    }
    assert!(resolve_tp(&short3(), Custom { pct: 99.0 }, 100.0).is_ok());
}

#[test]
fn a_target_not_beyond_the_fill_is_refused() {
    // Malformed ladder: TP2 sits below the fill of a long.
    let bad = signal_tps("long", &[104.0, 99.0, 117.0]);
    let skip = resolve_tp(&bad, Tp2, 100.0).unwrap_err();
    assert_eq!(
        (skip.key, skip.detail.as_deref()),
        (SKIP_TP_NOT_BEYOND_FILL, Some("TP2"))
    );
    assert!(is_final(skip.key));
    // Short mirror.
    let bad = signal_tps("short", &[96.0, 101.0]);
    assert_eq!(
        resolve_tp(&bad, Tp2, 100.0).unwrap_err().key,
        SKIP_TP_NOT_BEYOND_FILL
    );
    // No targets at all is the geometry refusal it always was.
    let mut none = long3();
    none.tp.clear();
    assert_eq!(
        resolve_tp(&none, Tp3, 100.0).unwrap_err().key,
        "incoherentGeometry"
    );
}

// ---- config ---------------------------------------------------------------

#[test]
fn per_symbol_override_wins_over_the_bot_default() {
    let mut c = cfg();
    c.take_profit = Tp2;
    c.take_profit_overrides
        .insert("SOLUSDT".into(), Custom { pct: 40.0 });
    assert_eq!(c.take_profit_for("SOLUSDT"), Custom { pct: 40.0 });
    assert_eq!(c.take_profit_for("solusdt"), Custom { pct: 40.0 });
    assert_eq!(c.take_profit_for("ETHUSDT"), Tp2);
}

#[test]
fn validate_refuses_out_of_bounds_custom_targets() {
    let mut c = cfg();
    assert!(c.validate().is_ok());
    c.take_profit = Custom { pct: 0.0 };
    assert!(c.validate().is_err());
    c.take_profit = Custom { pct: 40.0 };
    assert!(c.validate().is_ok());
    c.take_profit_overrides
        .insert("SOLUSDT".into(), Custom { pct: 5000.0 });
    assert!(c.validate().unwrap_err().contains("SOLUSDT"));
    c.take_profit_overrides.clear();
    c.take_profit_overrides.insert(" ".into(), Tp3);
    assert!(c.validate().is_err());
}

// Every bot saved before this feature deserialises as TP1, no overrides.
#[test]
fn saved_config_without_targets_migrates_to_tp1() {
    let legacy = r#"{"kind":"futures","exchangeId":"binance","maxPositions":3,
        "capital":100.0,"leverage":3,"sizing":"risk","riskPerTradePct":1.0}"#;
    let c: BotConfig = serde_json::from_str(legacy).unwrap();
    assert_eq!(c.take_profit, Tp1);
    assert!(c.take_profit_overrides.is_empty());
}

#[test]
fn target_wire_shape() {
    let mut c = cfg();
    c.take_profit = Custom { pct: 40.0 };
    c.take_profit_overrides.insert("BTCUSDT".into(), Tp3);
    let v = serde_json::to_value(&c).unwrap();
    assert_eq!(
        v["takeProfit"],
        serde_json::json!({"kind": "custom", "pct": 40.0})
    );
    assert_eq!(
        v["takeProfitOverrides"]["BTCUSDT"],
        serde_json::json!({"kind": "tp3"})
    );
    let back: BotConfig = serde_json::from_value(v).unwrap();
    assert_eq!(back.take_profit_for("BTCUSDT"), Tp3);
}

// ---- TP1 is byte-identical to the pre-target book --------------------------

// The plan scenarios of tests.rs (R = 10, TP1 at 0.72R and 2R) run once with
// the legacy construction and once through `resolve_tp(Tp1)`: positions,
// every intermediate state, the exit and the trade record must match to
// the byte (bar the two new, additive fields' presence in both).
#[test]
fn tp1_outcomes_are_byte_identical_to_the_legacy_book() {
    let scenarios: [(f64, &[f64]); 5] = [
        (0.72, &[103.0, 111.0]),
        (2.0, &[110.0, 120.0]),
        (2.0, &[97.0, 90.0]),
        (2.0, &[110.0, 100.0]),
        (2.0, &[105.0, 100.0]),
    ];
    for dir in ["long", "short"] {
        let s = if dir == "long" { 1.0 } else { -1.0 };
        for (tp_r, prices) in scenarios {
            let mut sig = signal(dir, 100.0, 100.0 + s * tp_r * 10.0, 100.0 - s * 10.0);
            sig.management_plan = Some(plan(0.5, 1.0, 0.5));
            let mut legacy =
                new_position(&cfg(), &sig, 100.0, 1, 0, &TpResolution::signal_tp1(&sig));
            let mut resolved = position_with(&sig, Tp1, 100.0, Some(plan(0.5, 1.0, 0.5)));
            assert_eq!(
                serde_json::to_string(&legacy).unwrap(),
                serde_json::to_string(&resolved).unwrap()
            );
            for &px in prices {
                let px = 100.0 + s * (px - 100.0);
                let a = close_step(&mut legacy, px, NOW, false, None);
                let b = close_step(&mut resolved, px, NOW, false, None);
                assert_eq!(a, b, "{dir} {tp_r} {px}");
                assert_eq!(
                    serde_json::to_string(&legacy).unwrap(),
                    serde_json::to_string(&resolved).unwrap()
                );
                if let Some(exit) = a.exit {
                    let ra =
                        serde_json::to_string(&trade_record(&legacy, exit.price, exit.reason, 7))
                            .unwrap();
                    let rb =
                        serde_json::to_string(&trade_record(&resolved, exit.price, exit.reason, 7))
                            .unwrap();
                    assert_eq!(ra, rb);
                    break;
                }
            }
        }
    }
}

// ---- management with a farther target -------------------------------------

#[test]
fn tp3_does_not_exit_at_tp1_and_exits_whole_at_tp3() {
    for (sig, s) in [(long3(), 1.0), (short3(), -1.0)] {
        let mut p = position_with(&sig, Tp3, 100.0, None);
        assert_eq!(p.tp_target, "tp3");
        // Through TP1 and TP2: still open.
        for px in [104.5, 107.5, 116.0] {
            assert_eq!(
                close_step(&mut p, 100.0 + s * (px - 100.0), NOW, false, None).exit,
                None
            );
        }
        let exit = close_step(&mut p, 100.0 + s * 17.0, NOW, false, None)
            .exit
            .unwrap();
        assert_eq!(exit.reason, "tp");
        assert!(close(
            unrealized_net_pnl_pct(&p, exit.price),
            17.0 - FEES_1X
        ));
        assert_eq!(
            trade_record(&p, exit.price, exit.reason, 7)
                .tp_target
                .as_deref(),
            Some("tp3")
        );
    }
}

// With TP1 at 0.4R the 1R partial can never fire (it is not nearer than
// TP1). Against TP3 at 1.7R it is nearer, so it now fires: the rule compares
// against the RESOLVED target.
#[test]
fn partial_fires_when_nearer_than_the_resolved_target() {
    for (sig, s) in [(long3(), 1.0), (short3(), -1.0)] {
        let px = |v: f64| 100.0 + s * (v - 100.0);
        let mut at_tp1 = position_with(&sig, Tp1, 100.0, Some(plan(0.5, 1.0, 0.5)));
        let exit = close_step(&mut at_tp1, px(110.0), NOW, false, None)
            .exit
            .unwrap();
        assert_eq!(exit.reason, "tp");
        assert!(
            at_tp1.partial_price.is_none(),
            "no phantom partial beyond TP1"
        );

        let mut at_tp3 = position_with(&sig, Tp3, 100.0, Some(plan(0.5, 1.0, 0.5)));
        let step = close_step(&mut at_tp3, px(110.0), NOW, false, None);
        assert_eq!(step.exit, None);
        assert!(step.changed && at_tp3.partial_price.is_some() && at_tp3.breakeven_armed);
        let exit = close_step(&mut at_tp3, px(117.0), NOW, false, None)
            .exit
            .unwrap();
        assert_eq!(exit.reason, "tp");
        let pnl = unrealized_net_pnl_pct(&at_tp3, exit.price);
        assert!(close(pnl, 0.5 * 10.0 + 0.5 * 17.0 - FEES_1X), "{pnl}");
    }
}

// Partial at 1R and breakeven armed, then back to entry: a breakeven exit
// that keeps the banked half, exactly as with a nearer target.
#[test]
fn breakeven_still_protects_a_far_custom_target() {
    for (sig, s) in [(long3(), 1.0), (short3(), -1.0)] {
        let px = |v: f64| 100.0 + s * (v - 100.0);
        let mut p = position_with(&sig, Custom { pct: 40.0 }, 100.0, Some(plan(0.5, 1.0, 0.5)));
        assert!(close(p.tp, px(140.0)));
        assert_eq!(close_step(&mut p, px(110.0), NOW, false, None).exit, None);
        let exit = close_step(&mut p, px(100.0), NOW, false, None)
            .exit
            .unwrap();
        assert_eq!(exit.reason, "breakeven");
        assert!(close(unrealized_net_pnl_pct(&p, exit.price), 5.0 - FEES_1X));
    }
}

// A far target can outlive the signal's horizon: the position then exits
// on time at the live price, never at the unreached target.
#[test]
fn horizon_exits_before_a_far_target() {
    for (sig, s) in [(long3(), 1.0), (short3(), -1.0)] {
        let px = |v: f64| 100.0 + s * (v - 100.0);
        let mut p = position_with(&sig, Custom { pct: 40.0 }, 100.0, None);
        p.horizon_ms = 5_000;
        assert_eq!(close_step(&mut p, px(120.0), 4_999, false, None).exit, None);
        let exit = close_step(&mut p, px(120.0), 5_000, false, None)
            .exit
            .unwrap();
        assert_eq!((exit.reason, exit.price), ("horizon", px(120.0)));
        let rec = trade_record(&p, exit.price, exit.reason, 7);
        assert_eq!(
            (rec.exit_reason.as_str(), rec.tp_target.as_deref()),
            ("horizon", Some("custom:40"))
        );
    }
}
