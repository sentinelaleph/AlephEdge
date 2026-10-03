//! Sizing: every stop costs the declared share of capital regardless of stop
//! width, the leverage cap is surfaced when it binds, and fixed mode is the
//! pre-sizing behaviour exactly.

use super::entry::new_position;
use super::pnl::unrealized_net_pnl_pct;
use super::sizing::{apply_size, size_position, ROUND_TRIP_COST_FRAC, SKIP_INVALID_STOP};
use super::take_profit::TpResolution;
use super::test_fixtures::{cfg, signal};
use crate::bot::model::{BotConfig, BotKind, SizingMode, FUTURES_FEE_RATE};

fn risk_cfg(risk_pct: f64, leverage: u8) -> BotConfig {
    let mut c = cfg();
    (c.sizing, c.risk_per_trade_pct, c.capital, c.leverage) =
        (SizingMode::Risk, risk_pct, 1_000.0, leverage);
    c
}

// The ledger's range: a 3% and a 7% stop (avg 4.1%). At 1% risk both lose
// ≈1% of capital at the stop — the declared cost includes the 0.12% round
// trip, so the loss including costs is exactly the declared 10 USDT.
#[test]
fn narrow_and_wide_stops_lose_the_same_share_of_capital() {
    let c = risk_cfg(1.0, 20);
    for (dir, sl) in [
        ("long", 97.0),
        ("long", 93.0),
        ("short", 103.0),
        ("short", 107.0),
    ] {
        let stop_frac = (100.0_f64 - sl).abs() / 100.0;
        let size = size_position(&c, 20, 100.0, sl).unwrap();
        assert!(!size.risk_capped, "{dir} {sl}");
        let loss_with_cost = size.notional * (stop_frac + ROUND_TRIP_COST_FRAC);
        assert!(
            (loss_with_cost - 10.0).abs() < 1e-9,
            "{dir} {sl}: {loss_with_cost}"
        );

        // Through the engine's own PnL at the stop (desk taker model 0.10%
        // round trip, slightly below the 0.12% sized for): ≈ −1% of capital.
        let tp = if dir == "long" { 110.0 } else { 90.0 };
        let sig = signal(dir, 100.0, tp, sl);
        let mut pos = new_position(&c, &sig, 100.0, 20, 0, &TpResolution::signal_tp1(&sig));
        apply_size(&mut pos, &size);
        let pnl = unrealized_net_pnl_pct(&pos, sl);
        assert!((pnl + 1.0).abs() < 0.02, "{dir} {sl}: {pnl}");
        assert!((pos.effective_leverage - size.notional / 1_000.0).abs() < 1e-12);
    }
    // Same risk, different notional: the 3% stop carries 2.3x the size.
    let narrow = size_position(&c, 20, 100.0, 97.0).unwrap();
    let wide = size_position(&c, 20, 100.0, 93.0).unwrap();
    assert!(narrow.notional > 2.0 * wide.notional);
    assert!(wide.effective_leverage < 1.5, "wide stop may run below x2");
}

#[test]
fn leverage_cap_binds_and_says_so() {
    // 0.5% stop at 5% risk wants 5% / 0.62% ≈ x8 notional; the cap is x3.
    let c = risk_cfg(5.0, 3);
    let size = size_position(&c, 3, 100.0, 99.5).unwrap();
    assert!(size.risk_capped);
    assert_eq!((size.notional, size.effective_leverage), (3_000.0, 3.0));
    assert_eq!(
        size.risk_pct,
        Some(5.0),
        "declared stays declared; actual is lower"
    );
    assert!(size.notional * (0.005 + ROUND_TRIP_COST_FRAC) < 50.0);
}

#[test]
fn invalid_stop_is_a_skip() {
    let c = risk_cfg(1.0, 5);
    assert_eq!(size_position(&c, 5, 100.0, 100.0), Err(SKIP_INVALID_STOP));
    assert_eq!(
        size_position(&c, 5, 100.0, f64::NAN),
        Err(SKIP_INVALID_STOP)
    );
    assert_eq!(size_position(&c, 5, 0.0, 90.0), Err(SKIP_INVALID_STOP));
    assert!(super::precheck::is_final(SKIP_INVALID_STOP));
}

// Fixed mode must reproduce the old arithmetic bit for bit: notional =
// capital × leverage and PnL = move × leverage − 2 × fee × leverage.
#[test]
fn fixed_mode_is_identical_to_before() {
    let mut c = cfg();
    (c.capital, c.leverage) = (250.0, 5);
    let size = size_position(&c, 5, 100.0, 90.0).unwrap();
    assert_eq!(
        (size.mode, size.risk_pct, size.risk_capped),
        (SizingMode::Fixed, None, false)
    );
    assert_eq!((size.notional, size.effective_leverage), (1_250.0, 5.0));

    let sig = signal("long", 100.0, 110.0, 90.0);
    let mut pos = new_position(&c, &sig, 100.0, 5, 0, &TpResolution::signal_tp1(&sig));
    let before = unrealized_net_pnl_pct(&pos, 103.0);
    apply_size(&mut pos, &size);
    let old_formula = 3.0 * f64::from(5u8) - 2.0 * FUTURES_FEE_RATE * f64::from(5u8) * 100.0;
    assert_eq!(
        unrealized_net_pnl_pct(&pos, 103.0).to_bits(),
        old_formula.to_bits()
    );
    assert_eq!(before.to_bits(), old_formula.to_bits());
    // A position persisted before sizing (effective_leverage 0) still uses
    // its configured leverage.
    pos.effective_leverage = 0.0;
    assert_eq!(
        unrealized_net_pnl_pct(&pos, 103.0).to_bits(),
        old_formula.to_bits()
    );
}

#[test]
fn saved_config_without_sizing_is_fixed_and_new_config_is_risk_1pct() {
    let saved = r#"{"kind":"futures","exchangeId":"binance","maxPositions":3,
        "capital":100.0,"leverage":5}"#;
    let old: BotConfig = serde_json::from_str(saved).unwrap();
    assert_eq!(old.sizing, SizingMode::Fixed);
    assert_eq!(old.risk_per_trade_pct, 1.0);

    let new = BotConfig::new_default(BotKind::Futures, "binance");
    assert_eq!(
        (new.sizing, new.risk_per_trade_pct),
        (SizingMode::Risk, 1.0)
    );
    assert!(!new.live, "a default never opts into live orders");
    let json = serde_json::to_string(&new).unwrap();
    assert!(json.contains(r#""sizing":"risk""#) && json.contains(r#""riskPerTradePct":1.0"#));
}

#[test]
fn risk_bounds_are_validated() {
    let mut c = BotConfig::new_default(BotKind::Futures, "binance");
    for ok in [0.1, 1.0, 5.0] {
        c.risk_per_trade_pct = ok;
        assert!(c.validate().is_ok(), "{ok}");
    }
    for bad in [0.0, 0.09, 5.01, f64::NAN] {
        c.risk_per_trade_pct = bad;
        assert!(c.validate().is_err(), "{bad}");
    }
    c.sizing = SizingMode::Fixed;
    assert!(c.validate().is_ok(), "fixed mode ignores the risk field");
}
