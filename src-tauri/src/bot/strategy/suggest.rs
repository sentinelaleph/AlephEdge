//! Volatility-based grid suggestion (pure). From recent hourly bars: the
//! range is the last 14 days' low..high around the last close, each side at
//! least 1.5 daily ATRs; the step is about one hourly ATR but never below
//! the fee floor (`grid::min_net_per_grid_pct`) with a 25% margin. Geometric
//! spacing, so every grid earns the same percentage.

use serde::Serialize;

use super::costs::CostModel;
use super::grid;
use super::limits::{MAX_GRID_LEVELS, MIN_GRID_LEVELS};
use super::model::Bar;

/// Bars the range looks back over (14 days of 1h bars).
pub const RANGE_BARS: usize = 14 * 24;
/// ATR period (Wilder).
pub const ATR_PERIOD: usize = 14;
/// Each side of the range is at least this many daily ATRs.
const MIN_SIDE_DAILY_ATR: f64 = 1.5;
/// Margin over the fee floor for the suggested step.
const FLOOR_MARGIN: f64 = 1.25;
const DAY_MS: u64 = 86_400_000;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GridSuggestion {
    /// Last close the suggestion is centred on.
    pub price: f64,
    pub lower_pct: f64,
    pub upper_pct: f64,
    pub n_grids: u16,
    /// Gross geometric step, percent.
    pub step_pct: f64,
    /// Net per grid after maker fees on both sides, percent.
    pub net_per_grid_pct: f64,
    pub atr_hour_pct: f64,
    pub atr_day_pct: f64,
    /// Low / high of the range window (the last `RANGE_BARS` bars).
    pub window_low: f64,
    pub window_high: f64,
}

/// Wilder ATR over `bars` (oldest first); None below `period + 1` bars.
pub fn atr(bars: &[Bar], period: usize) -> Option<f64> {
    if period == 0 || bars.len() < period + 1 {
        return None;
    }
    let tr = |i: usize| {
        let (b, prev) = (&bars[i], bars[i - 1].c);
        (b.h - b.l).max((b.h - prev).abs()).max((b.l - prev).abs())
    };
    let mut a = (1..=period).map(tr).sum::<f64>() / period as f64;
    for i in period + 1..bars.len() {
        a = (a * (period as f64 - 1.0) + tr(i)) / period as f64;
    }
    a.is_finite().then_some(a)
}

/// Hourly (or any intraday) bars merged into UTC days, oldest first.
pub fn daily(bars: &[Bar]) -> Vec<Bar> {
    let mut out: Vec<Bar> = Vec::new();
    for b in bars {
        let day = b.open_ms - b.open_ms % DAY_MS;
        match out.last_mut() {
            Some(d) if d.open_ms == day => {
                d.h = d.h.max(b.h);
                d.l = d.l.min(b.l);
                d.c = b.c;
                d.close_ms = b.close_ms;
            }
            _ => out.push(Bar { open_ms: day, ..*b }),
        }
    }
    out
}

