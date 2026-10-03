//! Rust ports of the research simulator's `botsim.py --self-test` cases 1-9:
//! the same synthetic 1h bars fed to the same pure step functions, P&L
//! checked to 1e-9 against the hand-computed formula AND the published
//! number. Plus intrabar property tests (causality, gap, ties, protective
//! first, trailing).

use super::costs::FUTURES_SIM;
use super::cycle::{step_bar, CycleStart, CycleState, Engine};
use super::model::{
    Bar, DcaParams, ExitReason, FillKind, GridParams, GridRange, Liquidity, Side, Spacing,
};

const H: u64 = 3_600_000;
const T0: u64 = 1_736_121_600_000; // 2025-01-06 00:00 UTC, as in botsim

fn bars(ohlc: &[(f64, f64, f64, f64)], funding: &[(usize, f64)]) -> Vec<Bar> {
    ohlc.iter()
        .enumerate()
        .map(|(i, (o, h, l, c))| Bar {
            open_ms: T0 + i as u64 * H,
            close_ms: T0 + (i as u64 + 1) * H - 1,
            o: *o,
            h: *h,
            l: *l,
            c: *c,
            funding_rate: funding.iter().find(|(k, _)| *k == i).map(|(_, r)| *r),
            funding_unknown: false,
        })
        .collect()
}

fn start(side: Side, budget: f64, lev: f64) -> CycleStart<'static> {
    CycleStart {
        bot_id: "sb_golden000000",
        seq: 1,
        side,
        budget,
        leverage: lev,
        size_factor: 1.0,
        costs: FUTURES_SIM,
        max_duration_min: None,
    }
}

/// Runs one cycle over all bars; an open cycle is closed at the last close
/// ("end"), as the simulator does at data end. Returns (cycle, hours).
fn run(mut c: CycleState, bars: &[Bar]) -> (CycleState, u64) {
    let mut end_k = 0;
    for (k, b) in bars.iter().enumerate() {
        step_bar(&mut c, b, k == 0);
        end_k = k;
        if !c.is_open() {
            break;
        }
    }
    if c.is_open() {
        c.close_market(bars[end_k].c, ExitReason::End);
    }
    (c, end_k as u64 + 1)
}

fn dca(base: f64, so: Option<f64>, max_so: u8, tp: f64, sl: Option<f64>) -> DcaParams {
    DcaParams {
        base_order: Some(base),
        safety_order: so,
        base_weight: None,
        safety_weight: None,
        max_so,
        so_step_pct: 2.0,
        step_scale: 1.0,
        volume_scale: 1.0,
        tp_pct: tp,
        trailing_pct: None,
        sl_pct: sl,
        max_duration_min: None,
    }
}

fn run_dca(ohlc: &[(f64, f64, f64, f64)], p: DcaParams, side: Side, lev: f64, funding: &[(usize, f64)]) -> (CycleState, u64) {
    let b = bars(ohlc, funding);
    let c = CycleState::open_dca(&start(side, 1000.0, lev), &p, &b[0]).unwrap();
    run(c, &b)
}

fn grid_params(stop: Option<f64>) -> GridParams {
    GridParams {
        range: GridRange::Absolute { lower: 90.0, upper: 110.0 },
        n_grids: 2,
        spacing: Spacing::Arith,
        stop_out_pct: stop,
        trailing_up: false,
        trail_up_limit: None,
        take_profit_pct: None,
        max_duration_min: None,
    }
}

fn run_grid(ohlc: &[(f64, f64, f64, f64)], p: GridParams, side: Side) -> (CycleState, u64) {
    let b = bars(ohlc, &[]);
    let c = CycleState::open_grid(&start(side, 1000.0, 1.0), &p, &b[0]).unwrap();
    run(c, &b)
}

fn close(got: f64, want: f64, tol: f64) {
    assert!((got - want).abs() <= tol * want.abs().max(1.0), "got {got}, want {want}");
}

fn std_dca() -> DcaParams {
    dca(100.0, Some(100.0), 2, 2.0, None)
}

// ---------------------------------------------------------------- golden

