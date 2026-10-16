//! Historical crash replay (pure): today's open book walked through the
//! actual hourly price path of a past crash window.
//!
//! Each coin's path is its own DataHub candles for the window, rescaled so
//! the window's first open equals today's mark. A coin with no usable
//! history there (listed later, too many hours missing) follows beta x BTC's
//! path instead and is labelled "estimated (beta)". BTC itself must be served
//! with at least `MIN_COVERAGE` of the window's hours, or the whole window is
//! unavailable: nothing is invented. Walking is path dependent (stops,
//! ladder fills, liquidations along the way: `Exposure::walk`).

use serde::Serialize;

use super::beta::BetaInfo;
use super::exposure::{Exposure, PRICE_FLOOR};
use super::stress::{aggregate, contribution, BookResult};
use crate::bot::strategy::model::Bar;

pub const HOUR_MS: u64 = 3_600_000;
/// Share of the window's hours a series needs to count as served.
pub const MIN_COVERAGE: f64 = 0.95;

/// A past crash window, UTC, end exclusive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub id: &'static str,
    pub start: &'static str,
    pub end: &'static str,
}

/// The windows the Risk page replays.
pub const WINDOWS: [Window; 5] = [
    // 10 Oct 2025: the tariff-headline liquidation cascade.
    Window { id: "oct2025", start: "2025-10-10T00:00:00Z", end: "2025-10-12T00:00:00Z" },
    // FTX insolvency, 8-10 Nov 2022.
    Window { id: "ftx2022", start: "2022-11-08T00:00:00Z", end: "2022-11-11T00:00:00Z" },
    // LUNA / UST collapse, 9-12 May 2022.
    Window { id: "luna2022", start: "2022-05-09T00:00:00Z", end: "2022-05-13T00:00:00Z" },
    // 19 May 2021.
    Window { id: "may2021", start: "2021-05-19T00:00:00Z", end: "2021-05-20T00:00:00Z" },
    // 12 Mar 2020 ("Black Thursday").
    Window { id: "mar2020", start: "2020-03-12T00:00:00Z", end: "2020-03-13T00:00:00Z" },
];

impl Window {
    pub fn range_ms(&self) -> (u64, u64) {
        let p = |s| crate::signal::time::parse_rfc3339_ms(s).expect("window dates are valid RFC 3339");
        (p(self.start), p(self.end))
    }

    #[cfg(test)]
    pub fn hours(&self) -> usize {
        let (a, b) = self.range_ms();
        ((b - a) / HOUR_MS) as usize
    }
}

/// Bars of `[start, end)` only, sorted, and whether they cover the window.
pub fn served(bars: &[Bar], start: u64, end: u64) -> (Vec<Bar>, bool) {
    let inside: Vec<Bar> = bars.iter().copied().filter(|b| b.open_ms >= start && b.open_ms < end).collect();
    let expected = ((end - start) / HOUR_MS) as f64;
    let ok = expected > 0.0
        && inside.len() as f64 >= MIN_COVERAGE * expected
        && inside.first().is_some_and(|b| b.open_ms < start + 2 * HOUR_MS)
        && inside.iter().all(|b| b.o > 0.0 && b.h > 0.0 && b.l > 0.0 && b.c > 0.0);
    (inside, ok)
}

/// BTC's move over the window (last close vs first open) and its deepest
/// point (lowest low vs first open), %.
pub fn btc_summary(btc: &[Bar]) -> Option<(f64, f64)> {
    let first = btc.first()?.o;
    let last = btc.last()?.c;
    let low = btc.iter().map(|b| b.l).fold(f64::INFINITY, f64::min);
    (first > 0.0).then(|| ((last / first - 1.0) * 100.0, (low / first - 1.0) * 100.0))
}

fn retime(i: usize, t0: u64) -> (u64, u64) {
    let open = t0 + i as u64 * HOUR_MS;
    (open, open + HOUR_MS - 1)
}

/// The coin's own path rescaled to start at `mark`, re-timed from `t0`.
pub fn scaled_path(coin: &[Bar], mark: f64, t0: u64) -> Vec<Bar> {
    let k = mark / coin[0].o;
    coin.iter()
        .enumerate()
        .map(|(i, b)| {
            let (open_ms, close_ms) = retime(i, t0);
            Bar { open_ms, close_ms, o: b.o * k, h: b.h * k, l: b.l * k, c: b.c * k, funding_rate: None, funding_unknown: false }
        })
        .collect()
}

