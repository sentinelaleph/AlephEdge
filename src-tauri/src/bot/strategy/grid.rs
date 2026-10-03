//! Pure futures grid (Binance-style), a line-for-line port of the research
//! simulator's `Grid` class.
//!
//! Levels L_0..L_n; equal quantity per level q = budget * leverage / sum(L_1..L_n).
//! Below the start price: buys; above: sells; the level nearest the start is
//! empty. A buy filled at L_i places a sell at L_{i+1}, a sell at L_i a buy
//! at L_{i-1}. The whole budget is the isolated margin.

use serde::{Deserialize, Serialize};

use super::cycle::CycleCore;
use super::liquidation::grid_liq_price;
use super::model::{
    ExitReason, FillKind, GridParams, GridRange, Liquidity, OrderRole, OrderSide, OrderState,
    OrderType, Side, SimOrder, Spacing,
};

/// Range at a start price.
pub fn range_at(range: GridRange, start: f64) -> (f64, f64) {
    match range {
        GridRange::Absolute { lower, upper } => (lower, upper),
        GridRange::Relative {
            lower_pct,
            upper_pct,
        } => (start * (1.0 - lower_pct / 100.0), start * (1.0 + upper_pct / 100.0)),
    }
}

/// Price levels L_0..L_n.
pub fn levels(lower: f64, upper: f64, n: u16, spacing: Spacing) -> Vec<f64> {
    let nf = f64::from(n);
    match spacing {
        Spacing::Geom => {
            let r = (upper / lower).powf(1.0 / nf);
            (0..=i32::from(n)).map(|i| lower * r.powi(i)).collect()
        }
        Spacing::Arith => {
            let d = (upper - lower) / nf;
            (0..=n).map(|i| lower + d * f64::from(i)).collect()
        }
    }
}

/// Equal quantity per level: budget * leverage / sum(L_1..L_n).
pub fn qty_per_level(budget: f64, leverage: f64, levels: &[f64]) -> f64 {
    budget * leverage / levels.iter().skip(1).sum::<f64>()
}

/// Order side per level at the start price: +1 buy, -1 sell, 0 empty
/// (the level nearest the start, first one on a tie).
pub fn initial_orders(levels: &[f64], start: f64) -> Vec<i8> {
    let near = levels
        .iter()
        .enumerate()
        .fold((0usize, f64::INFINITY), |acc, (i, l)| {
            let d = (l - start).abs();
            if d < acc.1 {
                (i, d)
            } else {
                acc
            }
        })
        .0;
    levels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            if i == near {
                0
            } else if *l < start {
                1
            } else {
                -1
            }
        })
        .collect()
}

/// The counter order of a fill at level `i`: (level, side).
pub fn counter_order(i: usize, filled_side: i8, n: usize) -> Option<(usize, i8)> {
    if filled_side > 0 {
        if i < n {
            Some((i + 1, -1))
        } else {
            None
        }
    } else if i > 0 {
        Some((i - 1, 1))
    } else {
        None
    }
}

/// Stop band [lower*(1-x), upper*(1+x)]; the upper side is off with trailing up.
pub fn stop_band(lower: f64, upper: f64, stop_out_pct: Option<f64>, trailing_up: bool) -> (Option<f64>, Option<f64>) {
    match stop_out_pct.filter(|x| *x > 0.0) {
        Some(x) => (
            Some(lower * (1.0 - x / 100.0)),
            (!trailing_up).then_some(upper * (1.0 + x / 100.0)),
        ),
        None => (None, None),
    }
}

