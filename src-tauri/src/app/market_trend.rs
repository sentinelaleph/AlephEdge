//! Market trend panel (`market_trend`): BTC's slow trend. Shown as
//! information; the only reader is the opt-in strategy gate
//! `pause_in_btc_downtrend` (via `cached_up`), which holds new cycles.
//!
//! Definition: daily BTCUSDT close vs its 50-day average; the confirmed state
//! switches only after 2 consecutive closes on the other side. Tested as a
//! trading rule it showed no edge, so the app shows the state and its history
//! and lets the user decide.
//!
//! Source: Binance spot daily klines (public, keyless, production even in a
//! testnet build: testnet prices are not the market).

use std::sync::{Mutex, OnceLock};

use serde::Serialize;

use crate::exchange::providers::klines::{parse_klines, Kline};

const SMA_D: usize = 50;
const CONFIRM: usize = 2;
const DAYS: u16 = 420;
const CACHE_MS: u64 = 15 * 60_000;
const DAY_MS: u64 = 86_400_000;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketTrend {
    /// "up" or "down".
    pub state: &'static str,
    /// When the current state became known (00:00 UTC after the confirming close).
    pub since_ms: u64,
    pub days: u32,
    pub close: f64,
    pub sma50: f64,
    /// close / sma50 - 1, in percent.
    pub gap_pct: f64,
    /// Confirmed switches in the last 365 days.
    pub switches_365d: u32,
    pub checked_at_ms: u64,
}

/// The confirmed trend over closed daily bars (oldest first). None until 50
/// closes exist.
pub fn confirmed_trend(bars: &[Kline], now_ms: u64) -> Option<MarketTrend> {
    if bars.len() < SMA_D {
        return None;
    }
    let mut state: Option<bool> = None;
    let mut run: (Option<bool>, usize) = (None, 0);
    let mut since = 0u64;
    let mut switch_times: Vec<u64> = Vec::new();
    let mut last = (0.0, 0.0);
    for i in (SMA_D - 1)..bars.len() {
        let sma = bars[i + 1 - SMA_D..=i].iter().map(|k| k.c).sum::<f64>() / SMA_D as f64;
        let raw = bars[i].c > sma;
        let known = bars[i].close_ms + 1;
        run = if run.0 == Some(raw) { (run.0, run.1 + 1) } else { (Some(raw), 1) };
        match state {
            None => {
                state = Some(raw);
                since = known;
            }
            Some(s) if s != raw && run.1 >= CONFIRM => {
                state = Some(raw);
                since = known;
                switch_times.push(known);
            }
            _ => {}
        }
        last = (bars[i].c, sma);
    }
    let up = state?;
    Some(MarketTrend {
        state: if up { "up" } else { "down" },
        since_ms: since,
        days: (now_ms.saturating_sub(since) / DAY_MS) as u32,
        close: last.0,
        sma50: last.1,
        gap_pct: (last.0 / last.1 - 1.0) * 100.0,
        switches_365d: switch_times.iter().filter(|&&t| t + 365 * DAY_MS >= now_ms).count() as u32,
        checked_at_ms: now_ms,
    })
}

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_default()
    })
}

/// After a failed read no new read is tried for this long. The strategy
/// engine awaits the refresh inside its serial tick, so an unreachable
/// api.binance.com must cost one timeout per window, not one per tick. The
/// gate stays closed meanwhile (`cached_up` goes unknown after a day).
const RETRY_AFTER_FAIL_MS: u64 = 5 * 60_000;

/// The last good read and when the last read failed.
#[derive(Debug, Clone, Default)]
struct TrendState {
    cache: Option<MarketTrend>,
    failed_at_ms: Option<u64>,
}

/// What a call does now: answer from the cache, refuse inside the backoff
/// window, or read.
#[derive(Debug, Clone, PartialEq)]
enum Plan {
    Cached(MarketTrend),
    Backoff,
    Fetch,
}

fn plan(s: &TrendState, now_ms: u64) -> Plan {
    if let Some(c) = &s.cache {
        if now_ms.saturating_sub(c.checked_at_ms) < CACHE_MS {
            return Plan::Cached(c.clone());
        }
    }
    match s.failed_at_ms {
        Some(at) if now_ms.saturating_sub(at) < RETRY_AFTER_FAIL_MS => Plan::Backoff,
        _ => Plan::Fetch,
    }
}

fn record(s: &mut TrendState, now_ms: u64, res: &Result<MarketTrend, String>) {
    match res {
        Ok(t) => {
            s.cache = Some(t.clone());
            s.failed_at_ms = None;
        }
        Err(_) => s.failed_at_ms = Some(now_ms),
    }
}

fn state() -> &'static Mutex<TrendState> {
    static C: OnceLock<Mutex<TrendState>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(TrendState::default()))
}

/// One call against `st`: the cache, the backoff refusal, or one `fetch`
/// whose outcome is recorded (a failure starts the backoff window).
async fn trend_with<F, Fut>(st: &Mutex<TrendState>, now_ms: u64, fetch: F) -> Result<MarketTrend, String>
where
    F: FnOnce(u64) -> Fut,
    Fut: std::future::Future<Output = Result<MarketTrend, String>>,
{
    let p = plan(&st.lock().expect("trend cache"), now_ms);
    match p {
        Plan::Cached(c) => Ok(c),
        Plan::Backoff => Err("trendUnavailable".to_string()),
        Plan::Fetch => {
            let res = fetch(now_ms).await;
            record(&mut st.lock().expect("trend cache"), now_ms, &res);
            res
        }
    }
}

