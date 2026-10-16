//! The foreign-trade watch on the desk: every minute, while a Binance trade
//! key is unlocked in a live build, judge the account's new fills
//! (`exchange::foreign_fill`). A fill this app did not make:
//!
//! - is recorded and shown as a danger alert (Risk page, header alerts),
//! - goes to Telegram under the "kill switch" category (`telegram::notify`),
//! - stops new entries on every bot through the phone's Stop path
//!   (`DeskBridge::stop_opening`). Open positions are NOT closed: whether a
//!   fill was an attack or the user's own trade on Binance is the user's call.
//!
//! "That was me" (`foreign_fill_ack`) clears the alarm and restarts nothing;
//! the user starts the bots again. Paper builds compile this and never poll.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use aleph_desk_link::DeskControl;
use aleph_link::BotSelector;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::bot::model::LIVE_TRADING_ENABLED;
use crate::bot::strategy_live::StrategyLiveManager;
use crate::bot::BotManager;
use crate::exchange::foreign_fill::{scan, ForeignFill, Reader, ScanCursor};
use crate::exchange::model::BinanceKeyCheckError as E;
use crate::exchange::providers::binance_http::now_ms;
use crate::exchange::ExchangeManager;
use crate::link::DeskBridge;
use crate::store::StoreManager;
use crate::vault::model::CredentialPermission;
use crate::vault::VaultManager;

const STATE_KEY: &str = "foreign_fill_state";
const OWN_KEY: &str = "own_orders";
const EVERY: Duration = Duration::from_secs(60);
/// Alarms kept until acknowledged (the newest).
const MAX_ALARMS: usize = 200;

/// Persisted watch state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WatchState {
    pub enabled: bool,
    pub cursor: ScanCursor,
    /// Unacknowledged foreign fills, oldest first.
    pub alarms: Vec<ForeignFill>,
    pub last_check_ms: Option<u64>,
    /// Stable code of the last failed scan (`errors.*`), None after a clean one.
    pub last_error: Option<String>,
}

/// OFF by default (owner, 16 Oct 2026): switched on by the user after the
/// testnet check, so an untested matcher can never stop bots on its own.
impl Default for WatchState {
    fn default() -> Self {
        Self { enabled: false, cursor: ScanCursor::default(), alarms: Vec::new(), last_check_ms: None, last_error: None }
    }
}

#[derive(Default)]
pub struct ForeignFillManager {
    state: Mutex<WatchState>,
    /// A scan ran on the last tick (key unlocked, switched on, live build).
    watching: Mutex<bool>,
}

/// What the UI shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignFillView {
    pub live_build: bool,
    pub enabled: bool,
    pub watching: bool,
    pub last_check_ms: Option<u64>,
    pub last_error: Option<String>,
    pub alarms: Vec<ForeignFill>,
}

impl ForeignFillManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn view(&self) -> ForeignFillView {
        let s = self.state.lock().expect("foreign fill mutex").clone();
        ForeignFillView {
            live_build: LIVE_TRADING_ENABLED,
            enabled: s.enabled,
            watching: LIVE_TRADING_ENABLED && *self.watching.lock().expect("foreign fill mutex"),
            last_check_ms: s.last_check_ms,
            last_error: s.last_error,
            alarms: s.alarms,
        }
    }

    /// Unacknowledged foreign fills (Telegram, preflight).
    pub fn alarms(&self) -> Vec<ForeignFill> {
        self.state.lock().expect("foreign fill mutex").alarms.clone()
    }

    pub fn enabled(&self) -> bool {
        self.state.lock().expect("foreign fill mutex").enabled
    }
}

/// Adds newly found fills to the alarm list (one per trade, newest kept).
/// Pure; true when anything was added.
pub fn add_alarms(state: &mut WatchState, found: &[ForeignFill]) -> bool {
    let mut added = false;
    for f in found {
        if !state.alarms.iter().any(|a| a.symbol == f.symbol && a.trade_id == f.trade_id) {
            state.alarms.push(f.clone());
            added = true;
        }
    }
    let over = state.alarms.len().saturating_sub(MAX_ALARMS);
    state.alarms.drain(..over);
    added
}

/// Switching the watch: back on, it starts from now (no alarm for trades
/// made while it was off). Pure.
pub fn set_enabled(state: &mut WatchState, on: bool) {
    if on && !state.enabled {
        state.cursor = ScanCursor::default();
        state.last_error = None;
    }
    state.enabled = on;
}

fn error_code(e: &E) -> String {
    match e {
        E::InvalidCredentials { code, .. } => format!("binanceRefused{code}"),
        E::NetworkUnavailable => "binanceUnreachable".into(),
        E::Rejected { code, .. } => format!("binanceRefused{code}"),
        E::RateLimited => "binanceRateLimited".into(),
        E::Unknown => "binanceUnconfirmed".into(),
    }
}

fn save(app: &AppHandle, state: &WatchState) {
    let (Ok(dir), Ok(body)) = (app.path().app_data_dir(), serde_json::to_string(state)) else { return };
    let _ = app.state::<StoreManager>().set_meta(&dir, STATE_KEY, &body);
}