#[test]
fn golden_1_dca_tp_with_so_and_funding() {
    let (c, hours) = run_dca(
        &[(100.0, 100.5, 99.5, 100.2), (100.2, 100.4, 97.0, 97.5), (97.5, 101.5, 97.4, 101.0), (101.0, 101.2, 100.8, 101.0)],
        std_dca(),
        Side::Long,
        1.0,
        &[(2, 0.0001)],
    );
    let q = 100.0 / (100.0 * 1.0002) + 100.0 / 98.0;
    let tp = 200.0 / q * 1.02;
    let exp = q * tp - 200.0 - 100.0 * 0.0005 - 100.0 * 0.0002 - q * 97.5 * 0.0001 - q * tp * 0.0002;
    close(c.core.cash, exp, 1e-9);
    close(c.core.cash, 3.86950297, 1e-8);
    assert_eq!(c.core.exit, Some(ExitReason::Tp));
    assert_eq!(c.so_filled(), Some(1));
    assert_eq!(hours, 3);
}

#[test]
fn golden_2_dca_so_then_tp_in_one_bullish_bar() {
    let (c, hours) = run_dca(
        &[(100.0, 100.5, 99.5, 100.2), (100.2, 101.5, 97.0, 101.0), (101.0, 101.0, 101.0, 101.0)],
        std_dca(),
        Side::Long,
        1.0,
        &[],
    );
    let q = 100.0 / (100.0 * 1.0002) + 100.0 / 98.0;
    let tp = 200.0 / q * 1.02;
    close(c.core.cash, q * tp - 200.0 - 0.05 - 0.02 - q * tp * 0.0002, 1e-9);
    close(c.core.cash, 3.8892, 1e-9);
    assert_eq!(c.core.exit, Some(ExitReason::Tp));
    assert_eq!(hours, 2);
}

#[test]
fn golden_3_dca_bearish_bar_no_tp_then_end() {
    let (c, _) = run_dca(
        &[(100.0, 100.5, 99.5, 100.2), (100.2, 101.5, 97.0, 97.5), (97.5, 98.0, 97.2, 97.6)],
        std_dca(),
        Side::Long,
        1.0,
        &[],
    );
    let q = 100.0 / (100.0 * 1.0002) + 100.0 / 98.0;
    let px = 97.6 * (1.0 - 0.0002);
    close(c.core.cash, q * px - 200.0 - 0.05 - 0.02 - q * px * 0.0005, 1e-9);
    close(c.core.cash, -3.0356802693, 1e-9);
    assert_eq!(c.core.exit, Some(ExitReason::End));
}

#[test]
fn golden_4_stop_wins_over_tp_in_one_bar() {
    let (c, _) = run_dca(
        &[(100.0, 100.3, 99.8, 100.0), (100.0, 101.5, 96.5, 97.0), (97.0, 97.0, 97.0, 97.0)],
        dca(100.0, None, 0, 1.0, Some(3.0)),
        Side::Long,
        1.0,
        &[],
    );
    let q = 100.0 / 100.02;
    let px = 100.02 * 0.97 * (1.0 - 0.0002);
    close(c.core.cash, q * px - 100.0 - 0.05 - q * px * 0.0005, 1e-9);
    close(c.core.cash, -3.1178903, 1e-9);
    assert_eq!(c.core.exit, Some(ExitReason::Sl));
}

#[test]
fn golden_5_short_10x_liquidation() {
    let (c, _) = run_dca(
        &[(100.0, 100.5, 99.6, 100.1), (100.1, 112.0, 100.0, 111.0), (111.0, 111.0, 111.0, 111.0), (111.0, 111.0, 111.0, 111.0)],
        dca(5000.0, None, 0, 5.0, None),
        Side::Short,
        10.0,
        &[],
    );
    close(c.core.cash, -500.0 - 5000.0 * 0.0005, 1e-9);
    close(c.core.cash, -502.5, 1e-9);
    assert_eq!(c.core.exit, Some(ExitReason::Liq));
}

#[test]
fn golden_6_funding_on_quiet_bars() {
    let flat = (100.5, 100.6, 100.4, 100.5);
    let (c, _) = run_dca(
        &[(100.0, 100.5, 99.8, 100.5), flat, flat, flat, flat],
        dca(100.0, None, 0, 5.0, None),
        Side::Long,
        1.0,
        &[(2, 0.0003), (3, -0.0001)],
    );
    let q = 100.0 / 100.02;
    let px = 100.5 * (1.0 - 0.0002);
    close(c.core.cash, q * px - 100.0 - 0.05 - q * 100.5 * (0.0003 - 0.0001) - q * px * 0.0005, 1e-9);
    assert!((c.core.funding - q * 100.5 * 0.0002).abs() < 1e-12);
    assert_eq!(c.core.exit, Some(ExitReason::End));
}

