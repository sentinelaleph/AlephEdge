//! Pure isolated-margin liquidation (flat maintenance margin rate, no tiers),
//! identical to the research simulator.
//!
//! A position is liquidated when margin + unrealised P&L <= mmr x notional at
//! that price. Solving for the price:
//!   long:  (N - M) / (Q * (1 - mmr))
//!   short: (N + M) / (Q * (1 + mmr))
//! N = filled notional, M = isolated margin, Q = quantity. A price <= 0 means
//! the position cannot be liquidated (a 1x long).

/// DCA: margin = filled notionals / leverage. `s` = +1 long, -1 short.
pub fn dca_liq_price(notional: f64, margin: f64, qty: f64, s: f64, mmr: f64) -> Option<f64> {
    if qty <= 0.0 {
        return None;
    }
    let p = (notional - s * margin) / (qty * (1.0 - s * mmr));
    (p > 1e-12).then_some(p)
}

/// Grid: the whole budget is the margin, realised P&L, fees and funding
/// included (`equity` = budget + cycle cash). `pos` is signed.
pub fn grid_liq_price(pos: f64, avg: f64, equity: f64, mmr: f64) -> Option<f64> {
    if pos == 0.0 {
        return None;
    }
    let p = if pos > 0.0 {
        (pos * avg - equity) / (pos * (1.0 - mmr))
    } else {
        (pos * avg - equity) / (pos * (1.0 + mmr))
    };
    (p > 1e-12).then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_10x_matches_botsim_self_test_5() {
        // 5000 notional at 100 * (1 - 0.0002), margin 500.
        let q = 5000.0 / (100.0 * 0.9998);
        let p = dca_liq_price(5000.0, 500.0, q, -1.0, 0.005).unwrap();
        assert!((p - 109.43).abs() < 0.01, "{p}");
        let exact = 5500.0 / (q * 1.005);
        assert!((p - exact).abs() < 1e-12);
    }

    #[test]
    fn one_x_long_never_liquidates() {
        assert_eq!(dca_liq_price(100.0, 100.0, 1.0, 1.0, 0.005), None);
    }

    #[test]
    fn long_liquidation_solves_the_margin_equation() {
        for lev in [2.0, 3.0, 5.0, 10.0, 20.0] {
            let n = 1000.0;
            let q = 10.0; // average 100
            let p = dca_liq_price(n, n / lev, q, 1.0, 0.005).unwrap();
            let lhs = n / lev + (q * p - n);
            assert!((lhs - 0.005 * q * p).abs() < 1e-9, "lev {lev}");
            assert!(p < 100.0);
        }
        for lev in [2.0, 5.0, 20.0] {
            let p = dca_liq_price(1000.0, 1000.0 / lev, 10.0, -1.0, 0.005).unwrap();
            let lhs = 1000.0 / lev - (10.0 * p - 1000.0);
            assert!((lhs - 0.005 * 10.0 * p).abs() < 1e-9, "short lev {lev}");
            assert!(p > 100.0);
        }
    }

    #[test]
    fn grid_liq_both_sides() {
        let long = grid_liq_price(10.0, 100.0, 200.0, 0.005).unwrap();
        assert!((10.0 * (long - 100.0) + 200.0 - 0.005 * 10.0 * long).abs() < 1e-9);
        let short = grid_liq_price(-10.0, 100.0, 200.0, 0.005).unwrap();
        assert!((-10.0 * (short - 100.0) + 200.0 - 0.005 * 10.0 * short).abs() < 1e-9);
        assert_eq!(grid_liq_price(0.0, 100.0, 200.0, 0.005), None);
        // a fully funded long cannot be liquidated
        assert_eq!(grid_liq_price(10.0, 100.0, 1000.0, 0.005), None);
    }
}
