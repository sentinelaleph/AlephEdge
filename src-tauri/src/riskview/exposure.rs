//! One open exposure of the book and how it fares along a price path (pure).
//!
//! Two bodies, each walked by the code that manages it for real:
//! - a signal-bot position (bot-opened, Execute or New trade): the engine's
//!   own `management::close_step_with` at every point of the path, so the
//!   stop (or the entry once breakeven armed), the take-profit, the partial,
//!   the user's max-loss cap and the paper liquidation exit exactly where the
//!   desk would. P&L is `pnl::unrealized_net_pnl_pct`: fees included, an
//!   isolated position floored at its margin.
//! - an open DCA / Grid cycle: a clone of the live `CycleState` stepped bar by
//!   bar through `cycle::step_bar`, so safety orders and grid buys fill on
//!   the way down, the stop-loss / stop-out / liquidation and the bot's
//!   drawdown stop close it, all with the paper engine's fees and slippage.
//!   A new cycle after an exit is not simulated.
//!
//! The result is the change from now (mark-to-market at the current mark),
//! in USDT, scaled by `scale` (a live strategy bot's real / simulated size).

use serde::Serialize;

use crate::bot::engine::management::{self, Fill, EXIT_LIQUIDATION};
use crate::bot::engine::{pnl, sizing};
use crate::bot::model::OpenPosition;
use crate::bot::strategy::cycle::{step_bar, CycleState};
use crate::bot::strategy::model::{Bar, ExitReason, MarketKind};
use crate::bot::strategy::path::bar_paths;
use crate::signal::model::Direction;

/// A shocked price never goes below this share of the mark (a 3x beta at
/// BTC -40% would otherwise reach zero or below).
pub const PRICE_FLOOR: f64 = 0.01;
/// Largest relative step when a position is walked along a monotone leg:
/// the max-loss cap books at the price it is seen at, so the step bounds
/// its overshoot (0.05%).
const SWEEP_STEP: f64 = 0.0005;
const HOUR_MS: u64 = 3_600_000;

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// Opened by a signal bot's own loop.
    Signal,
    /// Opened by the user (Execute on a signal, or New trade).
    Manual,
    Dca,
    Grid,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Dir {
    Long,
    Short,
    Neutral,
}

/// The bot drawdown stop of a strategy bot (driver.rs semantics).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DdStop {
    pub pct: f64,
    pub peak_equity: f64,
    pub budget: f64,
    pub realized: f64,
}

#[derive(Clone, Debug)]
pub enum Body {
    Position { pos: Box<OpenPosition>, max_loss_pct: Option<f64> },
    Cycle { cycle: Box<CycleState>, dd: Option<DdStop> },
}

#[derive(Clone, Debug)]
pub struct Exposure {
    /// "pos:<botKind>:<signalId>" or "sb:<botId>".
    pub id: String,
    pub source: Source,
    /// Bot kind ("futures" / "spot" / "pump") or the strategy bot's name.
    pub label: String,
    pub symbol: String,
    pub market: MarketKind,
    pub live: bool,
    /// Real / simulated size (live strategy pilot); 1 otherwise.
    pub scale: f64,
    pub direction: Dir,
    /// Capital the exposure ties up (position capital; the cycle's budget),
    /// already scaled.
    pub capital: f64,
    /// Current price the change is measured from.
    pub mark: f64,
    pub body: Body,
}

/// How the exposure ended up at the end of the path.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    /// Change from now, USDT (negative = loss), scaled.
    pub pnl: f64,
    /// Exit taken on the way (`riskview.exit.*`); None = still open.
    pub exit: Option<&'static str>,
}

/// The coin's price after BTC moves `btc_move_pct` %, through `beta`.
pub fn shocked_price(mark: f64, beta: f64, btc_move_pct: f64) -> f64 {
    mark * (1.0 + beta * btc_move_pct / 100.0).max(PRICE_FLOOR)
}

/// One hourly bar from `from` straight to `to`, opening at `t0`.
pub fn shock_bar(from: f64, to: f64, t0: u64) -> Bar {
    Bar {
        open_ms: t0,
        close_ms: t0 + HOUR_MS - 1,
        o: from,
        h: from.max(to),
        l: from.min(to),
        c: to,
        funding_rate: None,
        funding_unknown: false,
    }
}

