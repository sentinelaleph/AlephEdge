//! Pure DCA engine (3Commas-style), a line-for-line port of the research
//! simulator's `Dca` class so presets behave identically in the app.
//!
//! The ladder is anchored to the START BAR OPEN, not the base fill (a
//! documented divergence from 3Commas). Safety orders and the take profit
//! are resting maker limits; base order, stop loss, trailing exit, timeout
//! and manual close are market orders (taker + slippage).

use serde::{Deserialize, Serialize};

use super::cycle::CycleCore;
use super::liquidation::dca_liq_price;
use super::model::{
    DcaParams, ExitReason, FillKind, Liquidity, OrderRole, OrderSide, OrderState, OrderType,
    SimOrder,
};

/// Safety-order deviations from the anchor, cumulative, %:
/// d_k = so_step_pct * sum_{j<k} step_scale^j.
pub fn deviations(p: &DcaParams) -> Vec<f64> {
    let mut out = Vec::with_capacity(usize::from(p.max_so));
    let mut d = 0.0;
    for i in 0..i32::from(p.max_so) {
        d += p.so_step_pct * p.step_scale.powi(i);
        out.push(d);
    }
    out
}

/// (base notional, safety notionals) for a cycle with margin pool `budget`
/// (already size-scaled) at `leverage`; `scale` applies to the explicit
/// notional form (the weight form is already scaled through `budget`).
pub fn ladder_notionals(
    p: &DcaParams,
    budget: f64,
    leverage: f64,
    scale: f64,
) -> (f64, Vec<f64>) {
    let max_so = i32::from(p.max_so);
    let (bo, so) = match (p.base_order, p.safety_order) {
        (Some(bo), so) => (bo * scale, so.unwrap_or(0.0) * scale),
        (None, _) => {
            let bw = p.base_weight.unwrap_or(1.0);
            let sw = p.safety_weight.unwrap_or(1.0);
            let tot = bw + sw * (0..max_so).map(|i| p.volume_scale.powi(i)).sum::<f64>();
            let unit = budget * leverage / tot;
            (bw * unit, sw * unit)
        }
    };
    let sos = (0..max_so).map(|i| so * p.volume_scale.powi(i)).collect();
    (bo, sos)
}

/// Margin the full ladder reserves.
pub fn required_capital(base: f64, safeties: &[f64], leverage: f64) -> f64 {
    (base + safeties.iter().sum::<f64>()) / leverage
}

pub fn avg_entry(notional: f64, qty: f64) -> f64 {
    notional / qty
}

pub fn tp_price(avg: f64, tp_pct: f64, s: f64) -> f64 {
    avg * (1.0 + s * tp_pct / 100.0)
}

pub fn sl_price(avg: f64, sl_pct: f64, s: f64) -> f64 {
    avg * (1.0 - s * sl_pct / 100.0)
}

/// One rung of the ladder as previewed (base filled at the anchor + slippage).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LadderRung {
    /// 0 = base order, k = safety order k.
    pub index: u8,
    pub deviation_pct: f64,
    pub price: f64,
    pub notional: f64,
    pub qty: f64,
    pub cum_notional: f64,
    pub cum_qty: f64,
    pub avg_entry: f64,
    pub tp_price: f64,
    pub liq_price: Option<f64>,
}