async fn fetch(now_ms: u64) -> Result<MarketTrend, String> {
    let res = client()
        .get("https://api.binance.com/api/v3/klines")
        .query(&[("symbol", "BTCUSDT"), ("interval", "1d"), ("limit", &DAYS.to_string())])
        .send()
        .await
        .map_err(|_| "trendUnavailable".to_string())?;
    let body: serde_json::Value = res.json().await.map_err(|_| "trendUnavailable".to_string())?;
    let bars = parse_klines(&body, now_ms).map_err(|_| "trendUnavailable".to_string())?;
    confirmed_trend(&bars, now_ms).ok_or_else(|| "trendUnavailable".to_string())
}

/// The cached trend for the strategy gate: Some(up) when read within the
/// last day, else None (unknown). No I/O.
pub fn cached_up(now_ms: u64) -> Option<bool> {
    cached_up_in(state(), now_ms)
}

fn cached_up_in(st: &Mutex<TrendState>, now_ms: u64) -> Option<bool> {
    let c = st.lock().expect("trend cache").cache.clone()?;
    (now_ms.saturating_sub(c.checked_at_ms) <= DAY_MS).then_some(c.state == "up")
}

#[tauri::command]
pub async fn market_trend() -> Result<MarketTrend, String> {
    trend_with(state(), crate::bot::engine::now_ms(), fetch).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bars(closes: &[f64]) -> Vec<Kline> {
        closes
            .iter()
            .enumerate()
            .map(|(i, &c)| {
                let open_ms = i as u64 * DAY_MS;
                Kline { open_ms, close_ms: open_ms + DAY_MS - 1, o: c, h: c, l: c, c }
            })
            .collect()
    }

    fn sample(at: u64) -> MarketTrend {
        MarketTrend {
            state: "down",
            since_ms: 0,
            days: 0,
            close: 1.0,
            sma50: 1.0,
            gap_pct: 0.0,
            switches_365d: 0,
            checked_at_ms: at,
        }
    }

    #[test]
    fn a_failed_read_is_not_retried_inside_the_backoff_window() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let st = Mutex::new(TrendState::default());
        let calls = AtomicU32::new(0);
        let failing = |_: u64| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err::<MarketTrend, String>("trendUnavailable".into()) }
        };
        let t0 = 10 * DAY_MS;
        let run = |now: u64| tauri::async_runtime::block_on(trend_with(&st, now, failing));
        assert!(run(t0).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1, "the first tick reads");
        // Engine ticks every few seconds inside the window: no read, gate unknown.
        for dt in [1_000, 30_000, 60_000, RETRY_AFTER_FAIL_MS - 1] {
            assert_eq!(run(t0 + dt), Err("trendUnavailable".to_string()));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1, "no read inside the backoff window");
        assert_eq!(cached_up_in(&st, t0 + 60_000), None);
        assert!(run(t0 + RETRY_AFTER_FAIL_MS).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2, "one retry after the window");
    }

    #[test]
    fn a_good_read_clears_the_backoff_and_is_cached() {
        let st = Mutex::new(TrendState::default());
        let t0 = 10 * DAY_MS;
        record(&mut st.lock().unwrap(), t0, &Err("x".into()));
        assert_eq!(plan(&st.lock().unwrap(), t0 + 1), Plan::Backoff);
        let ok = tauri::async_runtime::block_on(trend_with(&st, t0 + RETRY_AFTER_FAIL_MS, |now| async move { Ok(sample(now)) }));
        assert!(ok.is_ok());
        let s = st.lock().unwrap().clone();
        assert_eq!(s.failed_at_ms, None);
        assert!(matches!(plan(&s, t0 + RETRY_AFTER_FAIL_MS + 60_000), Plan::Cached(_)));
        // A stale cache whose refresh fails keeps the old read for the gate (fails closed after a day).
        let mut s2 = s.clone();
        record(&mut s2, t0 + RETRY_AFTER_FAIL_MS + CACHE_MS, &Err("x".into()));
        assert_eq!(plan(&s2, t0 + RETRY_AFTER_FAIL_MS + CACHE_MS + 1), Plan::Backoff);
        assert_eq!(cached_up_in(&Mutex::new(s2), t0 + RETRY_AFTER_FAIL_MS + DAY_MS + 1), None);
    }

    #[test]
    fn needs_fifty_closes() {
        assert!(confirmed_trend(&bars(&[1.0; 49]), 0).is_none());
    }

    #[test]
    fn one_close_below_does_not_switch_two_do() {
        let mut c: Vec<f64> = (0..60).map(|i| 100.0 + i as f64).collect();
        let now = 70 * DAY_MS;
        assert_eq!(confirmed_trend(&bars(&c), now).unwrap().state, "up");
        c.push(50.0); // one close far below the average
        let t = confirmed_trend(&bars(&c), now).unwrap();
        assert_eq!(t.state, "up", "a single close must not flip the state");
        c.push(50.0);
        let t = confirmed_trend(&bars(&c), now).unwrap();
        assert_eq!(t.state, "down");
        assert_eq!(t.since_ms, 62 * DAY_MS, "known at 00:00 after the confirming close");
        assert_eq!(t.switches_365d, 1);
        assert!(t.gap_pct < 0.0);
    }

    #[test]
    fn a_flip_back_needs_two_closes_too() {
        let mut c: Vec<f64> = (0..60).map(|i| 100.0 + i as f64).collect();
        c.extend([50.0, 50.0, 500.0]);
        let t = confirmed_trend(&bars(&c), 70 * DAY_MS).unwrap();
        assert_eq!(t.state, "down");
        c.push(500.0);
        assert_eq!(confirmed_trend(&bars(&c), 70 * DAY_MS).unwrap().state, "up");
    }
}
