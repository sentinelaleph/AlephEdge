//! Pure strategy statistics with the research simulator's field set, over
//! closed cycles: cycles, win rate, mean / median cycle return, return per
//! day of capital, max drawdown, exits by reason, fees and funding % of
//! budget, mean safety orders filled, grid closing fills per cycle, and the
//! two utilisation figures.

use std::collections::BTreeMap;

use serde::Serialize;

use super::accounting;
use super::model::StrategyBot;

/// One closed cycle as the stats need it.
#[derive(Clone, Debug)]
pub struct ClosedCycle {
    pub pnl_quote: f64,
    pub budget: f64,
    pub exit: String,
    pub fees: f64,
    pub funding: f64,
    pub so_filled: Option<u32>,
    pub grid_closing_fills: Option<u32>,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StrategyStats {
    pub bots: u32,
    pub cycles: u32,
    /// Share of cycles with P&L > 0 (None without cycles).
    pub win_rate: Option<f64>,
    pub mean_cycle_return_pct: Option<f64>,
    pub median_cycle_return_pct: Option<f64>,
    /// Sum P&L / sum(budget x bot-days) x 100.
    pub return_per_day_of_capital_pct: Option<f64>,
    pub total_pnl_quote: f64,
    pub total_pnl_pct: f64,
    /// Worst bot's peak-to-trough drawdown, % of its budget.
    pub max_drawdown_pct: f64,
    pub exits: BTreeMap<String, u32>,
    pub fees_pct_of_budget: f64,
    pub funding_pct_of_budget: f64,
    pub safety_orders_filled_mean: Option<f64>,
    pub grid_closing_fills_per_cycle: Option<f64>,
    pub utilisation_in_position: f64,
    pub utilisation_committed: f64,
}

pub fn compute(bots: &[StrategyBot], cycles: &[ClosedCycle]) -> StrategyStats {
    let mut st = StrategyStats {
        bots: bots.len() as u32,
        cycles: cycles.len() as u32,
        ..Default::default()
    };
    let budget_sum: f64 = bots.iter().map(|b| b.cfg.budget).sum();
    let mut rets: Vec<f64> = cycles.iter().map(|c| c.pnl_quote / c.budget * 100.0).collect();
    if !cycles.is_empty() {
        let n = cycles.len() as f64;
        st.win_rate = Some(cycles.iter().filter(|c| c.pnl_quote > 0.0).count() as f64 / n);
        st.mean_cycle_return_pct = Some(rets.iter().sum::<f64>() / n);
        rets.sort_by(|a, b| a.total_cmp(b));
        let m = rets.len();
        st.median_cycle_return_pct = Some(if m % 2 == 1 {
            rets[m / 2]
        } else {
            (rets[m / 2 - 1] + rets[m / 2]) / 2.0
        });
    }
    st.total_pnl_quote = cycles.iter().map(|c| c.pnl_quote).sum();
    if budget_sum > 0.0 {
        st.total_pnl_pct = st.total_pnl_quote / budget_sum * 100.0;
        st.fees_pct_of_budget = cycles.iter().map(|c| c.fees).sum::<f64>() / budget_sum * 100.0;
        st.funding_pct_of_budget = cycles.iter().map(|c| c.funding).sum::<f64>() / budget_sum * 100.0;
    }
    // utilisation bars are minutes of life on the 1m feed
    let budget_days: f64 = bots.iter().map(|b| b.cfg.budget * b.util.bars / 1440.0).sum();
    if budget_days > 0.0 && !cycles.is_empty() {
        st.return_per_day_of_capital_pct = Some(st.total_pnl_quote / budget_days * 100.0);
    }
    st.max_drawdown_pct = bots
        .iter()
        .map(|b| b.max_dd_quote / b.cfg.budget * 100.0)
        .fold(0.0, f64::min);
    for c in cycles {
        *st.exits.entry(c.exit.clone()).or_insert(0) += 1;
    }
    let so: Vec<u32> = cycles.iter().filter_map(|c| c.so_filled).collect();
    if !so.is_empty() {
        st.safety_orders_filled_mean = Some(so.iter().sum::<u32>() as f64 / so.len() as f64);
    }
    let gc: Vec<u32> = cycles.iter().filter_map(|c| c.grid_closing_fills).collect();
    if !gc.is_empty() {
        st.grid_closing_fills_per_cycle = Some(gc.iter().sum::<u32>() as f64 / gc.len() as f64);
    }
    let ut: Vec<(f64, f64)> = bots
        .iter()
        .filter(|b| b.util.bars > 0.0)
        .map(|b| accounting::utilisation(&b.util, b.cfg.budget))
        .collect();
    if !ut.is_empty() {
        let n = ut.len() as f64;
        st.utilisation_in_position = ut.iter().map(|u| u.0).sum::<f64>() / n;
        st.utilisation_committed = ut.iter().map(|u| u.1).sum::<f64>() / n;
    }
    st
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::strategy::driver::tests::sample_bot;

    fn cyc(pnl: f64, exit: &str) -> ClosedCycle {
        ClosedCycle {
            pnl_quote: pnl,
            budget: 1000.0,
            exit: exit.into(),
            fees: 0.1,
            funding: 0.0,
            so_filled: Some(2),
            grid_closing_fills: None,
        }
    }

    #[test]
    fn botsim_field_set() {
        let mut b = sample_bot();
        b.util.bars = 1440.0 * 2.0;
        b.max_dd_quote = -50.0;
        let s = compute(&[b], &[cyc(20.0, "tp"), cyc(-10.0, "sl"), cyc(30.0, "tp")]);
        assert_eq!(s.cycles, 3);
        assert!((s.win_rate.unwrap() - 2.0 / 3.0).abs() < 1e-12);
        assert!((s.mean_cycle_return_pct.unwrap() - 4.0 / 3.0).abs() < 1e-12);
        assert!((s.median_cycle_return_pct.unwrap() - 2.0).abs() < 1e-12);
        assert!((s.return_per_day_of_capital_pct.unwrap() - 40.0 / 2000.0 * 100.0).abs() < 1e-12);
        assert_eq!(s.exits.get("tp"), Some(&2));
        assert!((s.max_drawdown_pct + 5.0).abs() < 1e-12);
        assert_eq!(s.safety_orders_filled_mean, Some(2.0));
        assert_eq!(s.grid_closing_fills_per_cycle, None);
        assert_eq!(compute(&[], &[]).win_rate, None);
    }
}