/// The full ladder for a cycle opening at `anchor`.
pub fn build_ladder(
    p: &DcaParams,
    anchor: f64,
    s: f64,
    budget: f64,
    leverage: f64,
    slippage: f64,
    mmr: f64,
) -> Vec<LadderRung> {
    let (bo, sos) = ladder_notionals(p, budget, leverage, 1.0);
    let devs = deviations(p);
    let base_px = anchor * (1.0 + s * slippage);
    let mut rungs = Vec::with_capacity(sos.len() + 1);
    let (mut cn, mut cq) = (0.0, 0.0);
    let mut push = |index: u8, dev: f64, price: f64, notional: f64| {
        let qty = notional / price;
        cn += notional;
        cq += qty;
        let avg = avg_entry(cn, cq);
        rungs.push(LadderRung {
            index,
            deviation_pct: dev,
            price,
            notional,
            qty,
            cum_notional: cn,
            cum_qty: cq,
            avg_entry: avg,
            tp_price: tp_price(avg, p.tp_pct, s),
            liq_price: dca_liq_price(cn, cn / leverage, cq, s, mmr),
        });
    };
    push(0, 0.0, base_px, bo);
    for (k, (nt, dev)) in sos.iter().zip(devs.iter()).enumerate() {
        push(k as u8 + 1, *dev, anchor * (1.0 - s * dev / 100.0), *nt);
    }
    rungs
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DcaState {
    /// +1 long, -1 short.
    pub s: f64,
    pub q: f64,
    pub n: f64,
    pub m: f64,
    pub a: f64,
    pub tp: f64,
    pub sl: Option<f64>,
    pub liq: Option<f64>,
    pub so_px: Vec<f64>,
    pub so_not: Vec<f64>,
    pub max_so: u8,
    pub nso: u8,
    pub tp_pct: f64,
    pub tr_pct: Option<f64>,
    pub sl_pct: Option<f64>,
    pub committed: f64,
    pub trail_on: bool,
    pub peak: Option<f64>,
    pub trail_px: Option<f64>,
    /// Leg on which the TP/SL were last re-placed (active from the next).
    pub tp_leg: u32,
}

/// Opens the deal: ladder at the anchor, base order at market.
pub fn open(core: &mut CycleCore, p: &DcaParams, anchor: f64) -> Result<DcaState, &'static str> {
    let s = match core.side {
        super::model::Side::Long => 1.0,
        super::model::Side::Short => -1.0,
        super::model::Side::Neutral => return Err("neutralDca"),
    };
    let lev = core.leverage;
    let (bo, so_not) = ladder_notionals(p, core.budget, lev, core.size_factor);
    let need = required_capital(bo, &so_not, lev);
    if need > core.budget * (1.0 + 1e-9) {
        return Err("ladderExceedsBudget");
    }
    let devs = deviations(p);
    if s > 0.0 && devs.last().is_some_and(|d| *d >= 100.0) {
        return Err("ladderBelowZero");
    }
    let so_px = devs.iter().map(|x| anchor * (1.0 - s * x / 100.0)).collect();
    let pf = core.costs.market_fill_price(anchor, s > 0.0);
    let mut d = DcaState {
        s,
        q: bo / pf,
        n: bo,
        m: bo / lev,
        a: 0.0,
        tp: 0.0,
        sl: None,
        liq: None,
        so_px,
        so_not,
        max_so: p.max_so,
        nso: 0,
        tp_pct: p.tp_pct,
        tr_pct: p.trailing_pct.filter(|v| *v > 0.0),
        sl_pct: p.sl_pct.filter(|v| *v > 0.0),
        committed: need,
        trail_on: false,
        peak: None,
        trail_px: None,
        tp_leg: 0,
    };
    let fee = core.costs.fee(bo, Liquidity::Taker);
    core.cash -= fee;
    core.fees += fee;
    core.fills = 1;
    core.push_fill("base", pf, s * d.q, Liquidity::Taker, fee, 0.0, FillKind::Fill);
    d.recompute(core.costs.mmr, 0);
    Ok(d)
}

impl DcaState {
    pub fn recompute(&mut self, mmr: f64, leg: u32) {
        let s = self.s;
        self.a = avg_entry(self.n, self.q);
        self.tp = tp_price(self.a, self.tp_pct, s);
        self.sl = self.sl_pct.map(|p| sl_price(self.a, p, s));
        self.liq = dca_liq_price(self.n, self.m, self.q, s, mmr);
        self.tp_leg = leg;
    }

    pub fn unreal(&self, p: f64) -> f64 {
        self.s * (self.q * p - self.n)
    }

    fn entry_side(&self) -> OrderSide {
        if self.s > 0.0 {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        }
    }

    fn exit_side(&self) -> OrderSide {
        if self.s > 0.0 {
            OrderSide::Sell
        } else {
            OrderSide::Buy
        }
    }