/// Profit per grid after maker fees on both sides, min..max over the levels.
pub fn profit_per_grid_pct(levels: &[f64], maker: f64) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for w in levels.windows(2) {
        let p = (w[1] / w[0] - 1.0 - 2.0 * maker) * 100.0;
        lo = lo.min(p);
        hi = hi.max(p);
    }
    (lo, hi)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GridState {
    pub levels: Vec<f64>,
    /// +1 buy, -1 sell, 0 none.
    pub side: Vec<i8>,
    pub qty: Vec<f64>,
    /// Per-level order number (client id) and the leg it was placed on.
    pub order_no: Vec<u32>,
    pub placed_leg: Vec<u32>,
    pub next_order_no: u32,
    pub n: usize,
    pub geom: bool,
    pub r: f64,
    pub d: f64,
    pub x: Option<f64>,
    pub trailing_up: bool,
    pub trail_limit: Option<f64>,
    pub lo_stop: Option<f64>,
    pub hi_stop: Option<f64>,
    pub tp_total: Option<f64>,
    pub lower: f64,
    pub upper: f64,
    pub pos: f64,
    pub a: f64,
    pub closes: u32,
    pub shifts: u32,
    pub liq: Option<f64>,
}

/// Opens the grid at `start` (bar open). Long: market-buys one q per sell
/// level; short: market-sells one q per buy level; neutral starts flat.
pub fn open(core: &mut CycleCore, p: &GridParams, start: f64) -> Result<GridState, &'static str> {
    let (lower, upper) = range_at(p.range, start);
    let n = p.n_grids;
    if !(0.0 < lower && lower < upper) || n < 1 {
        return Err("gridBadRange");
    }
    let lv = levels(lower, upper, n, p.spacing);
    let nn = usize::from(n);
    let x = p.stop_out_pct.filter(|v| *v > 0.0);
    let (lo_stop, hi_stop) = stop_band(lower, upper, x, p.trailing_up);
    if lo_stop.is_some_and(|s| start <= s) || hi_stop.is_some_and(|s| start >= s) {
        return Err("gridStartOutsideBand");
    }
    let q = qty_per_level(core.budget, core.leverage, &lv);
    let side = initial_orders(&lv, start);
    let order_no: Vec<u32> = (0..=nn as u32).collect();
    let mut g = GridState {
        levels: lv,
        side,
        qty: vec![q; nn + 1],
        order_no,
        placed_leg: vec![0; nn + 1],
        next_order_no: nn as u32 + 1,
        n: nn,
        geom: p.spacing == Spacing::Geom,
        r: (upper / lower).powf(1.0 / f64::from(n)),
        d: (upper - lower) / f64::from(n),
        x,
        trailing_up: p.trailing_up,
        trail_limit: p.trail_up_limit.filter(|v| *v > 0.0),
        lo_stop,
        hi_stop,
        tp_total: p.take_profit_pct.filter(|v| *v > 0.0),
        lower,
        upper,
        pos: 0.0,
        a: 0.0,
        closes: 0,
        shifts: 0,
        liq: None,
    };
    match core.side {
        Side::Long => {
            let nsell = g.side.iter().filter(|z| **z == -1).count();
            if nsell > 0 {
                let px = start * (1.0 + core.costs.slippage);
                g.trade(core, q * nsell as f64, px, Liquidity::Taker, "base");
            }
        }
        Side::Short => {
            let nbuy = g.side.iter().filter(|z| **z == 1).count();
            if nbuy > 0 {
                let px = start * (1.0 - core.costs.slippage);
                g.trade(core, -q * nbuy as f64, px, Liquidity::Taker, "base");
            }
        }
        Side::Neutral => {}
    }
    core.fills = 0;
    Ok(g)
}

impl GridState {
    pub fn exposure(&self) -> i8 {
        if self.pos > 1e-15 {
            1
        } else if self.pos < -1e-15 {
            -1
        } else {
            0
        }
    }

    pub fn margin(&self, leverage: f64) -> f64 {
        self.pos.abs() * self.a / leverage
    }

    pub fn unreal(&self, p: f64) -> f64 {
        self.pos * (p - self.a)
    }

