//! Tauri commands of the Assistant page. None returns the key.

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;
use zeroize::Zeroize;

use super::openrouter::{self, ModelInfo, OpenRouter};
use super::prompt::{self, PromptVersion};
use super::tools::ToolData;
use super::{append, context, delete_key, read_key, write_key, ApiKey, AssistantManager, ChatEntry, Settings, CONVERSATION_KEY, MAX_MESSAGE_CHARS, SETTINGS_KEY};
use crate::app::keychain::OsKeychain;
use crate::bot::model::LIVE_TRADING_ENABLED;
use crate::bot::strategy::model::StrategyParams;
use crate::bot::strategy::StrategyManager;
use crate::bot::BotManager;
use crate::signal::SignalManager;
use crate::store::StoreManager;

/// Where to get a key (opened by `assistant_open_keys_page`).
pub const KEYS_URL: &str = "https://openrouter.ai/keys";
const MODELS_TTL_MS: u64 = 3_600_000;

fn now_ms() -> u64 {
    crate::bot::engine::now_ms()
}

fn dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|_| "appDataDirUnavailable".to_string())
}

fn load_settings(app: &AppHandle) -> Settings {
    dir(app)
        .ok()
        .and_then(|d| app.state::<StoreManager>().get_meta(&d, SETTINGS_KEY).ok().flatten())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_settings(app: &AppHandle, s: &Settings) -> Result<(), String> {
    let body = serde_json::to_string(s).map_err(|_| "storeWriteFailed".to_string())?;
    app.state::<StoreManager>().set_meta(&dir(app)?, SETTINGS_KEY, &body)
}

fn load_conversation(app: &AppHandle) -> Vec<ChatEntry> {
    dir(app)
        .ok()
        .and_then(|d| app.state::<StoreManager>().get_meta(&d, CONVERSATION_KEY).ok().flatten())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_conversation(app: &AppHandle, conv: &[ChatEntry]) -> Result<(), String> {
    let body = serde_json::to_string(conv).map_err(|_| "storeWriteFailed".to_string())?;
    app.state::<StoreManager>().set_meta(&dir(app)?, CONVERSATION_KEY, &body)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantStatus {
    pub key_present: bool,
    pub model: Option<String>,
    pub prompt: String,
    pub prompt_is_default: bool,
    pub default_prompt: &'static str,
    /// The fixed rules sent before the master prompt (shown, not editable).
    pub guard: &'static str,
    pub prompt_version: &'static str,
    pub history: Vec<PromptVersion>,
    pub conversation: Vec<ChatEntry>,
    pub busy: bool,
    pub tool_names: Vec<&'static str>,
}

fn status(app: &AppHandle) -> AssistantStatus {
    let s = load_settings(app);
    AssistantStatus {
        key_present: read_key(&OsKeychain).is_some(),
        model: s.model,
        prompt_is_default: s.prompt.is_default(),
        prompt: s.prompt.text,
        default_prompt: prompt::DEFAULT_PROMPT,
        guard: prompt::GUARD,
        prompt_version: prompt::PROMPT_VERSION,
        history: s.prompt.history,
        conversation: load_conversation(app),
        busy: app.state::<AssistantManager>().busy.load(Ordering::SeqCst),
        tool_names: super::tools::TOOL_NAMES.to_vec(),
    }
}

#[tauri::command]
pub fn assistant_status(app: AppHandle) -> AssistantStatus {
    status(&app)
}

#[tauri::command]
pub fn assistant_set_key(app: AppHandle, mut key: String) -> Result<AssistantStatus, String> {
    let parsed = ApiKey::parse(&key);
    key.zeroize();
    let k = parsed?;
    write_key(&OsKeychain, &k)?;
    Ok(status(&app))
}

#[tauri::command]
pub fn assistant_remove_key(app: AppHandle) -> AssistantStatus {
    delete_key(&OsKeychain);
    status(&app)
}

#[tauri::command]
pub async fn assistant_models(mgr: State<'_, AssistantManager>, refresh: Option<bool>) -> Result<Vec<ModelInfo>, String> {
    let now = now_ms();
    if !refresh.unwrap_or(false) {
        if let Some((at, m)) = mgr.models.lock().expect("models cache").clone() {
            if now.saturating_sub(at) < MODELS_TTL_MS {
                return Ok(m);
            }
        }
    }
    let m = openrouter::fetch_models().await?;
    *mgr.models.lock().expect("models cache") = Some((now, m.clone()));
    Ok(m)
}

#[tauri::command]
pub fn assistant_set_model(app: AppHandle, model: String) -> Result<AssistantStatus, String> {
    let model = model.trim().to_string();
    let ok = !model.is_empty() && model.len() <= 200 && model.bytes().all(|b| b.is_ascii_graphic());
    if !ok {
        return Err("assistantModelInvalid".into());
    }
    let mut s = load_settings(&app);
    s.model = Some(model);
    save_settings(&app, &s)?;
    Ok(status(&app))
}

#[tauri::command]
pub fn assistant_save_prompt(app: AppHandle, text: String) -> Result<AssistantStatus, String> {
    let mut s = load_settings(&app);
    s.prompt.save(&text, now_ms())?;
    save_settings(&app, &s)?;
    Ok(status(&app))
}

#[tauri::command]
pub fn assistant_reset_prompt(app: AppHandle) -> Result<AssistantStatus, String> {
    let mut s = load_settings(&app);
    s.prompt.reset(now_ms());
    save_settings(&app, &s)?;
    Ok(status(&app))
}

#[tauri::command]
pub fn assistant_restore_prompt(app: AppHandle, saved_at_ms: u64) -> Result<AssistantStatus, String> {
    let mut s = load_settings(&app);
    s.prompt.restore(saved_at_ms, now_ms())?;
    save_settings(&app, &s)?;
    Ok(status(&app))
}

#[tauri::command]
pub fn assistant_clear_conversation(app: AppHandle) -> Result<AssistantStatus, String> {
    save_conversation(&app, &[])?;
    Ok(status(&app))
}

#[tauri::command]
pub fn assistant_open_keys_page(app: AppHandle) -> Result<(), String> {
    app.opener().open_url(KEYS_URL, None::<&str>).map_err(|_| "linkOpenFailed".to_string())
}

/// Clears `busy` however the turn ends.
struct BusyGuard<'a>(&'a AssistantManager);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::SeqCst);
    }
}

