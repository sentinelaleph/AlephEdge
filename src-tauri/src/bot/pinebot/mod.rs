//! The Pine bot (`BotKind::Pine`, docs/aleph-edge-pine-lite-spec.md): the
//! user's own Pine-lite script, run in Rust (`crate::pine`, QuickJS) on
//! closed Binance USDT-M bars of one timeframe for 1 to 10 symbols. Each
//! bar's mapped triggers become entries through the entry the TradingView
//! bot uses (`engine::user_entry`), Paper or Live behind the same locks.
//!
//! - `rules`: the decisions, pure (due / stale / missing bar, Close first,
//!   conflict, flip, exits, price guard, limits, the halt after 3 failures);
//! - `bars`: keyless klines on the bot's timeframe;
//! - `phase`: the tick's Pine phase (`engine::tick`), `act`: one bar's
//!   orders and its result row;
//! - `config`: the saved config (hash, disarm on edit);
//! - `commands`: the screens' Tauri commands.
//!
//! The action key of a bar (`rules::event_key`) is claimed in the store
//! (`store::pine`) before any order, so a bar is acted on at most once. A
//! run failure orders nothing; three in a row halt a Live bot (it stops,
//! `pineHalted` is kept in `meta` until the user starts it again) while its
//! open positions keep their exchange stops and the normal close phase.

mod act;
pub mod bars;
pub mod commands;
mod config;
mod pending;
pub mod phase;
pub mod rules;
pub mod types;

#[cfg(test)]
mod gate_tests;
#[cfg(test)]
mod tests;

pub use config::{normalize_config, script_terms_changed};

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::store::StoreManager;
use types::{FiredTrigger, PineRunError, PineSymbolStatus};

/// `meta` key holding the halt (`pineHalted|3`) until the next start.
pub const HALT_KEY: &str = "pine.halted";
/// Emitted (with the action key) when a result row is written or updated.
pub const RESULT_EVENT: &str = "pine:result";

#[derive(Default)]
pub(crate) struct Inner {
    /// Per symbol, the bar close (ms) whose run is done (ran, failed, stale
    /// or acted on) for the script hash it ran: never run twice.
    pub done: HashMap<String, (String, u64)>,
    pub symbols: HashMap<String, PineSymbolStatus>,
    pub streak: rules::FailStreak,
    /// None until read from the store once.
    pub halted: Option<Option<String>>,
    pub last_notice_ms: Option<u64>,
    /// Trigger titles of one script hash (from its compile).
    pub titles: Option<(String, HashMap<String, String>)>,
    pub last_pending_ms: u64,
}

pub struct PineManager {
    pub(crate) inner: Mutex<Inner>,
    pub(crate) client: reqwest::Client,
    /// One Pine phase at a time.
    pub(crate) phase_lock: tokio::sync::Mutex<()>,
}

impl PineManager {
    pub fn new() -> Self {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().unwrap_or_default();
        Self { inner: Mutex::new(Inner::default()), client, phase_lock: tokio::sync::Mutex::new(()) }
    }

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn entry<'a>(g: &'a mut Inner, symbol: &str) -> &'a mut PineSymbolStatus {
        g.symbols.entry(symbol.to_string()).or_insert_with(|| PineSymbolStatus { symbol: symbol.to_string(), ..Default::default() })
    }

    pub(crate) fn is_done(&self, hash: &str, symbol: &str, close_ms: u64) -> bool {
        self.lock().done.get(symbol).is_some_and(|(h, c)| h == hash && *c == close_ms)
    }

    pub(crate) fn mark_done(&self, hash: &str, symbol: &str, close_ms: u64) {
        self.lock().done.insert(symbol.to_string(), (hash.to_string(), close_ms));
    }

    pub(crate) fn note(&self, symbol: &str, code: &str) {
        Self::entry(&mut self.lock(), symbol).last_note = Some(code.to_string());
    }

    /// A good run: its numbers, the fired triggers; the error cleared.
    pub(crate) fn ran(&self, symbol: &str, bar_time: u64, bars: usize, ms: u64, fired: Vec<FiredTrigger>, now: u64) {
        let mut g = self.lock();
        let s = Self::entry(&mut g, symbol);
        (s.last_run_at, s.last_bar_time, s.bars_used, s.run_ms) = (Some(now), Some(bar_time), Some(bars), Some(ms));
        s.last_fired = fired;
        s.last_error = None;
        s.last_note = None;
    }

    pub(crate) fn failed(&self, symbol: &str, err: PineRunError) {
        let mut g = self.lock();
        let s = Self::entry(&mut g, symbol);
        (s.last_run_at, s.last_bar_time) = (Some(err.at_ms), Some(err.bar_time));
        s.last_fired.clear();
        s.last_note = None;
        s.last_error = Some(err);
    }

    /// The halt in force (read from the store the first time).
    pub(crate) fn halted(&self, app: &AppHandle) -> Option<String> {
        if let Some(h) = self.lock().halted.clone() {
            return h;
        }
        let read = app.path().app_data_dir().ok().and_then(|d| app.state::<StoreManager>().get_meta(&d, HALT_KEY).ok().flatten());
        let h = read.filter(|v| !v.is_empty());
        self.lock().halted = Some(h.clone());
        h
    }

    /// Sets (Some) or clears (None) the halt, in memory and in `meta`.
    pub(crate) fn set_halt(&self, app: &AppHandle, code: Option<String>) {
        {
            let mut g = self.lock();
            g.halted = Some(code.clone());
            if code.is_none() {
                g.streak = rules::FailStreak::default();
            }
        }
        if let Ok(dir) = app.path().app_data_dir() {
            let _ = app.state::<StoreManager>().set_meta(&dir, HALT_KEY, code.as_deref().unwrap_or(""));
        }
    }

    /// The trigger titles of `hash`, if compiled.
    pub(crate) fn title(&self, hash: &str, id: &str) -> String {
        let g = self.lock();
        g.titles.as_ref().filter(|(h, _)| h == hash).and_then(|(_, t)| t.get(id).cloned()).unwrap_or_default()
    }
}

impl Default for PineManager {
    fn default() -> Self {
        Self::new()
    }
}

/// A (re)start of the Pine bot by the user ends a halt (`BotManager::start`).
pub fn on_start(app: &AppHandle) {
    let mgr = app.state::<PineManager>();
    if mgr.halted(app).is_some() {
        mgr.set_halt(app, None);
    }
}