fn exit_code_position(reason: &str) -> &'static str {
    match reason {
        EXIT_LIQUIDATION => "liq",
        "sl" => "sl",
        "breakeven" => "breakeven",
        "tp" => "tp",
        "max_loss" => "maxLoss",
        "horizon" => "horizon",
        _ => "other",
    }
}

fn exit_code_cycle(r: ExitReason) -> &'static str {
    match r {
        ExitReason::Tp => "tp",
        ExitReason::Trail => "trail",
        ExitReason::Sl => "sl",
        ExitReason::Liq => "liq",
        ExitReason::Stop => "stop",
        ExitReason::GridTp => "gridTp",
        ExitReason::Timeout => "timeout",
        ExitReason::Ddstop => "ddstop",
        _ => "other",
    }
}

fn position_usdt(pos: &OpenPosition, price: f64) -> f64 {
    pos.capital * pnl::unrealized_net_pnl_pct(pos, price) / 100.0
}

impl Exposure {
    /// The nearest protective level now (signal stop / breakeven entry; DCA
    /// stop-loss, grid stop-out or drawdown stop price).
    pub fn stop(&self) -> Option<f64> {
        match &self.body {
            Body::Position { pos, .. } => Some(management::stop_level(pos)).filter(|s| *s > 0.0),
            Body::Cycle { cycle, .. } => cycle.protective_stop(),
        }
    }

    /// Isolated liquidation price, if the exposure can be liquidated.
    pub fn liq(&self) -> Option<f64> {
        match &self.body {
            Body::Position { pos, .. } => sizing::liquidation_price(pos),
            Body::Cycle { cycle, .. } => cycle.liq_price(),
        }
    }

    /// Walks the exposure through `bars` (coin prices, in time order; the
    /// first bar's open may differ from the mark: that jump is a gap).
    pub fn walk(&self, bars: &[Bar]) -> Outcome {
        let (pnl, exit) = match &self.body {
            Body::Position { pos, max_loss_pct } => walk_position(pos, *max_loss_pct, self.mark, bars),
            Body::Cycle { cycle, dd } => walk_cycle(cycle, dd.as_ref(), self.mark, bars),
        };
        Outcome { pnl: pnl * self.scale, exit }
    }

    /// Mark-to-market at the current mark, fees paid or owed included,
    /// scaled. A candidate's cost of entry: added to its shock change, it
    /// gives what opening it and then the shock would book.
    pub fn mtm_now(&self) -> f64 {
        let v = match &self.body {
            Body::Position { pos, .. } => position_usdt(pos, self.mark),
            Body::Cycle { cycle, .. } => cycle.mtm(self.mark),
        };
        v * self.scale
    }

    /// The exposure after BTC moves `btc_move_pct` % straight from now.
    pub fn shock(&self, beta: f64, btc_move_pct: f64, t0: u64) -> Outcome {
        let to = shocked_price(self.mark, beta, btc_move_pct);
        self.walk(&[shock_bar(self.mark, to, t0)])
    }
}

/// One evaluation of the position at `price`; Some(P&L USDT, exit) on a close.
fn eval(p: &mut OpenPosition, price: f64, max_loss: Option<f64>, watched: bool) -> Option<(f64, &'static str)> {
    let step = management::close_step_with(p, price, 0, false, max_loss, Fill::Levels { watched });
    let exit = step.exit?;
    let usdt = if exit.reason == EXIT_LIQUIDATION {
        p.capital * pnl::liquidation_pnl_pct(p) / 100.0
    } else {
        position_usdt(p, exit.price)
    };
    Some((usdt, exit_code_position(exit.reason)))
}

/// A monotone leg from `a` to `b`, sampled every `SWEEP_STEP` (a gap jumps
/// straight to `b`, booking a crossed stop at the worse of stop and price).
fn sweep(p: &mut OpenPosition, a: f64, b: f64, max_loss: Option<f64>, gap: bool) -> Option<(f64, &'static str)> {
    if !(a > 0.0 && b > 0.0) || a == b {
        return None;
    }
    if gap {
        return eval(p, b, max_loss, false);
    }
    let n = ((b / a).ln().abs() / SWEEP_STEP).ceil().clamp(1.0, 20_000.0) as u32;
    for i in 1..=n {
        let px = if i == n { b } else { a * (b / a).powf(f64::from(i) / f64::from(n)) };
        if let Some(x) = eval(p, px, max_loss, true) {
            return Some(x);
        }
    }
    None
}