fn grid_bars() -> Vec<(f64, f64, f64, f64)> {
    vec![
        (100.0, 101.0, 99.0, 100.5),
        (100.5, 100.6, 89.0, 91.0),
        (91.0, 111.0, 90.5, 110.5),
        (110.5, 110.8, 99.5, 100.5),
    ]
}

#[test]
fn golden_7_long_grid_round_trips() {
    let (c, _) = run_grid(&grid_bars(), grid_params(None), Side::Long);
    let q = 1000.0 / 210.0;
    let a2 = (100.02 + 90.0) / 2.0;
    let px = 100.5 * (1.0 - 0.0002);
    let fees = q * 100.02 * 0.0005 + q * 90.0 * 0.0002 + q * 100.0 * 0.0002 + q * 110.0 * 0.0002 + q * 100.0 * 0.0002 + q * px * 0.0005;
    close(c.core.cash, q * (100.0 - a2) + q * (110.0 - a2) + q * (px - 100.0) - fees, 1e-9);
    close(c.core.cash, 96.5697621429, 1e-9);
    assert_eq!(c.core.exit, Some(ExitReason::End));
}

#[test]
fn golden_8_long_grid_stop_out() {
    let mut b = grid_bars();
    b[3] = (110.5, 110.8, 80.0, 82.0);
    let (c, _) = run_grid(&b, grid_params(Some(5.0)), Side::Long);
    let q = 1000.0 / 210.0;
    let a2 = (100.02 + 90.0) / 2.0;
    let px = 85.5 * (1.0 - 0.0002);
    let fees = q * 100.02 * 0.0005 + q * 90.0 * 0.0002 + q * 100.0 * 0.0002 + q * 110.0 * 0.0002
        + q * 100.0 * 0.0002 + q * 90.0 * 0.0002 + 2.0 * q * px * 0.0005;
    close(c.core.cash, q * (100.0 - a2) + q * (110.0 - a2) + 2.0 * q * (px - 95.0) - fees, 1e-9);
    close(c.core.cash, 3.3919385714, 1e-9);
    assert_eq!(c.core.exit, Some(ExitReason::Stop));
}

#[test]
fn golden_9_neutral_grid_short_round_trip() {
    let (c, _) = run_grid(
        &[(100.0, 100.5, 99.5, 100.0), (100.0, 110.5, 99.8, 101.0), (101.0, 101.2, 99.5, 100.4)],
        grid_params(None),
        Side::Neutral,
    );
    let q = 1000.0 / 210.0;
    close(c.core.cash, q * 10.0 - q * 110.0 * 0.0002 - q * 100.0 * 0.0002, 1e-9);
    close(c.core.cash, 47.419047619, 1e-9);
    assert_eq!(c.grid_closing_fills(), Some(1));
}

// ---------------------------------------------------------------- properties

#[test]
fn hand_computed_dca_cycle_events_and_fees() {
    let b = bars(
        &[(100.0, 100.5, 99.5, 100.2), (100.2, 100.4, 97.0, 97.5), (97.5, 101.5, 97.4, 101.0)],
        &[],
    );
    let mut c = CycleState::open_dca(&start(Side::Long, 1000.0, 1.0), &std_dca(), &b[0]).unwrap();
    let mut ev = step_bar(&mut c, &b[0], true);
    ev.extend(step_bar(&mut c, &b[1], false));
    ev.extend(step_bar(&mut c, &b[2], false));
    let roles: Vec<&str> = ev.iter().map(|f| f.client_id.rsplit('-').next().unwrap()).collect();
    assert_eq!(roles, vec!["base", "so1", "tp1"]);
    assert_eq!(ev[0].liquidity, Liquidity::Taker);
    assert_eq!(ev[1].liquidity, Liquidity::Maker);
    assert!((ev[1].price - 98.0).abs() < 1e-12);
    let fees: f64 = ev.iter().map(|f| f.fee_quote).sum();
    assert!((fees - c.core.fees).abs() < 1e-12);
    let realized: f64 = ev.iter().map(|f| f.realized_quote).sum();
    assert!((realized - fees - c.core.cash).abs() < 1e-9);
}

