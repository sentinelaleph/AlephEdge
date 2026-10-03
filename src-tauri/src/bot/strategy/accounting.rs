//! Pure bot accounting, identical to the research simulator's definitions:
//! simple P&L in % of budget (no compounding), size-down after losses, dead
//! below 1% of budget, peak-to-trough drawdown on bar-close MTM, and two
//! time-averaged utilisation figures.

use super::model::{StrategyBot, Utilisation};

/// Later cycles are sized by (budget + P&L) / budget, never up.
pub fn size_down(budget: f64, realized: f64) -> f64 {
    if budget <= 0.0 {
        return 0.0;
    }
    ((budget + realized) / budget).min(1.0)
}

/// A bot with less than 1% of its budget left stops.
pub fn is_dead(size_factor: f64) -> bool {
    size_factor < 0.01
}

/// Funding charged on a position: `signed_qty * bar_open * rate`; positive =
/// paid (a long pays a positive rate, a short receives it).
pub fn funding(signed_qty: f64, bar_open: f64, rate: f64) -> f64 {
    signed_qty * bar_open * rate
}

/// Bot equity = budget + closed P&L + open cycle MTM.
pub fn equity(budget: f64, realized: f64, open_mtm: f64) -> f64 {
    budget + realized + open_mtm
}

/// Marks a bar-close equity: running peak and most negative trough.
pub fn mark(bot: &mut StrategyBot, equity: f64) {
    if equity > bot.peak_equity {
        bot.peak_equity = equity;
    } else if equity - bot.peak_equity < bot.max_dd_quote {
        bot.max_dd_quote = equity - bot.peak_equity;
    }
}

/// Current drawdown from the peak, % of budget (<= 0).
pub fn drawdown_pct(peak: f64, equity: f64, budget: f64) -> f64 {
    if budget <= 0.0 {
        return 0.0;
    }
    ((equity - peak).min(0.0)) / budget * 100.0
}

/// Adds one bar to the utilisation integrals.
pub fn add_bar(u: &mut Utilisation, margin: f64, committed: f64) {
    u.bars += 1.0;
    u.margin_bars += margin;
    u.commit_bars += committed;
}

/// (in-position, committed) utilisation as fractions of budget.
pub fn utilisation(u: &Utilisation, budget: f64) -> (f64, f64) {
    if u.bars <= 0.0 || budget <= 0.0 {
        return (0.0, 0.0);
    }
    (u.margin_bars / (budget * u.bars), u.commit_bars / (budget * u.bars))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_down_never_scales_up_and_kills_below_one_pct() {
        assert_eq!(size_down(1000.0, 250.0), 1.0);
        assert!((size_down(1000.0, -250.0) - 0.75).abs() < 1e-12);
        assert!(!is_dead(size_down(1000.0, -989.0)));
        assert!(is_dead(size_down(1000.0, -995.0)));
    }

    #[test]
    fn funding_sign_by_side() {
        assert!(funding(2.0, 100.0, 0.0001) > 0.0); // long pays
        assert!(funding(-2.0, 100.0, 0.0001) < 0.0); // short receives
        assert!((funding(2.0, 100.0, 0.0001) - 0.02).abs() < 1e-15);
    }

    #[test]
    fn drawdown_from_bar_close_marks() {
        let mut b = super::super::driver::tests::sample_bot();
        b.peak_equity = 1000.0;
        for e in [1010.0, 990.0, 1020.0, 1000.0] {
            mark(&mut b, e);
        }
        assert_eq!(b.peak_equity, 1020.0);
        assert!((b.max_dd_quote + 20.0).abs() < 1e-12);
        assert!((drawdown_pct(1020.0, 1000.0, 1000.0) + 2.0).abs() < 1e-12);
    }

    #[test]
    fn utilisation_includes_idle_bars() {
        let mut u = Utilisation::default();
        add_bar(&mut u, 500.0, 1000.0);
        add_bar(&mut u, 0.0, 0.0); // cooldown bar
        let (pos, commit) = utilisation(&u, 1000.0);
        assert!((pos - 0.25).abs() < 1e-12);
        assert!((commit - 0.5).abs() < 1e-12);
    }
}