    /// The bot drawdown stop as a price for the inventory held now: the price
    /// at which cash + pos * (p - a) reaches the driver's floor. Same rule as
    /// the DCA walk: it fills inside the bar at its level, not at the close.
    fn dd_price(&self, core: &CycleCore) -> Option<f64> {
        let floor = core.dd_floor?;
        if self.pos == 0.0 {
            return None;
        }
        let px = self.a + (floor - core.cash) / self.pos;
        (px.is_finite() && px > 0.0).then_some(px)
    }

    /// Signed trade `dq` at `px`; realises P&L on the closing part.
    fn trade(&mut self, core: &mut CycleCore, dq: f64, px: f64, liq: Liquidity, role: &str) {
        let pos = self.pos;
        let mut realized = 0.0;
        if pos == 0.0 || (pos > 0.0) == (dq > 0.0) {
            self.a = (self.a * pos.abs() + px * dq.abs()) / (pos.abs() + dq.abs());
            self.pos = pos + dq;
        } else {
            let closing = dq.abs().min(pos.abs());
            realized = closing * (px - self.a) * if pos > 0.0 { 1.0 } else { -1.0 };
            core.cash += realized;
            self.closes += 1;
            self.pos = pos + dq;
            if self.pos.abs() < 1e-12 * pos.abs().max(1.0) {
                self.pos = 0.0;
                self.a = 0.0;
            } else if (self.pos > 0.0) != (pos > 0.0) {
                self.a = px;
            }
        }
        let fee = dq.abs() * px * core.costs.rate(liq);
        core.cash -= fee;
        core.fees += fee;
        core.fills += 1;
        core.push_fill(role, px, dq, liq, fee, realized, FillKind::Fill);
        self.recompute(core);
    }

    pub fn recompute(&mut self, core: &CycleCore) {
        self.liq = grid_liq_price(self.pos, self.a, core.budget + core.cash, core.costs.mmr);
    }

    fn next_top(&self) -> f64 {
        let top = self.levels[self.n];
        if self.geom {
            top * self.r
        } else {
            top + self.d
        }
    }

    fn liquidate(&mut self, core: &mut CycleCore, px: f64) {
        let lost = -core.budget - core.cash;
        core.push_fill("liq", px, -self.pos, Liquidity::Taker, 0.0, lost, FillKind::Liquidation);
        core.cash = -core.budget;
        self.pos = 0.0;
        self.a = 0.0;
        core.close_as(ExitReason::Liq);
    }

    fn place(&mut self, i: usize, side: i8, qty: f64, leg: u32) {
        self.side[i] = side;
        self.qty[i] = qty;
        self.order_no[i] = self.next_order_no;
        self.next_order_no += 1;
        self.placed_leg[i] = leg;
    }

    fn down(&mut self, core: &mut CycleCore, a: f64, b: f64, gap: bool) {
        let mut cur = a;
        while core.open {
            // (key, priority, kind, payload): higher key = reached first on a
            // falling leg; exact ties: liquidation (0) > stop (-1) > fill (-2).
            let mut best: Option<(f64, i32, u8, f64)> = None;
            for i in (0..=self.n).rev() {
                if self.side[i] == 1 && self.levels[i] >= b {
                    best = Some((self.levels[i].min(cur), -2, 0, i as f64));
                    break;
                }
            }
            let liq_lv = if self.pos > 0.0 { self.liq } else { None };
            let dd_lv = if self.pos > 0.0 { self.dd_price(core) } else { None };
            for (lv, pr, kd) in [(self.lo_stop, 1, 1u8), (liq_lv, 0, 2u8), (dd_lv, 1, 4u8)] {
                if let Some(lv) = lv {
                    if lv >= b {
                        let cand = (lv.min(cur), -pr, kd, lv);
                        if best.map_or(true, |bb| cand.0 > bb.0 || (cand.0 == bb.0 && cand.1 > bb.1)) {
                            best = Some(cand);
                        }
                    }
                }
            }
            let Some((key, _, kd, z)) = best else {
                return;
            };
            match kd {
                0 => {
                    let i = z as usize;
                    let qty = self.qty[i];
                    let no = self.order_no[i];
                    self.trade(core, qty, self.levels[i], Liquidity::Maker, &format!("b{no}"));
                    self.side[i] = 0;
                    if let Some((j, s)) = counter_order(i, 1, self.n) {
                        self.place(j, s, qty, core.legs);
                    }
                    cur = key;
                }
                2 => self.liquidate(core, z),
                4 => close_market(core, self, if gap { b } else { key }, ExitReason::Ddstop),
                _ => close_market(core, self, if gap { b } else { key }, ExitReason::Stop),
            }
        }
    }