#[test]
fn grid_round_trip_counts_closing_fills() {
    let (c, _) = run_grid(&grid_bars(), grid_params(None), Side::Long);
    // sells at 100 and 110 close buys; the last buy at 100 opens again
    assert_eq!(c.grid_closing_fills(), Some(3));
}

#[test]
fn causality_new_tp_cannot_fill_on_the_creating_leg() {
    // Bearish bar: O -> H (101.5 > the post-SO TP) -> L (SO fill) -> C.
    // The TP created by the SO on the L leg would be crossed by the H leg
    // only if the path were reordered; it must not fill.
    let (c, _) = run_dca(
        &[(100.0, 100.5, 99.5, 100.2), (100.2, 101.5, 97.0, 97.5), (97.5, 98.0, 97.2, 97.6)],
        std_dca(),
        Side::Long,
        1.0,
        &[],
    );
    assert_eq!(c.core.exit, Some(ExitReason::End));
    // grid: the counter sell of a buy is active only from the next leg
    let b = bars(&[(100.0, 101.0, 99.0, 100.5), (100.5, 100.6, 89.0, 91.0)], &[]);
    let mut g = CycleState::open_grid(&start(Side::Long, 1000.0, 1.0), &grid_params(None), &b[0]).unwrap();
    step_bar(&mut g, &b[0], true);
    let ev = step_bar(&mut g, &b[1], false);
    let buy_leg = ev.iter().find(|f| f.qty > 0.0).unwrap().leg;
    let sell = g.open_orders().into_iter().find(|o| o.price == 100.0).unwrap();
    assert!(sell.active_from_leg > buy_leg);
}

#[test]
fn gap_fills_limits_at_their_price_and_stops_at_the_open() {
    // SO at 98 crossed by a gap 100 -> 95: fills at 98, not 95.
    let (c, _) = run_dca(
        &[(100.0, 100.2, 99.9, 100.0), (95.0, 95.5, 94.8, 95.2)],
        std_dca(),
        Side::Long,
        1.0,
        &[],
    );
    assert_eq!(c.so_filled(), Some(2), "both SOs (98, 96) crossed by the gap");
    let Engine::Dca(d) = &c.engine else { panic!() };
    let q = 100.0 / 100.02 + 100.0 / 98.0 + 100.0 / 96.0;
    assert!((d.q - q).abs() < 1e-12);
    // stop crossed by a gap fills at the open
    let b = bars(&[(100.0, 100.2, 99.9, 100.0), (90.0, 90.5, 89.0, 90.2)], &[]);
    let mut c = CycleState::open_dca(&start(Side::Long, 1000.0, 1.0), &dca(100.0, None, 0, 5.0, Some(3.0)), &b[0]).unwrap();
    step_bar(&mut c, &b[0], true);
    let ev = step_bar(&mut c, &b[1], false);
    assert_eq!(c.core.exit, Some(ExitReason::Sl));
    assert!((ev.last().unwrap().price - 90.0 * (1.0 - 0.0002)).abs() < 1e-12);
}