#[tauri::command]
pub async fn assistant_send(app: AppHandle, mgr: State<'_, AssistantManager>, text: String) -> Result<AssistantStatus, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("assistantMessageEmpty".into());
    }
    if text.chars().count() > MAX_MESSAGE_CHARS {
        return Err(format!("assistantMessageTooLong|{MAX_MESSAGE_CHARS}"));
    }
    let key = read_key(&OsKeychain).ok_or("assistantNoKey")?;
    let settings = load_settings(&app);
    let model = settings.model.clone().ok_or("assistantNoModel")?;
    if mgr.busy.swap(true, Ordering::SeqCst) {
        return Err("assistantBusy".into());
    }
    let _guard = BusyGuard(mgr.inner());

    let mut conv = load_conversation(&app);
    let history = context(&conv);
    let user = ChatEntry::user(&text, now_ms());
    let data = AppTools::gather(&app).await;
    let system = prompt::system_message(&settings.prompt.text);
    let turn = openrouter::run_turn(&OpenRouter { key }, &data, &model, &system, history, &text).await;
    let answer = match turn {
        Ok(r) => ChatEntry {
            role: "assistant".into(),
            text: r.text,
            at_ms: now_ms(),
            model: Some(r.model),
            usage: Some(r.usage),
            tools: r.tools,
            drafts: r.drafts,
            error: None,
            truncated: r.truncated,
        },
        Err(code) => ChatEntry {
            role: "assistant".into(),
            text: String::new(),
            at_ms: now_ms(),
            model: Some(model),
            usage: None,
            tools: vec![],
            drafts: vec![],
            error: Some(code),
            truncated: false,
        },
    };
    append(&mut conv, [user, answer]);
    save_conversation(&app, &conv)?;
    drop(_guard);
    Ok(status(&app))
}

/// The desk's data for one turn, read when the message is sent. Holds
/// handles and copies only: nothing here can change a bot or an order.
pub struct AppTools {
    app: AppHandle,
    byt: Value,
    risk: Value,
}

