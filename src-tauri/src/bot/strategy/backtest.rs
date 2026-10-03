//! Pure backtest runner for the DCA and Grid strategy bots (paper only).
//!
//! Not a second simulator: every bar goes through `driver::on_bar`, the exact
//! function the paper engine feeds its live closed bars to, and the cycle
//! still open at the last bar is closed with `driver::force_close` at that
//! bar's close (exit `end`). No gate is applied (no kill switch, breaker,
//! membership or BTC-break history exists for a past window). Funding is not
//! charged: the history source has no funding rates, so every `Bar` carries
//! `funding_rate: None` and the report says "Funding not included".
//!
//! No I/O, no Tauri, no exchange code. Candles come in as `Bar`s from the
//! caller (DataHub via the Sentinel API); this module never fetches anything.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::accounting;
use super::cycle::CycleState;
use super::driver;
use super::model::{Bar, BotRunState, ExitReason, StrategyBot, StrategyConfig, Utilisation};

/// The id the simulated bot carries (client ids are derived from it).
pub const BACKTEST_BOT_ID: &str = "sb_backtest00000";
/// Equity curve points kept in a stored run.
pub const EQUITY_POINTS_MAX: usize = 500;
/// Cycles kept in a stored run (the newest are kept; totals cover all).
pub const CYCLES_MAX: usize = 5000;
/// Candles the history route returns per request.
pub const CANDLES_PER_REQUEST: u64 = 9000;
/// Longest window a run may cover.
pub const MAX_WINDOW_MS: u64 = 3 * 366 * 86_400_000;

/// Bar intervals the history route serves: (name, milliseconds).
pub const INTERVALS: [(&str, u64); 4] = [
    ("15m", 15 * 60_000),
    ("1h", 3_600_000),
    ("4h", 4 * 3_600_000),
    ("1d", 86_400_000),
];

pub fn interval_ms(name: &str) -> Option<u64> {
    INTERVALS.iter().find(|(n, _)| *n == name).map(|(_, ms)| *ms)
}

/// Request windows `[from, to]` (inclusive bounds) covering `[start, end)`,
/// each at most `CANDLES_PER_REQUEST` candles even when the server treats
/// `to` as inclusive. Consecutive pages share their boundary candle (deduped
/// by the caller) so an exclusive `to` leaves no hole either.
pub fn plan_pages(start_ms: u64, end_ms: u64, interval_ms: u64) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    if interval_ms == 0 || end_ms <= start_ms {
        return out;
    }
    let span = (CANDLES_PER_REQUEST - 1) * interval_ms;
    let mut from = start_ms;
    loop {
        let to = (from + span).min(end_ms);
        out.push((from, to));
        if to >= end_ms {
            break;
        }
        from = to;
    }
    out
}

/// One candle of the history response, already parsed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candle {
    pub open_ms: u64,
    pub o: f64,
    pub h: f64,
    pub l: f64,
    pub c: f64,
}

/// Candles -> closed bars: sorted, deduplicated by open time, malformed rows
/// dropped (non-finite or non-positive prices, high below low, open/close
/// outside the range), and a candle still forming at `now_ms` dropped.
/// Returns (bars, dropped count). Funding is never attached.
pub fn candles_to_bars(mut candles: Vec<Candle>, interval_ms: u64, now_ms: u64) -> (Vec<Bar>, u32) {
    candles.sort_by_key(|c| c.open_ms);
    let mut bars: Vec<Bar> = Vec::with_capacity(candles.len());
    let mut dropped = 0u32;
    for k in candles {
        if bars.last().is_some_and(|b| b.open_ms == k.open_ms) {
            continue; // page boundary overlap
        }
        let prices_ok = [k.o, k.h, k.l, k.c].iter().all(|p| p.is_finite() && *p > 0.0);
        let shape_ok = prices_ok
            && k.h >= k.l
            && (k.l..=k.h).contains(&k.o)
            && (k.l..=k.h).contains(&k.c);
        let closed = k.open_ms + interval_ms <= now_ms;
        if !shape_ok || !closed {
            dropped += 1;
            continue;
        }
        bars.push(Bar {
            open_ms: k.open_ms,
            close_ms: k.open_ms + interval_ms - 1,
            o: k.o,
            h: k.h,
            l: k.l,
            c: k.c,
            funding_rate: None,
            funding_unknown: false,
        });
    }
    (bars, dropped)
}