#[test]
fn tie_order_liquidation_then_fill_then_stop() {
    // Same price for liquidation and stop -> liquidation.
    let b = bars(&[(100.0, 100.2, 99.9, 100.0), (100.0, 100.0, 40.0, 45.0)], &[]);
    let mut c = CycleState::open_dca(&start(Side::Long, 1000.0, 2.0), &dca(100.0, None, 0, 5.0, Some(10.0)), &b[0]).unwrap();
    step_bar(&mut c, &b[0], true);
    if let Engine::Dca(d) = &mut c.engine {
        d.sl = d.liq;
        assert!(d.liq.is_some());
    }
    step_bar(&mut c, &b[1], false);
    assert_eq!(c.core.exit, Some(ExitReason::Liq));
    // Same price for a safety order and the stop -> the simulator fills the
    // safety order first (priority 1 < 2), then the re-placed stop fires.
    let mut c = CycleState::open_dca(&start(Side::Long, 1000.0, 1.0), &dca(100.0, Some(100.0), 1, 50.0, Some(10.0)), &b[0]).unwrap();
    step_bar(&mut c, &b[0], true);
    if let Engine::Dca(d) = &mut c.engine {
        d.sl = Some(d.so_px[0]);
    }
    let ev = step_bar(&mut c, &b[1], false);
    let roles: Vec<&str> = ev.iter().map(|f| f.client_id.rsplit('-').next().unwrap()).collect();
    assert_eq!(roles[0], "so1");
    assert_eq!(c.core.exit, Some(ExitReason::Sl));
    // Grid: stop-out level equal to a buy level -> the stop wins.
    let p = GridParams {
        range: GridRange::Absolute { lower: 90.0, upper: 110.0 },
        n_grids: 2,
        stop_out_pct: Some(5.0),
        ..grid_params(None)
    };
    let gb = bars(&[(100.0, 100.2, 99.9, 100.0), (100.0, 100.0, 80.0, 81.0)], &[]);
    let mut g = CycleState::open_grid(&start(Side::Long, 1000.0, 1.0), &p, &gb[0]).unwrap();
    step_bar(&mut g, &gb[0], true);
    if let Engine::Grid(s) = &mut g.engine {
        s.lo_stop = Some(90.0);
    }
    let ev = step_bar(&mut g, &gb[1], false);
    assert_eq!(g.core.exit, Some(ExitReason::Stop));
    assert!(ev.iter().all(|f| f.liquidity == Liquidity::Taker), "no buy filled at the stop level");
}

#[test]
fn protective_first_for_sl_stop_out_and_liquidation() {
    // DCA: covered by golden_4 (TP and SL in one down bar -> SL).
    // Grid stop-out: bullish bar visits the high (a sell) before the low
    // (beyond the stop band) on the natural path; the harmful path stops.
    let p = grid_params(Some(5.0));
    let b = bars(&[(100.0, 100.2, 99.9, 100.0), (100.0, 110.5, 84.0, 99.0)], &[]);
    let mut g = CycleState::open_grid(&start(Side::Long, 1000.0, 1.0), &p, &b[0]).unwrap();
    step_bar(&mut g, &b[0], true);
    let ev = step_bar(&mut g, &b[1], false);
    assert_eq!(g.core.exit, Some(ExitReason::Stop));
    assert!(
        !ev.iter().any(|f| f.qty < 0.0 && f.liquidity == Liquidity::Maker),
        "the natural path's sell at 110 was not committed"
    );
    // Liquidation: short 10x on a bullish bar: the natural path visits the
    // low (TP) first, the harmful path the high (liquidation).
    let (c, _) = run_dca(
        &[(100.0, 100.5, 99.6, 100.1), (100.1, 112.0, 94.0, 101.0)],
        dca(5000.0, None, 0, 5.0, None),
        Side::Short,
        10.0,
        &[],
    );
    assert_eq!(c.core.exit, Some(ExitReason::Liq));
}

#[test]
fn trailing_tp_arms_then_exits_on_a_later_retrace() {
    let mut p = dca(100.0, None, 0, 2.0, None);
    p.trailing_pct = Some(1.0);
    let b = bars(
        &[(100.0, 100.2, 99.9, 100.0), (100.0, 103.0, 99.95, 102.9), (102.9, 104.0, 102.8, 103.9), (103.9, 104.0, 102.0, 102.2)],
        &[],
    );
    let mut c = CycleState::open_dca(&start(Side::Long, 1000.0, 1.0), &p, &b[0]).unwrap();
    step_bar(&mut c, &b[0], true);
    step_bar(&mut c, &b[1], false);
    assert!(c.is_open(), "TP touched: trailing armed, no fill");
    step_bar(&mut c, &b[2], false);
    assert!(c.is_open());
    let ev = step_bar(&mut c, &b[3], false);
    assert_eq!(c.core.exit, Some(ExitReason::Trail));
    let exit = ev.last().unwrap();
    assert_eq!(exit.liquidity, Liquidity::Taker);
    assert!((exit.price - 104.0 * 0.99 * (1.0 - 0.0002)).abs() < 1e-9);
}