fn unavailable(code: &str) -> Value {
    json!({ "status": "unavailable", "reason": code })
}

impl AppTools {
    pub async fn gather(app: &AppHandle) -> Self {
        // "en": the English lines, without every locale's translations.
        let byt = match crate::app::before_you_trade::current(app, false, Some("en")).await {
            Ok(v) => json!({ "fetched_at_ms": v.fetched_at_ms, "stale": v.stale, "fetch_error": v.error, "payload": v.payload }),
            Err(e) => unavailable(&e),
        };
        let risk = match crate::riskview::riskview_get(app.clone(), None).await {
            Ok(s) => serde_json::to_value(s).unwrap_or_else(|_| unavailable("serialize")),
            Err(e) => unavailable(&e),
        };
        Self { app: app.clone(), byt, risk }
    }
}

fn to_json<T: Serialize>(r: Result<T, String>) -> Value {
    match r {
        Ok(v) => serde_json::to_value(v).unwrap_or_else(|_| unavailable("serialize")),
        Err(e) => unavailable(&e),
    }
}

impl ToolData for AppTools {
    fn bots(&self) -> Value {
        let s = self.app.state::<BotManager>().status();
        json!({
            "live_trading_build": s.live_trading_enabled,
            "kill_switch_tripped": s.kill_switch_tripped,
            "btc_regime": s.btc_regime,
            "signal_bots": [
                { "kind": "futures", "configured": s.futures.is_some(), "running": s.futures_running, "config": s.futures },
                { "kind": "spot", "configured": s.spot.is_some(), "running": s.spot_running, "config": s.spot },
                { "kind": "pump", "configured": s.pump.is_some(), "running": s.pump_running, "config": s.pump, "note": "paper only, untested" },
            ],
            "strategy_bots": self.app.state::<StrategyManager>().views(),
        })
    }

    fn positions(&self) -> Value {
        let s = self.app.state::<BotManager>().status();
        let strategy: Vec<Value> = self
            .app
            .state::<StrategyManager>()
            .views()
            .into_iter()
            .filter_map(|v| serde_json::to_value(v).ok())
            .collect();
        json!({ "signal_positions": s.open_positions, "strategy_bots": strategy })
    }

    fn pnl(&self) -> Value {
        let Ok(d) = dir(&self.app) else { return unavailable("appDataDirUnavailable") };
        let store = self.app.state::<StoreManager>();
        json!({
            "signal_paper": to_json(store.stats_for(&d, false)),
            "signal_live": if LIVE_TRADING_ENABLED { to_json(store.stats_for(&d, true)) } else { json!({ "status": "not_in_this_build" }) },
            "signal_by_bot": to_json(store.stats_by_bot(&d, None)),
            "strategy_paper": to_json(crate::bot::strategy::commands::strategy_pnl(self.app.clone(), Some(10))),
            "note": "paper and live are separate; never add them"
        })
    }

    fn risk(&self) -> Value {
        self.risk.clone()
    }

    fn before_you_trade(&self) -> Value {
        // Wrapping applies to the payload's own fields.
        let mut v = self.byt.clone();
        if let Some(p) = v.get_mut("payload") {
            *p = super::tools::wrap_untrusted(p.take());
        }
        v
    }

    fn signals(&self) -> Value {
        let mut list = self.app.state::<SignalManager>().recent();
        list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Value::Array(
            list.into_iter()
                .take(30)
                .map(|s| {
                    json!({ "id": s.id, "symbol": s.symbol, "market": s.market_type, "timeframe": s.timeframe,
                        "direction": s.direction, "entry": s.entry, "stop": s.sl, "targets": s.tp,
                        "confidence": s.confidence, "rr": s.rr, "created_at": s.created_at, "expires_at": s.expires_at })
                })
                .collect(),
        )
    }

    fn presets(&self) -> Vec<(String, String)> {
        crate::bot::strategy::presets::all()
            .into_iter()
            .map(|p| {
                let kind = match p.config.params {
                    StrategyParams::Dca(_) => "dca",
                    StrategyParams::Grid(_) => "grid",
                };
                (p.id.to_string(), kind.to_string())
            })
            .collect()
    }
}