    fn up(&mut self, core: &mut CycleCore, a: f64, b: f64, gap: bool) {
        let mut cur = a;
        while core.open {
            // key = -max(level, cur): higher = reached first on a rising leg.
            let mut best: Option<(f64, i32, u8, f64)> = None;
            for i in 0..=self.n {
                if self.side[i] == -1 && self.levels[i] <= b {
                    best = Some((-self.levels[i].max(cur), -2, 0, i as f64));
                    break;
                }
            }
            let mut tl = self.trailing_up.then(|| self.next_top());
            if let (Some(t), Some(limit)) = (tl, self.trail_limit) {
                if t > limit {
                    tl = None;
                }
            }
            let liq_lv = if self.pos < 0.0 { self.liq } else { None };
            let dd_lv = if self.pos < 0.0 { self.dd_price(core) } else { None };
            for (lv, pr, kd) in [(self.hi_stop, 1, 1u8), (liq_lv, 0, 2u8), (tl, 3, 3u8), (dd_lv, 1, 4u8)] {
                if let Some(lv) = lv {
                    if lv <= b {
                        let cand = (-lv.max(cur), -pr, kd, lv);
                        if best.map_or(true, |bb| cand.0 > bb.0 || (cand.0 == bb.0 && cand.1 > bb.1)) {
                            best = Some(cand);
                        }
                    }
                }
            }
            let Some((neg_key, _, kd, z)) = best else {
                return;
            };
            let key = -neg_key;
            match kd {
                0 => {
                    let i = z as usize;
                    let qty = self.qty[i];
                    let no = self.order_no[i];
                    self.trade(core, -qty, self.levels[i], Liquidity::Maker, &format!("s{no}"));
                    self.side[i] = 0;
                    if let Some((j, s)) = counter_order(i, -1, self.n) {
                        self.place(j, s, qty, core.legs);
                    }
                    cur = key;
                }
                2 => self.liquidate(core, z),
                3 => {
                    self.shift_up(z, core.legs);
                    cur = key;
                }
                4 => close_market(core, self, if gap { b } else { key }, ExitReason::Ddstop),
                _ => close_market(core, self, if gap { b } else { key }, ExitReason::Stop),
            }
        }
    }

    /// Trailing up: the bottom buy is cancelled and its notional re-placed as
    /// a buy at the old top; the grid moves one level up.
    fn shift_up(&mut self, new_top: f64, leg: u32) {
        let old_top = self.levels[self.n];
        let freed = if self.side[0] == 1 {
            self.qty[0] * self.levels[0]
        } else {
            0.0
        };
        self.levels.remove(0);
        self.levels.push(new_top);
        self.side.remove(0);
        self.side.push(0);
        let last_q = self.qty[self.n];
        self.qty.remove(0);
        self.qty.push(last_q);
        self.order_no.remove(0);
        self.order_no.push(0);
        self.placed_leg.remove(0);
        self.placed_leg.push(0);
        if self.side[self.n - 1] == 0 && freed > 0.0 {
            self.place(self.n - 1, 1, freed / old_top, leg);
        }
        if let Some(x) = self.x {
            self.lo_stop = Some(self.levels[0] * (1.0 - x / 100.0));
        }
        self.lower = self.levels[0];
        self.upper = self.levels[self.n];
        self.shifts += 1;
    }

