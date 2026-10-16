//! Tauri surface of the portfolio risk view, and its background pass.

use std::time::Duration;

use tauri::{AppHandle, Manager, State};

use super::whatif::{Candidate, WhatIf};
use super::{ReplayReport, RiskViewManager, RiskViewSnapshot};

/// Between background passes. Each pass reads the desk; betas are cached
/// for hours, so a pass makes no request unless a new coin appeared.
const PASS: Duration = Duration::from_secs(5 * 60);
/// First pass after launch: positions and cycles are restored by then.
const FIRST: Duration = Duration::from_secs(30);
/// A snapshot younger than this answers `riskview_get` without recomputing.
const FRESH_MS: u64 = 60_000;

/// Recomputes the view every PASS for the life of the process, so a
/// threshold crossing alerts (in the app and on Telegram) with no page open.
pub fn spawn(app: &AppHandle) {
    let a = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST).await;
        loop {
            a.state::<RiskViewManager>().refresh(&a).await;
            tokio::time::sleep(PASS).await;
        }
    });
}

/// The latest snapshot; recomputed when `refresh` is set, none exists yet,
/// or the last one is older than a minute.
#[tauri::command]
pub async fn riskview_get(app: AppHandle, refresh: Option<bool>) -> Result<RiskViewSnapshot, String> {
    let mgr = app.state::<RiskViewManager>();
    if !refresh.unwrap_or(false) {
        if let Some(s) = mgr.latest() {
            if super::now_ms().saturating_sub(s.computed_at) < FRESH_MS {
                return Ok(s);
            }
        }
    }
    Ok(mgr.refresh(&app).await)
}

/// Today's open book through every crash window (DataHub candles; cached).
#[tauri::command]
pub async fn riskview_replay(app: AppHandle) -> Result<ReplayReport, String> {
    Ok(app.state::<RiskViewManager>().replay(&app).await)
}

/// Paper-book stress at BTC -20% before and after `candidate`.
#[tauri::command]
pub async fn riskview_whatif(app: AppHandle, mgr: State<'_, RiskViewManager>, candidate: Candidate) -> Result<WhatIf, String> {
    Ok(mgr.whatif(&app, candidate).await)
}
