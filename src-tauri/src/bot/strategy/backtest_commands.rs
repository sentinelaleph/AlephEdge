//! Tauri surface of the strategy backtest (DCA / Grid, paper only).
//!
//! Candles come from DataHub through the Sentinel API with the signed-in
//! session (`market::history`); this file never calls an exchange. The
//! simulation is `backtest::run`, which drives the paper engine's own
//! `driver::on_bar`. Runs are kept locally (newest 50). Errors are i18n
//! codes, `backtest.errors.*` or `strategy.errors.*`, optionally `code|detail`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::market::history::{self, HistoryError};
use crate::membership::MembershipManager;
use crate::risk::RiskManager;
use crate::store::backtest::{self as db, BacktestData, BacktestRun, BacktestRunSummary};
use crate::store::StoreManager;

use super::backtest::{self, interval_ms, plan_pages, MAX_WINDOW_MS};
use super::engine::now_ms;
use super::model::{MarketKind, StrategyConfig, CONFIG_SCHEMA_VERSION};
use super::validate;

pub const PROGRESS_EVENT: &str = "backtest:progress";

/// `run_token`: the caller's token for this run, echoed on every event, so
/// two runs in flight (two windows, a page left and reopened) never mix
/// their progress. Empty when the caller sent none.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress<'a> {
    run_token: &'a str,
    done: u32,
    total: u32,
}

/// At most 64 characters of [A-Za-z0-9_-]; anything else is dropped.
fn clean_token(raw: Option<String>) -> String {
    raw.unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect()
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|_| "app data dir unavailable".to_string())
}

/// "bt_" + 12 lowercase base32 characters.
fn new_run_id() -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let bytes = uuid::Uuid::new_v4().into_bytes();
    let id: String = bytes
        .iter()
        .take(12)
        .map(|b| ALPHABET[usize::from(*b) % 32] as char)
        .collect();
    format!("bt_{id}")
}

/// Window aligned to the interval grid and clipped to now. Err = i18n code.
pub fn window(start_ms: u64, end_ms: u64, iv: u64, now: u64) -> Result<(u64, u64), &'static str> {
    let start = start_ms - start_ms % iv;
    let end = end_ms.min(now);
    let end = end - end % iv;
    if end <= start {
        return Err("windowInvalid");
    }
    if end - start > MAX_WINDOW_MS {
        return Err("windowTooLong");
    }
    Ok((start, end))
}

/// Fetches every page of the window. One 401 refreshes the session through
/// the membership manager (the app's only credential path) and retries.
#[allow(clippy::too_many_arguments)]
async fn fetch_all(
    app: &AppHandle,
    symbol: &str,
    market: MarketKind,
    interval: &str,
    iv: u64,
    start: u64,
    end: u64,
    run_token: &str,
) -> Result<(Vec<backtest::Candle>, BacktestData), String> {
    let membership = app.state::<MembershipManager>();
    let base = membership.base_url();
    let mut token = membership.fresh_access_token().await.ok_or("historySignIn")?;
    let client = history::build_client();
    let pages = plan_pages(start, end, iv);
    let total = pages.len() as u32;
    let mut candles = Vec::new();
    let mut sources = BTreeSet::new();
    let (mut server_count, mut server_expected, mut server_complete) = (0u64, 0u64, true);
    let mut single_pct: Option<f64> = None;
    let mut unparsed = 0u32;
    let mut refreshed = false;
    let _ = app.emit(PROGRESS_EVENT, Progress { run_token, done: 0, total });
    for (i, (from, to)) in pages.iter().enumerate() {
        let (from_s, to_s) = (backtest::rfc3339(*from), backtest::rfc3339(*to));
        let page = loop {
            let got = history::fetch_page(&client, &base, &token, symbol, market.as_str(), interval, &from_s, &to_s).await;
            match got {
                Err(HistoryError::Unauthorized) if !refreshed => {
                    refreshed = true;
                    if !membership.refresh_after_unauthorized(&token).await {
                        return Err("historyUnauthorized".into());
                    }
                    token = membership.access_token().ok_or("historySignIn")?;
                }
                other => break other.map_err(|e| e.to_code())?,
            }
        };
        if let Some(s) = page.source {
            sources.insert(s);
        }
        if total == 1 {
            single_pct = page.coverage.and_then(|c| c.coverage_pct).filter(|p| p.is_finite());
        }
        match page.coverage {
            Some(c) if c.candle_count.is_some() && c.expected_candles.is_some() => {
                server_count += c.candle_count.unwrap_or(0);
                server_expected += c.expected_candles.unwrap_or(0);
            }
            _ => server_complete = false,
        }
        unparsed += page.unparsed;
        candles.extend(page.candles);
        let _ = app.emit(PROGRESS_EVENT, Progress { run_token, done: i as u32 + 1, total });
    }
    let data = BacktestData {
        source: sources.into_iter().collect::<Vec<_>>().join(", "),
        candle_count: 0,
        expected_candles: (server_complete && server_expected > 0).then_some(server_expected),
        // One page: the server's own figure; several: their summed counts.
        coverage_pct: single_pct.or_else(|| {
            (server_complete && server_expected > 0)
                .then(|| (server_count as f64 / server_expected as f64 * 100.0).min(100.0))
        }),
        missing_bars: 0,
        dropped_candles: unparsed,
        requests: total,
        funding_included: false,
    };
    Ok((candles, data))
}

