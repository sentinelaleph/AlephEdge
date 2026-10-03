//! Tauri command surface for the global risk config (level, simulated
//! balance, daily-stop close preference).

use tauri::{AppHandle, Manager, State};

use super::model::{RiskLevel, RiskState};
use super::RiskManager;

fn data_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|_| "appDataDirUnavailable".to_string())
}

#[tauri::command]
pub fn risk_get(risk: State<RiskManager>) -> RiskState {
    risk.state()
}

#[tauri::command]
pub fn risk_set_level(
    app: AppHandle,
    risk: State<RiskManager>,
    level: RiskLevel,
) -> Result<RiskState, String> {
    Ok(risk.set_level(&data_dir(&app)?, level))
}

#[tauri::command]
pub fn risk_set_balance(
    app: AppHandle,
    risk: State<RiskManager>,
    balance: f64,
) -> Result<RiskState, String> {
    Ok(risk.set_balance(&data_dir(&app)?, balance))
}

#[tauri::command]
pub fn risk_set_close_on_stop(
    app: AppHandle,
    risk: State<RiskManager>,
    close: bool,
) -> Result<RiskState, String> {
    Ok(risk.set_close_on_stop(&data_dir(&app)?, close))
}

/// Sets the user's own daily-loss tolerance, or clears it with `None`.
///
/// The value is stored as typed and clamped at read time, never at write time:
/// if the user later moves to a looser risk level their stricter tolerance must
/// still be there waiting, rather than having been quietly rewritten to
/// whatever the old level's cap happened to be.
#[tauri::command]
pub fn risk_set_daily_loss(
    app: AppHandle,
    risk: State<RiskManager>,
    pct: Option<f64>,
) -> Result<RiskState, String> {
    Ok(risk.set_daily_loss_override(&data_dir(&app)?, pct))
}