/// Loads the state, makes the order registry persistent, and starts the
/// minute loop. Paper builds: nothing.
pub fn spawn(app: &AppHandle) {
    if !LIVE_TRADING_ENABLED {
        return;
    }
    let Ok(dir) = app.path().app_data_dir() else { return };
    let store = app.state::<StoreManager>();
    if let Some(s) = store.get_meta(&dir, STATE_KEY).ok().flatten().and_then(|b| serde_json::from_str::<WatchState>(&b).ok()) {
        *app.state::<ForeignFillManager>().state.lock().expect("foreign fill mutex") = s;
    }
    let persisted = store.get_meta(&dir, OWN_KEY).ok().flatten();
    let a = app.clone();
    app.state::<ExchangeManager>().own.install(
        persisted.as_deref(),
        Arc::new(move |body: String| {
            if let Ok(dir) = a.path().app_data_dir() {
                let _ = a.state::<StoreManager>().set_meta(&dir, OWN_KEY, &body);
            }
        }),
    );
    let a = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(EVERY).await;
            tick(&a).await;
        }
    });
}

async fn tick(app: &AppHandle) {
    let mgr = app.state::<ForeignFillManager>();
    let set_watching = |on: bool| *mgr.watching.lock().expect("foreign fill mutex") = on;
    let (enabled, mut cursor) = {
        let s = mgr.state.lock().expect("foreign fill mutex");
        (s.enabled, s.cursor.clone())
    };
    // Real money exists only on Binance; a key that is not verified
    // trade-only never trades here, so it is not watched either.
    let cred = app
        .state::<VaultManager>()
        .credential_quiet("binance")
        .filter(|c| c.permission == CredentialPermission::TradeOnly);
    let Some(cred) = cred.filter(|_| enabled) else {
        set_watching(false);
        return;
    };
    set_watching(true);
    let mut extra: Vec<(String, i64)> = app
        .state::<BotManager>()
        .positions_snapshot()
        .iter()
        .filter(|p| p.is_live())
        .flat_map(|p| [p.stop_algo_id, p.tp_algo_id].into_iter().flatten().map(|id| (p.symbol.clone(), id)))
        .collect();
    extra.extend(app.state::<StrategyLiveManager>().stop_algos());
    let exchange = app.state::<ExchangeManager>();
    let base = crate::app::endpoints::binance_futures_base();
    let reader = Reader { client: exchange.http(), base: &base, key: &cred.api_key, signer: cred.binance_signer() };
    let now = now_ms();
    let res = scan(&reader, &mut cursor, &exchange.own, &extra, now).await;

    let added = {
        let mut s = mgr.state.lock().expect("foreign fill mutex");
        if !s.enabled {
            // Switched off while the scan ran: its result is dropped.
            return;
        }
        s.cursor = cursor;
        s.last_check_ms = Some(now);
        s.last_error = res.error.as_ref().map(error_code);
        let added = add_alarms(&mut s, &res.foreign);
        save(app, &s);
        added
    };
    if added {
        for f in &res.foreign {
            eprintln!("foreign fill: {} trade {} order {} {} {} @ {}", f.symbol, f.trade_id, f.order_id, f.side, f.qty, f.price);
        }
        // The phone's Stop: no new entries anywhere, positions untouched.
        let _ = DeskBridge::new(app.clone()).stop_opening(BotSelector::All).await;
    }
}

#[tauri::command]
pub fn foreign_fill_status(mgr: State<ForeignFillManager>) -> ForeignFillView {
    mgr.view()
}

/// "That was me": clears the alarm. Bots stay stopped until the user starts
/// them.
#[tauri::command]
pub fn foreign_fill_ack(app: AppHandle, mgr: State<ForeignFillManager>) -> ForeignFillView {
    {
        let mut s = mgr.state.lock().expect("foreign fill mutex");
        s.alarms.clear();
        save(&app, &s);
    }
    mgr.view()
}

#[tauri::command]
pub fn foreign_fill_set_enabled(app: AppHandle, mgr: State<ForeignFillManager>, on: bool) -> ForeignFillView {
    {
        let mut s = mgr.state.lock().expect("foreign fill mutex");
        set_enabled(&mut s, on);
        save(&app, &s);
    }
    mgr.view()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(symbol: &str, trade_id: i64) -> ForeignFill {
        ForeignFill {
            symbol: symbol.into(),
            trade_id,
            order_id: 1,
            side: "BUY".into(),
            qty: 1.0,
            price: 1.0,
            time_ms: 1,
            detected_ms: 2,
        }
    }

    #[test]
    fn alarms_are_once_per_trade_and_bounded() {
        let mut s = WatchState::default();
        assert!(add_alarms(&mut s, &[fill("XUSDT", 1), fill("XUSDT", 2)]));
        assert!(!add_alarms(&mut s, &[fill("XUSDT", 1)]), "the same trade twice is one alarm");
        assert_eq!(s.alarms.len(), 2);
        let many: Vec<_> = (10..10 + MAX_ALARMS as i64).map(|i| fill("YUSDT", i)).collect();
        add_alarms(&mut s, &many);
        assert_eq!(s.alarms.len(), MAX_ALARMS);
        assert_eq!(s.alarms.last().unwrap().trade_id, 9 + MAX_ALARMS as i64, "newest kept");
    }

    #[test]
    fn off_by_default_and_switching_back_on_starts_from_now() {
        let mut s = WatchState::default();
        assert!(!s.enabled, "off until the user switches it on");
        set_enabled(&mut s, true);
        s.cursor.cursor_ms = Some(5);
        set_enabled(&mut s, true);
        assert_eq!(s.cursor.cursor_ms, Some(5), "already on: cursor kept");
        set_enabled(&mut s, false);
        set_enabled(&mut s, true);
        assert_eq!(s.cursor.cursor_ms, None, "the next scan sets it to now: no backlog");
        // A state saved without the field loads switched off.
        let old: WatchState = serde_json::from_str(r#"{"alarms":[]}"#).unwrap();
        assert!(!old.enabled);
    }
}
