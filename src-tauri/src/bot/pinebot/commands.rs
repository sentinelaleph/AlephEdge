//! Tauri commands of the Pine bot's screens. Saving goes through the desk's
//! own `apply_config` (the same checks as `bot_configure`), starting and
//! stopping through `BotManager`, the Live arm through `bot_set_live` with
//! kind `pine` (a bot-level stop is required there, D4).

use tauri::{AppHandle, Manager, State};

use crate::bot::model::{BotConfig, BotKind, TradeMode};
use crate::bot::{apply_config, BotManager, KeepLive};
use crate::pine::CompileInfo;
use crate::risk::RiskManager;
use crate::store::StoreManager;

use super::rules;
use super::types::{PineResult, PineStatus};
use super::PineManager;

/// Most result rows one call returns.
pub const MAX_RESULTS: u32 = 500;

/// Compiles a script (off the main thread): its inputs, triggers and hash,
/// or the engine's named error. Nothing is saved.
#[tauri::command]
pub async fn pine_compile(source: String) -> Result<CompileInfo, String> {
    if source.len() > crate::bot::model::pine_settings::PINE_MAX_SCRIPT_BYTES {
        return Err(format!("pineScriptTooLarge|{}", crate::bot::model::pine_settings::PINE_MAX_SCRIPT_BYTES / 1024));
    }
    tokio::task::spawn_blocking(move || crate::pine::compile(&source)).await.map_err(|_| "pineCompileFailed".to_string())
}

/// A new Pine bot's defaults (Binance, Paper, capital fitted under the cap).
#[tauri::command]
pub fn pine_default_config(risk: State<RiskManager>) -> BotConfig {
    BotConfig::new_tradable_for_desk(BotKind::Pine, "binance", risk.state().max_capital_quote)
}

/// The saved Pine bot, if one was made.
#[tauri::command]
pub fn pine_get_config(bots: State<BotManager>) -> Option<BotConfig> {
    bots.config_for(BotKind::Pine)
}

/// Saves the Pine bot (`bot_configure`'s checks). An edit of the script, its
/// triggers, inputs, bars, exits or money on an armed bot disarms and stops
/// it (`liveDisarmedStopped`).
#[tauri::command]
pub fn pine_save_config(app: AppHandle, bots: State<BotManager>, config: BotConfig) -> Result<BotConfig, String> {
    if config.kind != BotKind::Pine {
        return Err("pineBotKind".to_string());
    }
    apply_config(&app, &bots, config, KeepLive::Yes)?;
    bots.config_for(BotKind::Pine).ok_or_else(|| "botNotConfigured".to_string())
}

/// Starts the Pine bot (a Live one needs its typed LIVE first); a start
/// ends a halt.
#[tauri::command]
pub async fn pine_start(app: AppHandle) -> Result<PineStatus, String> {
    app.state::<BotManager>().start(app.clone(), BotKind::Pine)?;
    Ok(status(&app))
}

/// Stops new entries; open positions stay managed.
#[tauri::command]
pub async fn pine_stop(app: AppHandle) -> Result<PineStatus, String> {
    app.state::<BotManager>().stop(&app, BotKind::Pine);
    Ok(status(&app))
}

#[tauri::command]
pub async fn pine_status(app: AppHandle) -> Result<PineStatus, String> {
    Ok(status(&app))
}

/// Action rows, newest first (at most `limit`, default 100, max 500).
#[tauri::command]
pub async fn pine_results(app: AppHandle, limit: Option<u32>) -> Result<Vec<PineResult>, String> {
    let dir = app.path().app_data_dir().map_err(|_| "appDataDirUnavailable".to_string())?;
    let bodies = app.state::<StoreManager>().pine_results(&dir, limit.unwrap_or(100).clamp(1, MAX_RESULTS))?;
    Ok(bodies.iter().filter_map(|b| serde_json::from_str(b).ok()).collect())
}

/// The bot card: per symbol the last run, the halt, entries today / max.
pub fn status(app: &AppHandle) -> PineStatus {
    let bots = app.state::<BotManager>();
    let mgr = app.state::<PineManager>();
    let Some(cfg) = bots.config_for(BotKind::Pine) else {
        return PineStatus::default();
    };
    let p = cfg.pine.clone().unwrap_or_default();
    let dir = app.path().app_data_dir().ok();
    let store = app.state::<StoreManager>();
    let now = crate::bot::engine::now_ms();
    let opens_today = dir.as_ref().and_then(|d| store.pine_opens_since(d, crate::tvlink::guard::utc_day(now)).ok()).unwrap_or(0);
    let pending = dir.as_ref().and_then(|d| store.pine_pending(d).ok()).map_or(0, |v| v.len());
    let halted = mgr.halted(app);
    let g = mgr.lock();
    let symbols = p
        .symbols
        .iter()
        .map(|s| g.symbols.get(s).cloned().unwrap_or_else(|| super::types::PineSymbolStatus { symbol: s.clone(), ..Default::default() }))
        .collect();
    let iv = p.timeframe.interval_ms();
    PineStatus {
        configured: true,
        running: bots.running_config(BotKind::Pine).is_some(),
        mode: Some(if cfg.mode == TradeMode::Live { "live" } else { "paper" }.to_string()),
        armed: cfg.live,
        script_hash: Some(p.script_hash.clone()),
        timeframe: Some(p.timeframe.as_str().to_string()),
        halted,
        failed_in_a_row: g.streak.0,
        opens_today,
        max_opens_per_day: p.max_opens_per_day,
        pending,
        pilot_left: bots.pilot_left(BotKind::Pine),
        next_bar_close: Some(rules::last_close(now, iv) + iv),
        symbols,
    }
}