fn walk_position(pos: &OpenPosition, max_loss: Option<f64>, mark: f64, bars: &[Bar]) -> (f64, Option<&'static str>) {
    let now = position_usdt(pos, mark);
    let mut p = pos.clone();
    let exposure = if p.direction == Direction::Long { 1 } else { -1 };
    let mut prev = mark;
    for bar in bars {
        if let Some((usdt, why)) = sweep(&mut p, prev, bar.o, max_loss, true) {
            return (usdt - now, Some(why));
        }
        // The harmful extreme first: inside one bar the stop is checked
        // before the target, as the paper engine's protective rule does.
        let (_, path) = bar_paths(bar.o, bar.h, bar.l, bar.c, exposure);
        for k in 0..3 {
            if let Some((usdt, why)) = sweep(&mut p, path[k], path[k + 1], max_loss, false) {
                return (usdt - now, Some(why));
            }
        }
        prev = bar.c;
    }
    (position_usdt(&p, prev) - now, None)
}

fn walk_cycle(cycle: &CycleState, dd: Option<&DdStop>, mark: f64, bars: &[Bar]) -> (f64, Option<&'static str>) {
    let mut c = cycle.clone();
    c.core.events.clear();
    let now = c.mtm(mark);
    c.core.prev_close = Some(mark);
    let mut peak = dd.map(|d| d.peak_equity).unwrap_or(0.0);
    let mut last = mark;
    for bar in bars {
        if !c.is_open() {
            break;
        }
        c.core.dd_floor = dd.map(|d| peak - d.pct / 100.0 * d.budget - d.budget - d.realized);
        step_bar(&mut c, bar, false);
        last = bar.c;
        if let (true, Some(d)) = (c.is_open(), dd) {
            let eq = d.budget + d.realized + c.mtm(bar.c);
            peak = peak.max(eq);
            if peak - eq >= d.pct / 100.0 * d.budget {
                c.close_market(bar.c, ExitReason::Ddstop);
            }
        }
    }
    let end = if c.is_open() { c.mtm(last) } else { c.core.cash };
    (end - now, c.core.exit.map(exit_code_cycle))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::bot::engine::test_fixtures;
    use crate::bot::model::BotKind;
    use crate::bot::strategy::cycle::CycleStart;
    use crate::bot::strategy::costs::CostModel;
    use crate::bot::strategy::model::{DcaParams, GridParams, GridRange, Side, Spacing};

    /// A futures long at 100 with its stop at `sl`, capital 100, `lev`x.
    pub(crate) fn long_position(sl: f64, tp: f64, lev: f64) -> OpenPosition {
        let sig = test_fixtures::signal("long", 100.0, tp, sl);
        let mut p = test_fixtures::position(&sig, None);
        p.bot_kind = BotKind::Futures;
        p.effective_leverage = lev;
        p.leverage = lev as u8;
        p.notional_usdt = 100.0 * lev;
        p
    }

    pub(crate) fn pos_exposure(pos: OpenPosition, mark: f64) -> Exposure {
        Exposure {
            id: format!("pos:futures:{}", pos.signal_id),
            source: Source::Signal,
            label: "futures".into(),
            symbol: pos.symbol.clone(),
            market: MarketKind::Futures,
            live: false,
            scale: 1.0,
            direction: Dir::Long,
            capital: pos.capital,
            mark,
            body: Body::Position { pos: Box::new(pos), max_loss_pct: None },
        }
    }

    fn dca_params(sl_pct: Option<f64>) -> DcaParams {
        DcaParams {
            base_order: Some(100.0),
            safety_order: Some(100.0),
            base_weight: None,
            safety_weight: None,
            max_so: 3,
            so_step_pct: 5.0,
            step_scale: 1.0,
            volume_scale: 1.0,
            tp_pct: 2.0,
            trailing_pct: None,
            sl_pct,
            max_duration_min: None,
        }
    }

    /// A long DCA cycle opened at 100: base 100 + 3 safety orders of 100 at
    /// 95 / 90 / 85, budget 400 at `lev`x.
    pub(crate) fn dca_cycle(sl_pct: Option<f64>, lev: f64, market: MarketKind) -> CycleState {
        let st = CycleStart {
            bot_id: "sb_testtesttest",
            seq: 1,
            side: Side::Long,
            budget: 400.0 / lev,
            leverage: lev,
            size_factor: 1.0,
            costs: CostModel::for_market(market),
            max_duration_min: None,
        };
        let bar = shock_bar(100.0, 100.0, 0);
        let mut c = CycleState::open_dca(&st, &dca_params(sl_pct), &bar).unwrap();
        step_bar(&mut c, &bar, true);
        c
    }

    pub(crate) fn cycle_exposure(cycle: CycleState, mark: f64) -> Exposure {
        Exposure {
            id: "sb:sb_testtesttest".into(),
            source: Source::Dca,
            label: "DCA".into(),
            symbol: "SOLUSDT".into(),
            market: MarketKind::Futures,
            live: false,
            scale: 1.0,
            direction: Dir::Long,
            capital: cycle.core.budget,
            mark,
            body: Body::Cycle { cycle: Box::new(cycle), dd: None },
        }
    }

    #[test]
    fn a_stop_above_the_shock_caps_the_loss_at_the_stop_plus_fees() {
        // Long 100, stop 95, 1x, capital 100: -30% loses only to the stop.
        let e = pos_exposure(long_position(95.0, 110.0, 1.0), 100.0);
        let out = e.shock(1.0, -30.0, 0);
        assert_eq!(out.exit, Some("sl"));
        // -5% move, both taker fees (0.05% each side of the notional).
        let expected = 100.0 * (-5.0 - 2.0 * crate::bot::model::FUTURES_FEE_RATE * 100.0) / 100.0;
        let now = 100.0 * pnl::unrealized_net_pnl_pct(&long_position(95.0, 110.0, 1.0), 100.0) / 100.0;
        assert!((out.pnl - (expected - now)).abs() < 1e-9, "{out:?}");
        // A stop below the shock price does not fire: the loss is the move.
        let small = e.shock(1.0, -3.0, 0);
        assert_eq!(small.exit, None);
        assert!((small.pnl - (-3.0)).abs() < 1e-9, "{small:?}");
    }

    #[test]
    fn the_shock_goes_through_beta() {
        let e = pos_exposure(long_position(50.0, 200.0, 1.0), 100.0);
        // beta 2 at BTC -10% = -20% on the coin.
        assert!((e.shock(2.0, -10.0, 0).pnl - (-20.0)).abs() < 1e-9);
        // beta 0: the coin does not move.
        assert!(e.shock(0.0, -30.0, 0).pnl.abs() < 1e-12);
        assert!((shocked_price(100.0, 3.0, -40.0) - 1.0).abs() < 1e-12, "floored at 1% of the mark");
    }

    #[test]
    fn a_take_profit_on_the_way_caps_the_gain() {
        let e = pos_exposure(long_position(90.0, 105.0, 1.0), 100.0);
        let out = e.shock(1.0, 10.0, 0);
        assert_eq!(out.exit, Some("tp"));
        // +5% on 100 at 1x; the fees are already in the mark-to-market now.
        assert!((out.pnl - 5.0).abs() < 1e-9, "{out:?}");
    }

    #[test]
    fn liquidation_caps_the_loss_at_the_margin() {
        // 10x long, stop far below liquidation (~90.4): the margin is the cap.
        let e = pos_exposure(long_position(50.0, 120.0, 10.0), 100.0);
        let out = e.shock(1.0, -30.0, 0);
        assert_eq!(out.exit, Some("liq"));
        let now = 100.0 * pnl::unrealized_net_pnl_pct(&long_position(50.0, 120.0, 10.0), 100.0) / 100.0;
        assert!((out.pnl - (-100.0 - now)).abs() < 1e-9, "the whole margin (capital 100): {out:?}");
        // A 10x DCA without stop: liquidated, loss = its margin, never more.
        let c = dca_cycle(None, 10.0, MarketKind::Futures);
        let budget = c.core.budget;
        let out = cycle_exposure(c, 100.0).shock(1.0, -30.0, 0);
        assert_eq!(out.exit, Some("liq"));
        assert!(out.pnl >= -budget - 1e-6 && out.pnl < -0.5 * budget, "{out:?} budget {budget}");
    }

    #[test]
    fn a_dca_ladder_fills_on_the_way_down() {
        // Spot 1x, no stop: -12% fills SO1 (95) and SO2 (90), not SO3 (85).
        let c = dca_cycle(None, 1.0, MarketKind::Spot);
        let e = cycle_exposure(c.clone(), 100.0);
        let out = e.shock(1.0, -12.0, 0);
        assert_eq!(out.exit, None);
        // Filled 300 notional at about 100, 95, 90: qty ~ 1 + 1.0526 + 1.1111.
        let qty = 100.0 / 100.02 + 100.0 / 95.0 + 100.0 / 90.0;
        let mtm_at_88 = qty * 88.0 - 300.0;
        assert!((out.pnl - mtm_at_88).abs() < 1.5, "{out:?} vs {mtm_at_88}");
        // The base order alone would have lost only ~12.
        assert!(out.pnl < -18.0, "{out:?}");
        // -30%: the full ladder (400) is filled and nothing stops it.
        let deep = e.shock(1.0, -30.0, 0);
        let qty4 = qty + 100.0 / 85.0;
        assert!((deep.pnl - (qty4 * 70.0 - 400.0)).abs() < 2.0, "{deep:?}");
        assert_eq!(deep.exit, None);
    }

    #[test]
    fn a_dca_stop_loss_closes_the_ladder() {
        // SL 3% from the average (97): hit before SO1 at 95, base order only.
        let c = dca_cycle(Some(3.0), 1.0, MarketKind::Spot);
        let out = cycle_exposure(c, 100.0).shock(1.0, -30.0, 0);
        assert_eq!(out.exit, Some("sl"));
        assert!(out.pnl > -3.5 && out.pnl < -2.9, "{out:?}");
        // SL 8%: SO1 (95) and SO2 (90) fill, each moving the stop down
        // (avg ~94.8 -> stop ~87.2), and the stop closes before SO3 (85).
        let c = dca_cycle(Some(8.0), 1.0, MarketKind::Spot);
        let out = cycle_exposure(c, 100.0).shock(1.0, -30.0, 0);
        assert_eq!(out.exit, Some("sl"));
        assert!(out.pnl > -30.0 && out.pnl < -18.0, "{out:?}");
        let no_stop = cycle_exposure(dca_cycle(None, 1.0, MarketKind::Spot), 100.0).shock(1.0, -30.0, 0);
        assert!(no_stop.pnl < 3.0 * out.pnl, "the stop caps the loss: {no_stop:?}");
    }

    #[test]
    fn grid_buys_fill_on_the_way_down() {
        let st = CycleStart {
            bot_id: "sb_gridgridgrid",
            seq: 1,
            side: Side::Long,
            budget: 1000.0,
            leverage: 1.0,
            size_factor: 1.0,
            costs: CostModel::for_market(MarketKind::Spot),
            max_duration_min: None,
        };
        let p = GridParams {
            range: GridRange::Absolute { lower: 80.0, upper: 120.0 },
            n_grids: 8,
            spacing: Spacing::Arith,
            stop_out_pct: None,
            trailing_up: false,
            trail_up_limit: None,
            take_profit_pct: None,
            max_duration_min: None,
        };
        let bar = shock_bar(100.0, 100.0, 0);
        let mut c = CycleState::open_grid(&st, &p, &bar).unwrap();
        step_bar(&mut c, &bar, true);
        let before = c.signed_qty();
        let e = cycle_exposure(c.clone(), 100.0);
        let out = e.shock(1.0, -15.0, 0);
        // The cloned walk bought 85..95 (3 levels): a bigger loss than the
        // starting inventory alone would book.
        let inventory_only = before * (85.0 - 100.0);
        assert!(out.pnl < inventory_only - 1.0, "{out:?} vs {inventory_only}");
        // The live state is untouched.
        assert_eq!(c.signed_qty(), before);
    }

    #[test]
    fn a_live_scale_multiplies_the_result() {
        let mut e = pos_exposure(long_position(50.0, 200.0, 1.0), 100.0);
        e.scale = 0.25;
        assert!((e.shock(1.0, -10.0, 0).pnl - (-2.5)).abs() < 1e-9);
    }
}