    pub fn open_orders(&self, core: &CycleCore) -> Vec<SimOrder> {
        let prefix = core.client_prefix();
        (0..=self.n)
            .filter(|i| self.side[*i] != 0)
            .map(|i| {
                let buy = self.side[i] > 0;
                SimOrder {
                    client_id: format!("{prefix}{}{}", if buy { "b" } else { "s" }, self.order_no[i]),
                    role: if buy {
                        OrderRole::GridBuy(i as u16)
                    } else {
                        OrderRole::GridSell(i as u16)
                    },
                    side: if buy { OrderSide::Buy } else { OrderSide::Sell },
                    kind: OrderType::Limit,
                    price: self.levels[i],
                    qty: self.qty[i],
                    active_from_leg: self.placed_leg[i] + 1,
                    state: OrderState::Open,
                }
            })
            .collect()
    }

    /// No stop-out and price outside [lower, upper]: no fill is possible.
    pub fn out_of_range(&self, px: f64) -> bool {
        self.x.is_none() && (px < self.lower || px > self.upper)
    }
}

/// One monotone leg a -> b.
pub fn seg(core: &mut CycleCore, g: &mut GridState, a: f64, b: f64, gap: bool) {
    if b <= a {
        g.down(core, a, b, gap);
    }
    if core.open && b >= a {
        g.up(core, a, b, gap);
    }
}

/// Market close of the whole inventory at `ref_px` (+ adverse slippage).
pub fn close_market(core: &mut CycleCore, g: &mut GridState, ref_px: f64, reason: ExitReason) {
    if g.pos != 0.0 {
        let px = if g.pos > 0.0 {
            ref_px * (1.0 - core.costs.slippage)
        } else {
            ref_px * (1.0 + core.costs.slippage)
        };
        let dq = -g.pos;
        g.trade(core, dq, px, Liquidity::Taker, "close");
    }
    core.close_as(reason);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arith_and_geom_levels() {
        assert_eq!(levels(90.0, 110.0, 2, Spacing::Arith), vec![90.0, 100.0, 110.0]);
        let g = levels(100.0, 400.0, 2, Spacing::Geom);
        assert!((g[1] - 200.0).abs() < 1e-9 && (g[2] - 400.0).abs() < 1e-9);
    }

    #[test]
    fn quantity_and_initial_orders() {
        let lv = levels(90.0, 110.0, 2, Spacing::Arith);
        assert!((qty_per_level(1000.0, 1.0, &lv) - 1000.0 / 210.0).abs() < 1e-12);
        assert_eq!(initial_orders(&lv, 100.0), vec![1, 0, -1]);
        assert_eq!(initial_orders(&lv, 104.0), vec![1, 0, -1]);
        assert_eq!(initial_orders(&lv, 106.0), vec![1, 1, 0]);
    }

    #[test]
    fn counter_orders_step_one_level() {
        assert_eq!(counter_order(0, 1, 2), Some((1, -1)));
        assert_eq!(counter_order(2, 1, 2), None);
        assert_eq!(counter_order(2, -1, 2), Some((1, 1)));
        assert_eq!(counter_order(0, -1, 2), None);
    }

    #[test]
    fn profit_per_grid_after_fees() {
        let lv = levels(90.0, 110.0, 2, Spacing::Arith);
        let (lo, hi) = profit_per_grid_pct(&lv, 0.0002);
        assert!((lo - (110.0 / 100.0 - 1.0 - 0.0004) * 100.0).abs() < 1e-9);
        assert!((hi - (100.0 / 90.0 - 1.0 - 0.0004) * 100.0).abs() < 1e-9);
    }

    #[test]
    fn stop_band_respects_trailing_up() {
        let (lo, hi) = stop_band(90.0, 110.0, Some(5.0), false);
        assert!((lo.unwrap() - 85.5).abs() < 1e-9 && (hi.unwrap() - 115.5).abs() < 1e-9);
        assert_eq!(stop_band(90.0, 110.0, Some(5.0), true), (Some(85.5), None));
        assert_eq!(stop_band(90.0, 110.0, None, false), (None, None));
    }
}