/// Bars missing between consecutive bars (interval grid holes).
pub fn missing_bars(bars: &[Bar], interval_ms: u64) -> u64 {
    if interval_ms == 0 {
        return 0;
    }
    bars.windows(2)
        .map(|w| (w[1].open_ms.saturating_sub(w[0].open_ms) / interval_ms).saturating_sub(1))
        .sum()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BacktestCycle {
    pub seq: u32,
    pub opened_at: u64,
    pub closed_at: u64,
    pub exit: String,
    pub anchor_price: f64,
    pub avg_entry: Option<f64>,
    pub so_filled: Option<u8>,
    pub grid_closing_fills: Option<u32>,
    pub fills: u32,
    pub fees_quote: f64,
    pub funding_quote: f64,
    /// Net P&L after fees (quote).
    pub pnl_quote: f64,
    /// Net P&L, % of the bot budget.
    pub pnl_pct_budget: f64,
    /// Worst bar-close MTM of the cycle, % of the bot budget (<= 0).
    pub max_adverse_pct: f64,
    /// Still open at the last bar; closed there at market (exit `end`).
    pub open_at_end: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EquitySample {
    pub ts: u64,
    pub equity: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BacktestResult {
    pub budget: f64,
    pub bars: u32,
    pub first_bar_ms: u64,
    pub last_bar_ms: u64,
    /// Cycles that closed inside the window (open-at-end excluded).
    pub closed_cycles: u32,
    pub wins: u32,
    pub losses: u32,
    pub closed_pnl_quote: f64,
    /// The cycle open at the last bar, closed there at market.
    pub open_at_end: Option<BacktestCycle>,
    pub open_at_end_pnl_quote: f64,
    pub total_pnl_quote: f64,
    pub total_pnl_pct: f64,
    /// Peak-to-trough of bar-close MTM equity (<= 0).
    pub max_drawdown_quote: f64,
    pub max_drawdown_pct: f64,
    /// Longest time equity stayed below its running peak.
    pub longest_underwater_ms: u64,
    /// Deepest safety order filled in any cycle (DCA).
    pub deepest_so: Option<u8>,
    /// Grid closing fills over all cycles (Grid).
    pub grid_closing_fills: Option<u32>,
    pub total_fills: u32,
    pub liquidations: u32,
    pub fees_quote: f64,
    /// Always 0: funding is not part of the history source.
    pub funding_quote: f64,
    pub exits: BTreeMap<String, u32>,
    /// Bot state at the last bar, before the end close.
    pub end_state: String,
    pub dead_reason: Option<String>,
    /// Driver note keys (strategy.notes.*) and how often they fired.
    pub notes: BTreeMap<String, u32>,
    pub utilisation_in_position: f64,
    pub utilisation_committed: f64,
    /// Bar-close equity, downsampled to <= EQUITY_POINTS_MAX points.
    pub equity: Vec<EquitySample>,
    /// Closed cycles (newest CYCLES_MAX) then the open-at-end one.
    pub cycles: Vec<BacktestCycle>,
    pub cycles_truncated: bool,
}

fn cycle_row(c: &CycleState, bot_budget: f64, open_at_end: bool) -> BacktestCycle {
    BacktestCycle {
        seq: c.core.seq,
        opened_at: c.core.opened_at,
        closed_at: c.core.now_ms,
        exit: c.core.exit.unwrap_or(ExitReason::End).as_str().to_string(),
        anchor_price: c.core.anchor_price,
        avg_entry: c.avg_entry(),
        so_filled: c.so_filled(),
        grid_closing_fills: c.grid_closing_fills(),
        fills: c.core.fills,
        fees_quote: c.core.fees,
        funding_quote: c.core.funding,
        pnl_quote: c.core.cash,
        pnl_pct_budget: c.core.cash / bot_budget * 100.0,
        max_adverse_pct: c.core.max_adverse_quote / bot_budget * 100.0,
        open_at_end,
    }
}

/// The simulated bot: armed from the first bar, nothing realised yet.
pub fn fresh_bot(cfg: &StrategyConfig, first_open_ms: u64) -> StrategyBot {
    StrategyBot {
        id: BACKTEST_BOT_ID.to_string(),
        cfg: cfg.clone(),
        state: BotRunState::Armed,
        created_at: first_open_ms,
        cycles_done: 0,
        realized_quote: 0.0,
        peak_equity: cfg.budget,
        max_dd_quote: 0.0,
        dead_reason: None,
        next_decision_ms: first_open_ms,
        chain_cycles: 0,
        start_triggered: false,
        util: Utilisation::default(),
        archived_at: None,
    }
}

/// Keeps the first and last point and, per bucket, its lowest and highest
/// equity in time order, so a downsampled curve never hides the trough.
pub fn downsample(points: &[EquitySample], max: usize) -> Vec<EquitySample> {
    if points.len() <= max || max < 4 {
        return points.to_vec();
    }
    let inner = &points[1..points.len() - 1];
    let buckets = (max - 2) / 2;
    let size = inner.len().div_ceil(buckets);
    let mut out = Vec::with_capacity(max);
    out.push(points[0]);
    for chunk in inner.chunks(size) {
        let (mut lo, mut hi) = (0usize, 0usize);
        for (i, p) in chunk.iter().enumerate() {
            if p.equity < chunk[lo].equity {
                lo = i;
            }
            if p.equity > chunk[hi].equity {
                hi = i;
            }
        }
        let (a, b) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        out.push(chunk[a]);
        if b != a {
            out.push(chunk[b]);
        }
    }
    out.push(points[points.len() - 1]);
    out
}

/// Runs `cfg` over `bars` (ascending, closed). The caller validates `cfg`
/// exactly as bot creation does.
pub fn run(cfg: &StrategyConfig, bars: &[Bar]) -> Result<BacktestResult, &'static str> {
    let (Some(first), Some(last)) = (bars.first(), bars.last()) else {
        return Err("historyEmpty");
    };
    if !(cfg.budget.is_finite() && cfg.budget > 0.0) {
        return Err("budgetInvalid");
    }
    let budget = cfg.budget;
    let mut bot = fresh_bot(cfg, first.open_ms);
    let mut cycle: Option<CycleState> = None;
    let mut closed: Vec<CycleState> = Vec::new();
    let mut notes: BTreeMap<String, u32> = BTreeMap::new();
    let mut curve: Vec<EquitySample> = Vec::with_capacity(bars.len() + 1);

    for bar in bars {
        let out = driver::on_bar(&mut bot, &mut cycle, bar, None);
        for (key, _) in out.notes {
            *notes.entry(key.to_string()).or_insert(0) += 1;
        }
        if let Some(c) = out.closed {
            closed.push(c);
        }
        let eq = match out.equity {
            Some(e) => e.equity,
            None => accounting::equity(budget, bot.realized_quote, cycle.as_ref().map_or(0.0, |c| c.mtm(bar.c))),
        };
        curve.push(EquitySample { ts: bar.close_ms, equity: eq });
    }

    let end_state = bot.state.as_str().to_string();
    let dead_reason = bot.dead_reason.map(|r| r.as_str().to_string());
    let open_at_end = driver::force_close(&mut bot, &mut cycle, last.c, last.close_ms, ExitReason::End).map(|(c, _)| c);
    if open_at_end.is_some() {
        // The end close's exit fee and slippage are part of the result.
        if let Some(p) = curve.last_mut() {
            p.equity = accounting::equity(budget, bot.realized_quote, 0.0);
        }
    }

    // Drawdown and time underwater over the full-resolution curve.
    let mut peak = budget;
    let mut peak_ts = first.open_ms;
    let mut max_dd = 0.0_f64;
    let mut longest = 0u64;
    let mut under = false;
    for p in &curve {
        // Underwater runs from the peak's time until the first point back at
        // or above it (or the last point, when it never recovers).
        if p.equity < peak {
            max_dd = max_dd.min(p.equity - peak);
            longest = longest.max(p.ts.saturating_sub(peak_ts));
            under = true;
        } else {
            if under {
                longest = longest.max(p.ts.saturating_sub(peak_ts));
                under = false;
            }
            peak = p.equity;
            peak_ts = p.ts;
        }
    }

    let wins = closed.iter().filter(|c| c.core.cash > 0.0).count() as u32;
    let losses = closed.iter().filter(|c| c.core.cash < 0.0).count() as u32;
    let closed_pnl: f64 = closed.iter().map(|c| c.core.cash).sum();
    let end_pnl = open_at_end.as_ref().map_or(0.0, |c| c.core.cash);
    let all: Vec<&CycleState> = closed.iter().chain(open_at_end.iter()).collect();
    let mut exits = BTreeMap::new();
    for c in &closed {
        *exits.entry(c.core.exit.unwrap_or(ExitReason::End).as_str().to_string()).or_insert(0) += 1;
    }
    let is_dca = matches!(cfg.params, super::model::StrategyParams::Dca(_));
    let total = closed_pnl + end_pnl;
    let (u_pos, u_commit) = accounting::utilisation(&bot.util, budget);

    let truncated = closed.len() > CYCLES_MAX;
    let mut cycles: Vec<BacktestCycle> = closed[closed.len().saturating_sub(CYCLES_MAX)..]
        .iter()
        .map(|c| cycle_row(c, budget, false))
        .collect();
    let end_row = open_at_end.as_ref().map(|c| cycle_row(c, budget, true));
    if let Some(r) = &end_row {
        cycles.push(r.clone());
    }

    Ok(BacktestResult {
        budget,
        bars: bars.len() as u32,
        first_bar_ms: first.open_ms,
        last_bar_ms: last.close_ms,
        closed_cycles: closed.len() as u32,
        wins,
        losses,
        closed_pnl_quote: closed_pnl,
        open_at_end: end_row,
        open_at_end_pnl_quote: end_pnl,
        total_pnl_quote: total,
        total_pnl_pct: total / budget * 100.0,
        max_drawdown_quote: max_dd,
        max_drawdown_pct: max_dd / budget * 100.0,
        longest_underwater_ms: longest,
        deepest_so: if is_dca { Some(all.iter().filter_map(|c| c.so_filled()).max().unwrap_or(0)) } else { None },
        grid_closing_fills: if is_dca { None } else { Some(all.iter().filter_map(|c| c.grid_closing_fills()).sum()) },
        total_fills: all.iter().map(|c| c.core.fills).sum(),
        liquidations: all.iter().filter(|c| c.core.exit == Some(ExitReason::Liq)).count() as u32,
        fees_quote: all.iter().map(|c| c.core.fees).sum(),
        funding_quote: all.iter().map(|c| c.core.funding).sum(),
        exits,
        end_state,
        dead_reason,
        notes,
        utilisation_in_position: u_pos,
        utilisation_committed: u_commit,
        equity: downsample(&curve, EQUITY_POINTS_MAX),
        cycles,
        cycles_truncated: truncated,
    })
}

/// UNIX millis -> "YYYY-MM-DDTHH:MM:SSZ" (seconds resolution).
pub fn rfc3339(ms: u64) -> String {
    let secs = ms / 1000;
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::strategy::driver::tests::{dca_params, sample_bot, wave};
    use crate::bot::strategy::model::{DcaParams, Side, StrategyParams};

    const H: u64 = 3_600_000;
    const T0: u64 = 1_736_121_600_000; // 2025-01-06 00:00 UTC

    fn hour_bars(ohlc: &[(f64, f64, f64, f64)]) -> Vec<Bar> {
        let cs = ohlc
            .iter()
            .enumerate()
            .map(|(i, (o, h, l, c))| Candle { open_ms: T0 + i as u64 * H, o: *o, h: *h, l: *l, c: *c })
            .collect();
        let (bars, dropped) = candles_to_bars(cs, H, u64::MAX);
        assert_eq!(dropped, 0);
        bars
    }

    fn close(got: f64, want: f64) {
        assert!((got - want).abs() <= 1e-9 * want.abs().max(1.0), "got {got}, want {want}");
    }

    /// botsim golden 2 (SO then TP in one bullish bar), then a restart on
    /// the next bar that is still open at data end.
    #[test]
    fn golden_dca_closed_cycle_plus_open_at_end() {
        let cfg = sample_bot().cfg; // futures, long, 1x, budget 1000, fixed 100/100, 2 SO @2%, TP 2%
        let bars = hour_bars(&[
            (100.0, 100.5, 99.5, 100.2),
            (100.2, 101.5, 97.0, 101.0),
            (101.0, 101.0, 101.0, 101.0),
        ]);
        let r = run(&cfg, &bars).unwrap();

        let q1 = 100.0 / (100.0 * 1.0002) + 100.0 / 98.0;
        let tp = 200.0 / q1 * 1.02;
        let c1 = q1 * tp - 200.0 - 0.05 - 0.02 - q1 * tp * 0.0002;
        close(c1, 3.8892);
        let q2 = 100.0 / (101.0 * 1.0002);
        let px2 = 101.0 * (1.0 - 0.0002);
        let c2 = q2 * px2 - 100.0 - 0.05 - q2 * px2 * 0.0005;

        assert_eq!(r.closed_cycles, 1);
        assert_eq!(r.wins, 1);
        assert_eq!(r.losses, 0);
        close(r.closed_pnl_quote, c1);
        let end = r.open_at_end.as_ref().expect("second cycle open at end");
        assert_eq!(end.exit, "end");
        assert!(end.open_at_end);
        assert_eq!(end.seq, 2);
        close(r.open_at_end_pnl_quote, c2);
        close(r.total_pnl_quote, c1 + c2);
        close(r.total_pnl_pct, (c1 + c2) / 1000.0 * 100.0);
        close(r.fees_quote, (0.05 + 0.02 + q1 * tp * 0.0002) + (0.05 + q2 * px2 * 0.0005));
        assert_eq!(r.funding_quote, 0.0);
        assert_eq!(r.deepest_so, Some(1));
        assert_eq!(r.grid_closing_fills, None);
        assert_eq!(r.liquidations, 0);
        assert_eq!(r.exits.get("tp"), Some(&1));
        assert_eq!(r.cycles.len(), 2);
        assert_eq!(r.end_state, "inCycle");
        // curve: bar-0 MTM, bar-1 settled TP, then the end close (settled)
        let q0 = 100.0 / (100.0 * 1.0002);
        let e0 = 1000.0 - 0.05 + (q0 * 100.2 - 100.0);
        assert!(e0 > 1000.0);
        assert_eq!(r.equity.len(), 3);
        close(r.equity[0].equity, e0);
        close(r.equity[1].equity, 1000.0 + c1);
        close(r.equity[2].equity, 1000.0 + c1 + c2);
        assert!(c2 < 0.0);
        // the only trough: the end cycle's entry and exit costs below the TP peak
        close(r.max_drawdown_quote, c2);
        close(r.max_drawdown_pct, c2 / 10.0);
        assert_eq!(r.longest_underwater_ms, bars[2].close_ms - bars[1].close_ms);
        assert_eq!(r.bars, 3);
    }

    /// Shared-code proof: the backtest equals the paper engine's own path,
    /// `driver::on_bar` fed the same bars, then `force_close` at the end.
    #[test]
    fn backtest_equals_the_paper_engine_on_the_same_bars() {
        let bars = wave();
        let cfg = sample_bot().cfg;
        let r = run(&cfg, &bars).unwrap();

        let mut bot = fresh_bot(&cfg, bars[0].open_ms);
        let mut cyc = None;
        let mut closed = Vec::new();
        for b in &bars {
            if let Some(c) = driver::on_bar(&mut bot, &mut cyc, b, None).closed {
                closed.push(c.core.cash);
            }
        }
        let last = bars.last().unwrap();
        let end = driver::force_close(&mut bot, &mut cyc, last.c, last.close_ms, ExitReason::End).map(|(c, _)| c.core.cash);

        assert!(closed.len() >= 2);
        assert_eq!(r.closed_cycles as usize, closed.len());
        let got: Vec<f64> = r.cycles.iter().filter(|c| !c.open_at_end).map(|c| c.pnl_quote).collect();
        assert_eq!(got, closed, "bit-identical cycle results");
        assert_eq!(r.open_at_end.as_ref().map(|c| c.pnl_quote), end);
        assert_eq!(r.total_pnl_quote, bot.realized_quote);
        assert_eq!(r.max_drawdown_quote.min(0.0), r.max_drawdown_quote);
        assert!(r.max_drawdown_quote <= bot.max_dd_quote + 1e-12);
    }

    #[test]
    fn liquidation_is_counted_and_the_bot_dies() {
        let mut cfg = sample_bot().cfg;
        cfg.side = Side::Short;
        cfg.leverage = 10;
        cfg.params = StrategyParams::Dca(DcaParams {
            base_order: Some(5000.0),
            safety_order: None,
            max_so: 0,
            tp_pct: 5.0,
            ..dca_params()
        });
        let bars = hour_bars(&[
            (100.0, 100.5, 99.6, 100.1),
            (100.1, 112.0, 100.0, 111.0),
            (111.0, 111.0, 111.0, 111.0),
            (111.0, 111.0, 111.0, 111.0),
        ]);
        let r = run(&cfg, &bars).unwrap();
        assert_eq!(r.liquidations, 1);
        assert_eq!(r.end_state, "dead");
        assert_eq!(r.dead_reason.as_deref(), Some("liq"));
        assert!(r.open_at_end.is_none());
        close(r.total_pnl_quote, -502.5);
        assert!(r.max_drawdown_pct <= -50.0);
    }

    #[test]
    fn pages_cover_the_window_within_the_request_cap() {
        let start = T0;
        let end = T0 + 730 * 24 * H; // 2 years of 1h = 17,520 candles
        let p = plan_pages(start, end, H);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].0, start);
        assert_eq!(p.last().unwrap().1, end);
        for (a, b) in &p {
            assert!((b - a) / H < CANDLES_PER_REQUEST, "<= 9000 candles even with inclusive bounds");
        }
        assert_eq!(p[0].1, p[1].0, "boundary shared, no hole");
        assert_eq!(plan_pages(start, start + 30 * 24 * H, H).len(), 1);
        assert!(plan_pages(start, start, H).is_empty());
    }

    #[test]
    fn candles_are_sorted_deduped_and_cleaned() {
        let c = |i: u64, o: f64, h: f64, l: f64, cl: f64| Candle { open_ms: T0 + i * H, o, h, l, c: cl };
        let raw = vec![
            c(2, 10.0, 11.0, 9.0, 10.5),
            c(0, 10.0, 11.0, 9.0, 10.5),
            c(0, 10.0, 11.0, 9.0, 10.5), // duplicate
            c(1, 10.0, 9.0, 11.0, 10.5), // high below low
            c(3, f64::NAN, 11.0, 9.0, 10.5),
            c(5, 10.0, 11.0, 9.0, 10.5), // still forming
        ];
        let (bars, dropped) = candles_to_bars(raw, H, T0 + 5 * H + 10);
        assert_eq!(bars.iter().map(|b| b.open_ms).collect::<Vec<_>>(), vec![T0, T0 + 2 * H]);
        assert_eq!(dropped, 3);
        assert_eq!(bars[0].close_ms, T0 + H - 1);
        assert!(bars.iter().all(|b| b.funding_rate.is_none()));
        assert_eq!(missing_bars(&bars, H), 1);
    }

    #[test]
    fn downsample_keeps_the_trough_and_the_ends() {
        let pts: Vec<EquitySample> = (0..10_000u64)
            .map(|i| EquitySample { ts: i, equity: if i == 6_789 { 1.0 } else { 1000.0 + (i % 7) as f64 } })
            .collect();
        let d = downsample(&pts, EQUITY_POINTS_MAX);
        assert!(d.len() <= EQUITY_POINTS_MAX);
        assert_eq!(d.first(), pts.first());
        assert_eq!(d.last(), pts.last());
        assert!(d.iter().any(|p| p.equity == 1.0));
        assert!(d.windows(2).all(|w| w[0].ts < w[1].ts));
    }

    #[test]
    fn rfc3339_round_trips_through_the_signal_parser() {
        for ms in [0u64, T0, 1_709_164_800_000, 4_102_444_799_000] {
            let s = rfc3339(ms);
            assert_eq!(crate::signal::time::parse_rfc3339_ms(&s), Some(ms), "{s}");
        }
        assert_eq!(rfc3339(T0), "2025-01-06T00:00:00Z");
    }

    #[test]
    fn empty_history_is_refused() {
        assert_eq!(run(&sample_bot().cfg, &[]).err(), Some("historyEmpty"));
    }
}
