//! Market trend panel (`market_trend`): BTC's slow trend, shown as
//! information only. No bot reads it and nothing trades on it.
//!
//! Definition, as in the 8 Oct 2026 pre-registered test (trend50):
//! daily BTCUSDT close vs its 50-day average; the confirmed state switches only
//! after 2 consecutive closes on the other side. That test FAILED as a trading
//! rule, so the app shows the state and its history and lets the user decide.
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

fn cache() -> &'static Mutex<Option<MarketTrend>> {
    static C: OnceLock<Mutex<Option<MarketTrend>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
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

#[tauri::command]
pub async fn market_trend() -> Result<MarketTrend, String> {
    let now = crate::bot::engine::now_ms();
    if let Some(c) = cache().lock().expect("trend cache").clone() {
        if now.saturating_sub(c.checked_at_ms) < CACHE_MS {
            return Ok(c);
        }
    }
    let fresh = fetch(now).await?;
    *cache().lock().expect("trend cache") = Some(fresh.clone());
    Ok(fresh)
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