    /// Resting orders: remaining safety orders, the TP (absent while a
    /// trailing exit is armed) and the stop.
    pub fn open_orders(&self, core: &CycleCore) -> Vec<SimOrder> {
        let prefix = core.client_prefix();
        let mut out = Vec::new();
        for k in usize::from(self.nso)..usize::from(self.max_so) {
            let px = self.so_px[k];
            out.push(SimOrder {
                client_id: format!("{prefix}so{}", k + 1),
                role: OrderRole::Safety(k as u8 + 1),
                side: self.entry_side(),
                kind: OrderType::Limit,
                price: px,
                qty: self.so_not[k] / px,
                active_from_leg: 0,
                state: OrderState::Open,
            });
        }
        if !self.trail_on {
            out.push(SimOrder {
                client_id: format!("{prefix}tp{}", self.nso),
                role: OrderRole::TakeProfit,
                side: self.exit_side(),
                kind: OrderType::Limit,
                price: self.tp,
                qty: self.q,
                active_from_leg: self.tp_leg + 1,
                state: OrderState::Open,
            });
        }
        if let Some(sl) = self.sl {
            out.push(SimOrder {
                client_id: format!("{prefix}sl{}", self.nso),
                role: OrderRole::StopLoss,
                side: self.exit_side(),
                kind: OrderType::StopMarket,
                price: sl,
                qty: self.q,
                active_from_leg: self.tp_leg + 1,
                state: OrderState::Open,
            });
        }
        out
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Trigger {
    So,
    Liq,
    Sl,
    Dd,
    Trail,
}

/// One monotone leg a -> b.
pub fn seg(core: &mut CycleCore, d: &mut DcaState, a: f64, b: f64, gap: bool) {
    let s = d.s;
    if s * (b - a) <= 0.0 {
        adverse(core, d, a, b, gap);
    }
    if core.open && s * (b - a) >= 0.0 {
        favour(core, d, b);
    }
}

fn adverse(core: &mut CycleCore, d: &mut DcaState, a: f64, b: f64, gap: bool) {
    let s = d.s;
    let mut cur = a;
    while core.open {
        let sb = s * b;
        let mut cands: Vec<(f64, i32, Trigger)> = Vec::with_capacity(4);
        if d.nso < d.max_so && s * d.so_px[usize::from(d.nso)] >= sb {
            cands.push((d.so_px[usize::from(d.nso)], 1, Trigger::So));
        }
        if let Some(liq) = d.liq {
            if s * liq >= sb {
                cands.push((liq, 0, Trigger::Liq));
            }
        }
        if let Some(sl) = d.sl {
            if s * sl >= sb {
                cands.push((sl, 2, Trigger::Sl));
            }
        }
        // The bot drawdown stop as a price: cash + s*(q*p - n) = floor.
        if let Some(floor) = core.dd_floor {
            if d.q > 0.0 {
                let px = (d.n + s * (floor - core.cash)) / d.q;
                if px.is_finite() && s * px >= sb {
                    cands.push((px, 2, Trigger::Dd));
                }
            }
        }
        if d.trail_on {
            if let Some(tp) = d.trail_px {
                if s * tp >= sb {
                    cands.push((tp, 3, Trigger::Trail));
                }
            }
        }
        // Reached first = largest s*min(lv, cur); exact ties: liq > so > sl > trail
        // by the simulator's priority numbers (lower wins).
        let mut best: Option<((f64, i32), f64, Trigger)> = None;
        for (lv, pr, kd) in cands {
            let key = ((s * lv).min(s * cur), -pr);
            let better = match &best {
                None => true,
                Some((bk, _, _)) => key.0 > bk.0 || (key.0 == bk.0 && key.1 > bk.1),
            };
            if better {
                best = Some((key, lv, kd));
            }
        }
        let Some((_, lv, kd)) = best else {
            return;
        };
        match kd {
            Trigger::So => {
                let k = usize::from(d.nso);
                let nt = d.so_not[k];
                let qty = nt / lv;
                d.q += qty;
                d.n += nt;
                d.m += nt / core.leverage;
                let fee = core.costs.fee(nt, Liquidity::Maker);
                core.cash -= fee;
                core.fees += fee;
                d.nso += 1;
                core.fills += 1;
                core.push_fill(
                    &format!("so{}", k + 1),
                    lv,
                    s * qty,
                    Liquidity::Maker,
                    fee,
                    0.0,
                    FillKind::Fill,
                );
                d.recompute(core.costs.mmr, core.legs);
                if d.m > core.max_margin {
                    core.max_margin = d.m;
                }
                if s * lv < s * cur {
                    cur = lv;
                }
            }
            Trigger::Liq => {
                core.cash -= d.m;
                core.push_fill(
                    "liq",
                    lv,
                    -s * d.q,
                    Liquidity::Taker,
                    0.0,
                    -d.m,
                    FillKind::Liquidation,
                );
                core.close_as(ExitReason::Liq);
            }
            Trigger::Sl | Trigger::Dd | Trigger::Trail => {
                let px = if gap {
                    b
                } else if s * lv <= s * cur {
                    lv
                } else {
                    cur
                };
                let reason = match kd {
                    Trigger::Sl => ExitReason::Sl,
                    Trigger::Dd => ExitReason::Ddstop,
                    _ => ExitReason::Trail,
                };
                close_market(core, d, px, reason);
            }
        }
    }
}

fn favour(core: &mut CycleCore, d: &mut DcaState, b: f64) {
    let s = d.s;
    if d.trail_on {
        if d.peak.is_some_and(|p| s * b > s * p) {
            d.peak = Some(b);
            d.trail_px = d.tr_pct.map(|t| b * (1.0 - s * t / 100.0));
        }
        return;
    }
    if s * b >= s * d.tp {
        if let Some(t) = d.tr_pct {
            d.trail_on = true;
            d.peak = Some(b);
            d.trail_px = Some(b * (1.0 - s * t / 100.0));
        } else {
            let px = d.tp;
            let fee = core.costs.fee(d.q * px, Liquidity::Maker);
            let realized = s * (d.q * px - d.n);
            core.cash += realized - fee;
            core.fees += fee;
            core.push_fill(
                &format!("tp{}", d.nso),
                px,
                -s * d.q,
                Liquidity::Maker,
                fee,
                realized,
                FillKind::Fill,
            );
            core.close_as(ExitReason::Tp);
        }
    }
}

/// Market exit of the whole position at `ref_px` (+ adverse slippage).
pub fn close_market(core: &mut CycleCore, d: &mut DcaState, ref_px: f64, reason: ExitReason) {
    let s = d.s;
    // closing a long sells, closing a short buys
    let px = core.costs.market_fill_price(ref_px, s < 0.0);
    let fee = core.costs.fee(d.q * px, Liquidity::Taker);
    let realized = s * (d.q * px - d.n);
    core.cash += realized - fee;
    core.fees += fee;
    // The stop fill carries the resting stop order's id ("sl{nso}"), so the
    // paper book marks that order Filled instead of Canceled.
    let role = match reason {
        ExitReason::Sl => format!("sl{}", d.nso),
        ExitReason::Trail => "trail".to_string(),
        _ => "close".to_string(),
    };
    core.push_fill(&role, px, -s * d.q, Liquidity::Taker, fee, realized, FillKind::Fill);
    core.close_as(reason);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset() -> DcaParams {
        DcaParams {
            base_order: None,
            safety_order: None,
            base_weight: Some(1.0),
            safety_weight: Some(1.0),
            max_so: 8,
            so_step_pct: 2.5,
            step_scale: 1.3,
            volume_scale: 1.4,
            tp_pct: 2.0,
            trailing_pct: None,
            sl_pct: None,
            max_duration_min: None,
        }
    }

    #[test]
    fn classic_preset_ladder_matches_research_table() {
        let p = preset();
        let devs = deviations(&p);
        // research table, rounded to 2 decimals
        let want = [2.5, 5.75, 9.98, 15.47, 22.61, 31.89, 43.96, 59.65];
        for (g, w) in devs.iter().zip(want) {
            assert!((g - w).abs() <= 0.01, "{g} vs {w}");
        }
        let (bo, sos) = ladder_notionals(&p, 1000.0, 1.0, 1.0);
        assert!((bo - 28.25).abs() < 0.01, "{bo}");
        let want = [28.25, 39.55, 55.37, 77.52, 108.53, 151.94, 212.72, 297.80];
        for (g, w) in sos.iter().zip(want) {
            assert!((g - w).abs() < 0.03, "{g} vs {w}");
        }
        assert!((required_capital(bo, &sos, 1.0) - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn ladder_average_and_tp_are_cumulative() {
        let p = DcaParams {
            base_order: Some(100.0),
            safety_order: Some(100.0),
            base_weight: None,
            safety_weight: None,
            max_so: 2,
            so_step_pct: 2.0,
            step_scale: 1.0,
            volume_scale: 1.0,
            tp_pct: 2.0,
            trailing_pct: None,
            sl_pct: None,
            max_duration_min: None,
        };
        let rungs = build_ladder(&p, 100.0, 1.0, 1000.0, 1.0, 0.0002, 0.005);
        assert_eq!(rungs.len(), 3);
        assert!((rungs[1].price - 98.0).abs() < 1e-12);
        assert!((rungs[2].price - 96.0).abs() < 1e-12);
        let q = 100.0 / 100.02 + 100.0 / 98.0;
        assert!((rungs[1].avg_entry - 200.0 / q).abs() < 1e-9);
        assert!((rungs[1].tp_price - 200.0 / q * 1.02).abs() < 1e-9);
        assert_eq!(rungs[2].liq_price, None);
    }
}
