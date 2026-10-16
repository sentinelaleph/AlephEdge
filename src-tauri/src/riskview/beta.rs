//! Beta of a coin to BTC from hourly DataHub candles (pure).
//!
//! Method: hourly log returns `ln(c_t / c_{t-1})` of the coin and of BTCUSDT
//! (same market), paired on the bar open time and only across consecutive
//! hours on both series (a missing hour drops that pair, it is never filled).
//! `beta = cov(coin, btc) / var(btc)`, the OLS slope of the coin's return on
//! BTC's; `r2 = cov^2 / (var_coin * var_btc)`, the share of the coin's hourly
//! variance BTC's direction explains. The beta used for shocks is clamped to
//! `[0, 3]`: a negative 30-day beta on crypto is noise, not a hedge to lean on,
//! and above 3 a few wicks dominate. The raw value is kept for display.
//! Fewer than `MIN_PAIRS` pairs (10 days) = no beta: the exposure is then
//! reported unavailable rather than guessed.

use serde::Serialize;

use crate::bot::strategy::model::Bar;

/// Days of hourly history the beta reads.
pub const BETA_DAYS: u64 = 30;
/// Fewest paired hourly returns a beta is computed from (10 days).
pub const MIN_PAIRS: usize = 240;
pub const BETA_MIN: f64 = 0.0;
pub const BETA_MAX: f64 = 3.0;
const HOUR_MS: u64 = 3_600_000;

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BetaInfo {
    /// Clamped to [0, 3]; what the shocks use.
    pub beta: f64,
    /// The OLS slope before the clamp.
    pub raw: f64,
    /// Share of the coin's hourly return variance explained by BTC, 0..1.
    pub r2: f64,
    /// Paired hourly returns it was computed from.
    pub samples: u32,
    pub clamped: bool,
}

impl BetaInfo {
    /// BTC against itself.
    pub fn unit() -> Self {
        Self { beta: 1.0, raw: 1.0, r2: 1.0, samples: 0, clamped: false }
    }
}

/// Hourly log returns keyed by the open time of the later bar; only where
/// the previous bar is exactly one hour earlier and both closes are positive.
fn returns(bars: &[Bar]) -> Vec<(u64, f64)> {
    bars.windows(2)
        .filter(|w| w[1].open_ms == w[0].open_ms + HOUR_MS && w[0].c > 0.0 && w[1].c > 0.0)
        .map(|w| (w[1].open_ms, (w[1].c / w[0].c).ln()))
        .filter(|(_, r)| r.is_finite())
        .collect()
}