/// beta x BTC's path from `mark` (beta >= 0 keeps highs as highs).
pub fn beta_path(btc: &[Bar], beta: f64, mark: f64, t0: u64) -> Vec<Bar> {
    let b0 = btc[0].o;
    let px = |x: f64| mark * (1.0 + beta * (x / b0 - 1.0)).max(PRICE_FLOOR);
    btc.iter()
        .enumerate()
        .map(|(i, b)| {
            let (open_ms, close_ms) = retime(i, t0);
            Bar { open_ms, close_ms, o: px(b.o), h: px(b.h), l: px(b.l), c: px(b.c), funding_rate: None, funding_unknown: false }
        })
        .collect()
}

/// The path one exposure is walked through: its own served history, else
/// beta x BTC, else none (no beta either: excluded).
pub fn path_for(coin: Option<&[Bar]>, btc: &[Bar], beta: Option<BetaInfo>, mark: f64, t0: u64) -> Option<(Vec<Bar>, &'static str)> {
    if let Some(c) = coin.filter(|c| !c.is_empty()) {
        return Some((scaled_path(c, mark, t0), "history"));
    }
    beta.map(|b| (beta_path(btc, b.beta, mark, t0), "beta"))
}

/// One book through one window. `coin_of(e)` returns the exposure's served
/// window bars (already checked with `served`), or None.
pub fn replay_book<'a>(
    book: &[(&'a Exposure, Option<BetaInfo>)],
    btc: &[Bar],
    coin_of: impl Fn(&'a Exposure) -> Option<Vec<Bar>>,
    t0: u64,
) -> BookResult {
    let capital = book.iter().map(|(e, _)| e.capital).sum();
    let mut rows = Vec::new();
    let mut excluded = 0;
    for (e, beta) in book {
        let coin = coin_of(e);
        match path_for(coin.as_deref(), btc, *beta, e.mark, t0) {
            Some((path, basis)) => {
                let out = e.walk(&path);
                // R^2 of the coin to BTC; a coin with no beta (own history
                // only) counts as unexplained.
                rows.push((contribution(e, out.pnl, out.exit, Some(basis)), beta.map(|b| b.r2)));
            }
            None => excluded += 1,
        }
    }
    aggregate(capital, rows, excluded)
}

/// One window as the Risk page shows it.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReplayWindow {
    pub id: &'static str,
    pub start_ms: u64,
    pub end_ms: u64,
    /// "ok" | "unavailable".
    pub status: &'static str,
    /// Why it is unavailable (`errors.*` / `backtest.errors.*` code).
    pub error: Option<String>,
    pub btc_move_pct: Option<f64>,
    pub btc_low_pct: Option<f64>,
    pub paper: Option<BookResult>,
    pub real: Option<BookResult>,
}

impl ReplayWindow {
    pub fn unavailable(w: &Window, error: String) -> Self {
        let (start_ms, end_ms) = w.range_ms();
        Self { id: w.id, start_ms, end_ms, status: "unavailable", error: Some(error), btc_move_pct: None, btc_low_pct: None, paper: None, real: None }
    }
}

#[cfg(test)]
mod tests {
    use super::super::beta::tests::hourly;
    use super::super::exposure::tests::{cycle_exposure, dca_cycle, long_position, pos_exposure};
    use super::*;
    use crate::bot::strategy::model::MarketKind;

    fn ohlc(rows: &[(f64, f64, f64, f64)]) -> Vec<Bar> {
        rows.iter()
            .enumerate()
            .map(|(i, (o, h, l, c))| Bar {
                open_ms: i as u64 * HOUR_MS,
                close_ms: (i as u64 + 1) * HOUR_MS - 1,
                o: *o,
                h: *h,
                l: *l,
                c: *c,
                funding_rate: None,
                funding_unknown: false,
            })
            .collect()
    }

    #[test]
    fn windows_parse_and_have_whole_hours() {
        for w in WINDOWS {
            let (a, b) = w.range_ms();
            assert!(b > a && (b - a) % HOUR_MS == 0, "{}", w.id);
        }
        assert_eq!(WINDOWS[0].hours(), 48);
        assert_eq!(WINDOWS[4].hours(), 24);
    }

    #[test]
    fn replay_is_path_dependent() {
        // Same start and end price (100 -> 100), two paths. A long with its
        // stop at 90 survives the gentle path and is stopped on the one that
        // wicks to 85 first.
        let e = pos_exposure(long_position(90.0, 150.0, 1.0), 100.0);
        let gentle = ohlc(&[(100.0, 101.0, 95.0, 97.0), (97.0, 100.0, 96.0, 100.0)]);
        let wick = ohlc(&[(100.0, 100.0, 85.0, 92.0), (92.0, 100.0, 92.0, 100.0)]);
        let g = e.walk(&scaled_path(&gentle, 100.0, 0));
        let w = e.walk(&scaled_path(&wick, 100.0, 0));
        assert_eq!(g.exit, None);
        assert!(g.pnl.abs() < 1e-9, "{g:?}");
        assert_eq!(w.exit, Some("sl"));
        assert!(w.pnl < -9.9, "{w:?}");
        // A DCA ladder: the wick fills SO1-SO3 and the recovery closes the
        // deal at its take profit; the gentle path fills SO1 only.
        let c = cycle_exposure(dca_cycle(None, 1.0, MarketKind::Spot), 100.0);
        let gw = c.walk(&scaled_path(&gentle, 100.0, 0));
        let ww = c.walk(&scaled_path(&wick, 100.0, 0));
        assert_ne!(gw, ww);
        assert_eq!(ww.exit, Some("tp"));
        assert!(ww.pnl > 0.0, "{ww:?}");
    }

    #[test]
    fn a_path_is_rescaled_to_the_mark() {
        let p = scaled_path(&ohlc(&[(50.0, 60.0, 40.0, 45.0)]), 10.0, 7 * HOUR_MS);
        assert_eq!((p[0].o, p[0].h, p[0].l, p[0].c), (10.0, 12.0, 8.0, 9.0));
        assert_eq!(p[0].open_ms, 7 * HOUR_MS);
        let b = beta_path(&ohlc(&[(100.0, 100.0, 80.0, 90.0)]), 2.0, 10.0, 0);
        assert!((b[0].l - 6.0).abs() < 1e-12 && (b[0].c - 8.0).abs() < 1e-12);
    }

    #[test]
    fn coverage_decides_served_and_estimated() {
        let full = hourly(&[100.0; 24]);
        let (inside, ok) = served(&full, 0, 24 * HOUR_MS);
        assert!(ok && inside.len() == 24);
        // Listed half way through: not served, the coin goes to beta.
        let late: Vec<Bar> = full[12..].to_vec();
        assert!(!served(&late, 0, 24 * HOUR_MS).1);
        let beta = Some(BetaInfo { beta: 1.5, raw: 1.5, r2: 0.6, samples: 500, clamped: false });
        let (_, basis) = path_for(None, &full, beta, 1.0, 0).unwrap();
        assert_eq!(basis, "beta");
        // No history and no beta: nothing to walk, the exposure is excluded.
        assert!(path_for(None, &full, None, 1.0, 0).is_none());
        let (_, basis) = path_for(Some(&full), &full, None, 1.0, 0).unwrap();
        assert_eq!(basis, "history");
    }

    #[test]
    fn an_unavailable_window_carries_no_numbers() {
        let w = ReplayWindow::unavailable(&WINDOWS[4], "historyNotAvailable".into());
        assert_eq!(w.status, "unavailable");
        assert!(w.paper.is_none() && w.real.is_none() && w.btc_move_pct.is_none());
        // An empty BTC series has no summary either.
        assert_eq!(btc_summary(&[]), None);
        let (inside, ok) = served(&[], 0, 24 * HOUR_MS);
        assert!(inside.is_empty() && !ok);
    }

    #[test]
    fn replay_book_marks_estimated_coins_and_excludes_unpriced() {
        let a = pos_exposure(long_position(10.0, 500.0, 1.0), 100.0);
        let mut b = pos_exposure(long_position(10.0, 500.0, 1.0), 100.0);
        b.id = "b".into();
        let btc = ohlc(&[(100.0, 100.0, 70.0, 80.0)]);
        let beta = Some(BetaInfo { beta: 1.0, raw: 1.0, r2: 1.0, samples: 500, clamped: false });
        let book = vec![(&a, beta), (&b, None)];
        let r = replay_book(&book, &btc, |_| None, 0);
        assert_eq!((r.included, r.excluded, r.estimated), (1, 1, 1));
        assert_eq!(r.top[0].basis, Some("beta"));
        assert!((r.pnl - (-20.0)).abs() < 1e-9, "{r:?}");
        let (mv, low) = btc_summary(&btc).unwrap();
        assert!((mv + 20.0).abs() < 1e-9 && (low + 30.0).abs() < 1e-9);
    }
}
