//! Tauri command surface for trade history + performance stats.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};

use super::model::{PnlStats, TradeRecord};
use super::StoreManager;

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|_| "appDataDirUnavailable".to_string())
}

#[tauri::command]
pub fn trades_list(
    app: AppHandle,
    store: State<StoreManager>,
    limit: Option<u32>,
) -> Result<Vec<TradeRecord>, String> {
    store.list_trades(&data_dir(&app)?, limit.unwrap_or(50))
}

/// `live`: Some(true) = real trades only, Some(false) = simulated only,
/// None = both (the pre-live behaviour). Real and simulated money are shown
/// apart once a live build exists.
#[tauri::command]
pub fn trades_stats(
    app: AppHandle,
    store: State<StoreManager>,
    live: Option<bool>,
) -> Result<PnlStats, String> {
    let dir = data_dir(&app)?;
    match live {
        Some(live) => store.stats_for(&dir, live),
        None => store.stats(&dir),
    }
}

/// Writes the CSV next to the database and returns its full path (the UI
/// shows it; no file dialogs in the Rust core).
#[tauri::command]
pub fn trades_export_csv(app: AppHandle, store: State<StoreManager>) -> Result<String, String> {
    let dir = data_dir(&app)?;
    let csv = store.export_csv(&dir)?;
    let path = dir.join("trades-export.csv");
    std::fs::write(&path, csv).map_err(|_| "csvWriteFailed".to_string())?;
    Ok(path.to_string_lossy().to_string())
}