/// Beta of `coin` to `btc` (both hourly, sorted by open time), or None when
/// fewer than `MIN_PAIRS` hours pair up or BTC did not move at all.
pub fn estimate(coin: &[Bar], btc: &[Bar]) -> Option<BetaInfo> {
    let rc = returns(coin);
    let rb = returns(btc);
    let mut pairs = Vec::with_capacity(rc.len().min(rb.len()));
    let (mut i, mut j) = (0, 0);
    while i < rc.len() && j < rb.len() {
        match rc[i].0.cmp(&rb[j].0) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                pairs.push((rc[i].1, rb[j].1));
                i += 1;
                j += 1;
            }
        }
    }
    if pairs.len() < MIN_PAIRS {
        return None;
    }
    let n = pairs.len() as f64;
    let mc = pairs.iter().map(|p| p.0).sum::<f64>() / n;
    let mb = pairs.iter().map(|p| p.1).sum::<f64>() / n;
    let (mut cov, mut vc, mut vb) = (0.0, 0.0, 0.0);
    for (c, b) in &pairs {
        cov += (c - mc) * (b - mb);
        vc += (c - mc) * (c - mc);
        vb += (b - mb) * (b - mb);
    }
    if vb <= 0.0 || !vb.is_finite() {
        return None;
    }
    let raw = cov / vb;
    let r2 = if vc > 0.0 { (cov * cov / (vc * vb)).clamp(0.0, 1.0) } else { 0.0 };
    if !raw.is_finite() {
        return None;
    }
    let beta = raw.clamp(BETA_MIN, BETA_MAX);
    Some(BetaInfo { beta, raw, r2, samples: pairs.len() as u32, clamped: beta != raw })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn hourly(closes: &[f64]) -> Vec<Bar> {
        closes
            .iter()
            .enumerate()
            .map(|(i, c)| Bar {
                open_ms: i as u64 * HOUR_MS,
                close_ms: (i as u64 + 1) * HOUR_MS - 1,
                o: *c,
                h: *c,
                l: *c,
                c: *c,
                funding_rate: None,
                funding_unknown: false,
            })
            .collect()
    }

    /// A deterministic, non-trivial BTC return series.
    fn btc_returns(n: usize) -> Vec<f64> {
        (0..n).map(|i| 0.01 * ((i as f64 * 0.7).sin() + 0.5 * (i as f64 * 1.3).cos())).collect()
    }

    fn path(returns: &[f64], start: f64) -> Vec<f64> {
        let mut out = vec![start];
        for r in returns {
            let last = *out.last().unwrap();
            out.push(last * r.exp());
        }
        out
    }

    #[test]
    fn a_coin_that_moves_twice_btc_has_beta_two_and_full_r2() {
        let rb = btc_returns(400);
        let rc: Vec<f64> = rb.iter().map(|r| 2.0 * r).collect();
        let b = estimate(&hourly(&path(&rc, 5.0)), &hourly(&path(&rb, 60_000.0))).unwrap();
        assert!((b.beta - 2.0).abs() < 1e-9, "{b:?}");
        assert!((b.r2 - 1.0).abs() < 1e-9);
        assert_eq!(b.samples, 400);
        assert!(!b.clamped);
    }

    #[test]
    fn independent_noise_lowers_r2_but_keeps_the_slope() {
        let rb = btc_returns(600);
        // Noise orthogonal-ish to BTC: a different frequency.
        let rc: Vec<f64> = rb.iter().enumerate().map(|(i, r)| 1.5 * r + 0.01 * (i as f64 * 2.9).sin()).collect();
        let b = estimate(&hourly(&path(&rc, 1.0)), &hourly(&path(&rb, 100.0))).unwrap();
        assert!((b.beta - 1.5).abs() < 0.15, "{b:?}");
        assert!(b.r2 > 0.3 && b.r2 < 0.95, "{b:?}");
    }

    #[test]
    fn beta_is_clamped_to_zero_and_three() {
        let rb = btc_returns(300);
        let neg: Vec<f64> = rb.iter().map(|r| -0.8 * r).collect();
        let b = estimate(&hourly(&path(&neg, 1.0)), &hourly(&path(&rb, 100.0))).unwrap();
        assert_eq!(b.beta, 0.0);
        assert!(b.raw < -0.7 && b.clamped);
        let big: Vec<f64> = rb.iter().map(|r| 5.0 * r).collect();
        let b = estimate(&hourly(&path(&big, 1.0)), &hourly(&path(&rb, 100.0))).unwrap();
        assert_eq!(b.beta, 3.0);
        assert!((b.raw - 5.0).abs() < 1e-9 && b.clamped);
    }

    #[test]
    fn too_little_history_or_a_flat_btc_gives_no_beta() {
        let rb = btc_returns(100);
        assert_eq!(estimate(&hourly(&path(&rb, 1.0)), &hourly(&path(&rb, 100.0))), None);
        let flat = vec![100.0; 400];
        assert_eq!(estimate(&hourly(&flat), &hourly(&flat)), None);
    }

    #[test]
    fn gaps_drop_pairs_instead_of_bridging_them() {
        let rb = btc_returns(300);
        let btc = hourly(&path(&rb, 100.0));
        let mut coin = hourly(&path(&rb, 1.0));
        // Remove 80 hours: the returns across the hole are not paired.
        coin.drain(100..180);
        assert_eq!(estimate(&coin, &btc), None, "219 pairs < MIN_PAIRS");
        let rb = btc_returns(500);
        let btc = hourly(&path(&rb, 100.0));
        let mut coin = hourly(&path(&rb, 1.0));
        coin.remove(250);
        let b = estimate(&coin, &btc).unwrap();
        assert_eq!(b.samples, 498, "two returns touch the missing hour");
        assert!((b.beta - 1.0).abs() < 1e-9);
    }
}