/// None when there is too little history (fewer than 15 days) or the
/// prices are not usable.
pub fn suggest_grid(hourly: &[Bar], costs: &CostModel) -> Option<GridSuggestion> {
    let price = hourly.last()?.c;
    if !(price.is_finite() && price > 0.0) {
        return None;
    }
    let days = daily(hourly);
    let atr_day = atr(&days, ATR_PERIOD)?;
    let atr_hour = atr(hourly, ATR_PERIOD)?;
    let window = &hourly[hourly.len().saturating_sub(RANGE_BARS)..];
    let low = window.iter().map(|b| b.l).fold(f64::INFINITY, f64::min);
    let high = window.iter().map(|b| b.h).fold(f64::NEG_INFINITY, f64::max);
    if !(low > 0.0 && high >= low) {
        return None;
    }
    let atr_day_pct = atr_day / price * 100.0;
    let side_min = MIN_SIDE_DAILY_ATR * atr_day_pct;
    let round1 = |x: f64| (x * 10.0).ceil() / 10.0;
    let lower_pct = round1(((price - low) / price * 100.0).max(side_min)).clamp(0.5, 50.0);
    let upper_pct = round1(((high - price) / price * 100.0).max(side_min)).clamp(0.5, 100.0);

    let floor_gross = grid::min_net_per_grid_pct(costs.maker, costs.slippage) + 2.0 * costs.maker * 100.0;
    let target_step = (atr_hour / price * 100.0).max(floor_gross * FLOOR_MARGIN);
    let span = ((1.0 + upper_pct / 100.0) / (1.0 - lower_pct / 100.0)).ln();
    let fit = (span / (1.0 + target_step / 100.0).ln()).floor();
    let mut n = (fit.max(0.0) as u16).clamp(MIN_GRID_LEVELS, MAX_GRID_LEVELS);
    let step = |n: u16| ((span / f64::from(n)).exp() - 1.0) * 100.0;
    // A narrow range can still leave the minimum grid count under the floor.
    while n > MIN_GRID_LEVELS && step(n) < floor_gross * FLOOR_MARGIN {
        n -= 1;
    }
    let step_pct = step(n);
    if step_pct < floor_gross {
        return None;
    }
    Some(GridSuggestion {
        price,
        lower_pct,
        upper_pct,
        n_grids: n,
        step_pct,
        net_per_grid_pct: step_pct - 2.0 * costs.maker * 100.0,
        atr_hour_pct: atr_hour / price * 100.0,
        atr_day_pct,
        window_low: low,
        window_high: high,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::strategy::costs::{FUTURES_SIM, SPOT_SIM};

    const H: u64 = 3_600_000;

    fn bar(i: u64, o: f64, h: f64, l: f64, c: f64) -> Bar {
        Bar { open_ms: i * H, close_ms: i * H + H - 1, o, h, l, c, funding_rate: None, funding_unknown: false }
    }

    /// 30 days of hourly bars oscillating +-2% around 100 once a day.
    fn wave(days: u64, amp: f64) -> Vec<Bar> {
        (0..days * 24)
            .map(|i| {
                let ph = (i % 24) as f64 / 24.0 * std::f64::consts::TAU;
                let c = 100.0 * (1.0 + amp * ph.sin());
                let prev = 100.0 * (1.0 + amp * (ph - std::f64::consts::TAU / 24.0).sin());
                bar(i, prev, c.max(prev) * 1.001, c.min(prev) * 0.999, c)
            })
            .collect()
    }

    #[test]
    fn atr_of_constant_ranges_is_that_range() {
        let bars: Vec<Bar> = (0..30).map(|i| bar(i, 100.0, 101.0, 99.0, 100.0)).collect();
        assert!((atr(&bars, 14).unwrap() - 2.0).abs() < 1e-12);
        assert_eq!(atr(&bars[..14], 14), None);
    }

    #[test]
    fn daily_merges_hours_into_utc_days() {
        let d = daily(&wave(3, 0.02));
        assert_eq!(d.len(), 3);
        assert_eq!(d[1].open_ms, DAY_MS);
        assert_eq!(d[1].close_ms, 2 * DAY_MS - 1);
    }

    #[test]
    fn suggestion_covers_the_range_and_clears_the_fee_floor() {
        let bars = wave(30, 0.02);
        for costs in [FUTURES_SIM, SPOT_SIM] {
            let s = suggest_grid(&bars, &costs).expect("enough history");
            assert!(s.lower_pct >= 2.0 && s.upper_pct >= 2.0, "{s:?}");
            assert!((MIN_GRID_LEVELS..=MAX_GRID_LEVELS).contains(&s.n_grids));
            let floor = grid::min_net_per_grid_pct(costs.maker, costs.slippage);
            assert!(s.net_per_grid_pct >= floor, "{s:?}");
            // The step follows the hourly ATR unless the fees force it wider.
            let floor_gross = (floor + 2.0 * costs.maker * 100.0) * FLOOR_MARGIN;
            assert!(s.step_pct >= s.atr_hour_pct.min(floor_gross) * 0.9, "{s:?}");
        }
    }

    #[test]
    fn a_suggested_config_passes_validation() {
        use crate::bot::strategy::model::{GridParams, GridRange, Spacing, StrategyParams};
        let s = suggest_grid(&wave(30, 0.02), &FUTURES_SIM).unwrap();
        let mut c = crate::bot::strategy::validate::tests::grid_cfg();
        c.params = StrategyParams::Grid(GridParams {
            range: GridRange::Relative { lower_pct: s.lower_pct, upper_pct: s.upper_pct },
            n_grids: s.n_grids,
            spacing: Spacing::Geom,
            stop_out_pct: Some(3.0),
            trailing_up: false,
            trail_up_limit: None,
            take_profit_pct: None,
            max_duration_min: None,
        });
        assert_eq!(crate::bot::strategy::validate::validate(&c, 2).err().map(|e| e.code), None);
    }

    #[test]
    fn too_little_history_gives_no_suggestion() {
        assert_eq!(suggest_grid(&wave(10, 0.02), &FUTURES_SIM), None);
        assert_eq!(suggest_grid(&[], &FUTURES_SIM), None);
    }
}