#[test]
fn funding_rows_are_recorded_and_unknown_is_flagged() {
    let mut b = bars(&[(100.0, 100.5, 99.8, 100.5), (100.5, 100.6, 100.4, 100.5)], &[(1, 0.0001)]);
    let mut c = CycleState::open_dca(&start(Side::Long, 1000.0, 1.0), &dca(100.0, None, 0, 5.0, None), &b[0]).unwrap();
    step_bar(&mut c, &b[0], true);
    let ev = step_bar(&mut c, &b[1], false);
    assert_eq!(ev.iter().filter(|f| f.kind == FillKind::Funding).count(), 1);
    b[1].funding_rate = None;
    b[1].funding_unknown = true;
    let mut c = CycleState::open_dca(&start(Side::Long, 1000.0, 1.0), &dca(100.0, None, 0, 5.0, None), &b[0]).unwrap();
    step_bar(&mut c, &b[0], true);
    step_bar(&mut c, &b[1], false);
    assert!(c.core.funding_unknown);
    assert_eq!(c.core.funding, 0.0);
}

#[test]
fn a_flat_neutral_grid_records_no_zero_funding_row() {
    // Opened at the range mid with no inventory; the funding bar trades no
    // level, so the position is flat when funding settles.
    let b = bars(&[(100.0, 100.5, 99.5, 100.0), (100.0, 100.4, 99.6, 100.1)], &[(1, 0.0001)]);
    let mut g = CycleState::open_grid(&start(Side::Neutral, 1000.0, 1.0), &grid_params(None), &b[0]).unwrap();
    step_bar(&mut g, &b[0], true);
    let ev = step_bar(&mut g, &b[1], false);
    assert!(g.is_open());
    assert_eq!(g.core.funding, 0.0);
    assert!(
        !ev.iter().any(|f| f.kind == FillKind::Funding),
        "no funding row for a flat position: {ev:?}"
    );
}

#[test]
fn max_duration_closes_at_the_bar_close() {
    let mut p = dca(100.0, None, 0, 50.0, None);
    p.max_duration_min = Some(120);
    let b = bars(&[(100.0, 100.5, 99.8, 100.5), (100.5, 100.6, 100.4, 100.5), (100.5, 101.0, 100.0, 100.8)], &[]);
    let mut st = start(Side::Long, 1000.0, 1.0);
    st.max_duration_min = p.max_duration_min;
    let mut c = CycleState::open_dca(&st, &p, &b[0]).unwrap();
    step_bar(&mut c, &b[0], true);
    let ev = step_bar(&mut c, &b[1], false);
    assert_eq!(c.core.exit, Some(ExitReason::Timeout));
    assert!((ev.last().unwrap().price - 100.5 * (1.0 - 0.0002)).abs() < 1e-12);
}

#[test]
fn grid_trailing_up_shifts_and_counts() {
    let p = GridParams {
        trailing_up: true,
        ..grid_params(Some(5.0))
    };
    let b = bars(&[(100.0, 100.2, 99.9, 100.0), (100.0, 121.0, 99.9, 120.5)], &[]);
    let mut g = CycleState::open_grid(&start(Side::Long, 1000.0, 1.0), &p, &b[0]).unwrap();
    step_bar(&mut g, &b[0], true);
    step_bar(&mut g, &b[1], false);
    let Engine::Grid(s) = &g.engine else { panic!() };
    assert!(g.is_open());
    assert_eq!(s.shifts, 1);
    assert!((s.levels[2] - 120.0).abs() < 1e-12);
}

#[test]
fn grid_take_profit_on_bar_close() {
    let p = GridParams {
        take_profit_pct: Some(5.0),
        ..grid_params(None)
    };
    let (c, _) = run_grid(&grid_bars(), p, Side::Long);
    assert_eq!(c.core.exit, Some(ExitReason::GridTp));
}

#[test]
fn grid_start_outside_band_is_refused() {
    let p = GridParams {
        range: GridRange::Absolute { lower: 90.0, upper: 110.0 },
        ..grid_params(Some(5.0))
    };
    let b = bars(&[(120.0, 121.0, 119.0, 120.0)], &[]);
    let r = CycleState::open_grid(&start(Side::Long, 1000.0, 1.0), &p, &b[0]);
    assert_eq!(r.err(), Some("gridStartOutsideBand"));
}