/// Runs a DCA or Grid config over DataHub candles of `symbol` on `market`,
/// `interval` bars, window `[start_ms, end_ms)`. Validated exactly like bot
/// creation (leverage ceiling of the current risk level included). Paper
/// only: no exchange call, no order.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn backtest_run(
    app: AppHandle,
    config: StrategyConfig,
    symbol: String,
    market: MarketKind,
    interval: String,
    start_ms: u64,
    end_ms: u64,
    run_token: Option<String>,
) -> Result<BacktestRun, String> {
    let run_token = clean_token(run_token);
    let iv = interval_ms(&interval).ok_or("intervalInvalid")?;
    let now = now_ms();
    let (start, end) = window(start_ms, end_ms, iv, now).map_err(str::to_string)?;
    let cfg = StrategyConfig {
        schema_version: CONFIG_SCHEMA_VERSION,
        name: config.name.trim().to_string(),
        symbol: symbol.trim().to_uppercase(),
        market,
        ..config
    };
    let max_lev = app.state::<RiskManager>().limits().max_leverage;
    validate::validate(&cfg, max_lev).map_err(|e| e.to_string())?;

    let (candles, mut data) = fetch_all(&app, &cfg.symbol, market, &interval, iv, start, end, &run_token).await?;
    let (bars, dropped) = backtest::candles_to_bars(candles, iv, now);
    let bars: Vec<_> = bars.into_iter().filter(|b| b.open_ms >= start && b.open_ms < end).collect();
    if bars.is_empty() {
        return Err("historyEmpty".into());
    }
    data.candle_count = bars.len() as u64;
    data.dropped_candles += dropped;
    data.missing_bars = backtest::missing_bars(&bars, iv);
    if data.coverage_pct.is_none() {
        // The server sent no coverage: the window's own grid is the expectation.
        let expected = (end - start) / iv;
        data.expected_candles = Some(expected);
        data.coverage_pct = Some((bars.len() as f64 / expected as f64 * 100.0).min(100.0));
    }

    let result = backtest::run(&cfg, &bars).map_err(str::to_string)?;
    let run = BacktestRun {
        id: new_run_id(),
        created_at: now,
        symbol: cfg.symbol.clone(),
        market,
        config: cfg,
        interval,
        start_ms: start,
        end_ms: end,
        data,
        result,
    };
    let dir = data_dir(&app)?;
    app.state::<StoreManager>().backtests(&dir, |c| db::insert(c, &run))?;
    Ok(run)
}

/// Stored runs, newest first (at most 50).
#[tauri::command]
pub fn backtest_list(app: AppHandle) -> Result<Vec<BacktestRunSummary>, String> {
    let dir = data_dir(&app)?;
    app.state::<StoreManager>().backtests(&dir, db::list)
}

#[tauri::command]
pub fn backtest_get(app: AppHandle, id: String) -> Result<BacktestRun, String> {
    let dir = data_dir(&app)?;
    app.state::<StoreManager>()
        .backtests(&dir, |c| db::get(c, &id))?
        .ok_or_else(|| "runNotFound".to_string())
}

#[tauri::command]
pub fn backtest_delete(app: AppHandle, id: String) -> Result<(), String> {
    let dir = data_dir(&app)?;
    if app.state::<StoreManager>().backtests(&dir, |c| db::delete(c, &id))? {
        Ok(())
    } else {
        Err("runNotFound".into())
    }
}

/// Empties the run list. Returns how many runs were removed.
#[tauri::command]
pub fn backtest_delete_all(app: AppHandle) -> Result<usize, String> {
    let dir = data_dir(&app)?;
    app.state::<StoreManager>().backtests(&dir, db::delete_all)
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: u64 = 3_600_000;

    #[test]
    fn window_is_aligned_clipped_and_bounded() {
        let now = 1_736_121_600_000 + 30 * 60_000; // 00:30
        assert_eq!(window(now - 10 * H + 5, now + H, H, now), Ok((now - 30 * 60_000 - 10 * H, now - 30 * 60_000)));
        assert_eq!(window(now, now, H, now), Err("windowInvalid"));
        assert_eq!(window(0, now, H, now), Err("windowTooLong"));
        assert!(window(now - 2 * 366 * 86_400_000, now, 86_400_000, now).is_ok());
    }

    #[test]
    fn progress_events_carry_the_run_token() {
        let token = clean_token(Some("rt-1a2b_C3".into()));
        let v = serde_json::to_value(Progress { run_token: &token, done: 2, total: 5 }).unwrap();
        assert_eq!(v, serde_json::json!({ "runToken": "rt-1a2b_C3", "done": 2, "total": 5 }));
        assert_eq!(clean_token(None), "");
        assert_eq!(clean_token(Some("a\"b<c>".into())), "abc");
        assert_eq!(clean_token(Some("x".repeat(200))).len(), 64);
    }

    #[test]
    fn run_ids_have_the_documented_shape() {
        let id = new_run_id();
        assert!(id.starts_with("bt_") && id.len() == 15);
        assert!(id[3..].chars().all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c)));
    }
}
