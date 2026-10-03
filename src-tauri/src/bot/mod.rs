//! Bot orchestrator: owns configs, running state, open positions, skip notes,
//! and the daily-loss kill-switch. The tick loop lives in `engine/`; this file
//! is state + lifecycle only (SRP).

mod close_claim;
mod commands;
pub(crate) mod engine;
mod judged;
pub mod model;
pub(crate) mod strategy;

pub use commands::{
    bot_close_position, bot_configure, bot_default_config, bot_set_live, bot_start, bot_status,
    bot_stop,
};

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tokio::sync::watch;

use crate::risk::RiskManager;
use crate::signal::model::Signal;
use crate::store::{utc_day_start_ms, StoreManager};
use close_claim::CloseClaims;
use judged::JudgeLedger;
use model::{BotConfig, BotDeskStatus, BotKind, OpenPosition, SkipNote, LIVE_TRADING_ENABLED};

const TICK_INTERVAL: Duration = Duration::from_secs(4);
const SKIP_CAP: usize = 30;
/// `meta` key holding the UTC day-start millis of the last kill-switch trip.
const TRIP_KEY: &str = "tripped_day";

/// `meta` key holding one bot's saved config.
fn config_key(kind: BotKind) -> String {
    format!("bot_config.{}", kind.as_str())
}

pub struct BotManager {
    configs: Mutex<HashMap<BotKind, BotConfig>>,
    running: Mutex<HashSet<BotKind>>,
    positions: Mutex<Vec<OpenPosition>>,
    skips: Mutex<VecDeque<SkipNote>>,
    /// Final decisions + once-only transient skip notes (see judged.rs).
    ledger: Mutex<JudgeLedger>,
    /// In-flight close claims — a veto close and the tick loop's own close
    /// stay mutually exclusive (close_claim.rs).
    closing: CloseClaims,
    /// UTC day-start millis when the kill-switch tripped; 0 = not tripped.
    tripped_day: AtomicU64,
    /// Last BTC regime the engine read from Sentinel (see engine/btc_break.rs).
    /// Starts Unknown — before the first successful read the guard refuses
    /// new entries rather than assuming the market is calm.
    btc_regime: Mutex<engine::btc_break::BtcRegime>,
    loop_stop: Mutex<Option<watch::Sender<bool>>>,
    /// Last Binance wallet balance read for the live daily stop, with its
    /// read time (ms).
    equity: Mutex<Option<(f64, u64)>>,
}

impl BotManager {
    pub fn new() -> Self {
        Self {
            configs: Mutex::new(HashMap::new()),
            running: Mutex::new(HashSet::new()),
            positions: Mutex::new(Vec::new()),
            skips: Mutex::new(VecDeque::new()),
            ledger: Mutex::new(JudgeLedger::default()),
            closing: CloseClaims::default(),
            tripped_day: AtomicU64::new(0),
            btc_regime: Mutex::new(engine::btc_break::BtcRegime::Unknown),
            loop_stop: Mutex::new(None),
            equity: Mutex::new(None),
        }
    }

    pub fn configure(&self, cfg: BotConfig) {
        self.configs
            .lock()
            .expect("bot mutex")
            .insert(cfg.kind, cfg);
    }

    /// Starts a bot. Refused while today's kill-switch is tripped (PRD §5.3:
    /// manual restart only after the next UTC day begins).
    pub fn start(&self, app: AppHandle, kind: BotKind) -> Result<(), String> {
        if self.kill_switch_tripped() {
            return Err("botDailyLossTripped".to_string());
        }
        // Hard server-side guard for Pump — the engine gates per-entry too, but
        // starting a Pump bot below Ambitious should be refused outright.
        if kind == BotKind::Pump && !app.state::<RiskManager>().allows_pump() {
            return Err("botPumpNeedsAmbitious".to_string());
        }
        if !self.configs.lock().expect("bot mutex").contains_key(&kind) {
            return Err("botNotConfigured".to_string());
        }
        self.running.lock().expect("bot mutex").insert(kind);
        self.ensure_loop(app);
        Ok(())
    }

    /// Stops a bot. Open positions stay managed (close phase keeps running)
    /// until they resolve — stopping only halts NEW entries.
    pub fn stop(&self, kind: BotKind) {
        self.running.lock().expect("bot mutex").remove(&kind);
    }

    /// Reloads open positions persisted by a previous run and, if any exist,
    /// starts the loop so they keep being managed even before a bot starts
    /// (positions used to live only in memory and vanished on restart).
    pub fn restore(&self, app: AppHandle) {
        self.seed_judged(&app);
        self.restore_trip(&app);
        self.restore_configs(&app);
        let restored = engine::book::load_positions(&app);
        if restored.is_empty() {
            return;
        }
        {
            let mut ledger = self.ledger.lock().expect("bot mutex");
            for p in &restored {
                ledger.judge(p.bot_kind, &p.signal_id);
            }
        }
        *self.positions.lock().expect("bot mutex") = restored;
        self.ensure_loop(app);
    }

    /// The judge ledger is memory-only, and the reconnect backfill returns
    /// every pending signal — including ones this bot already traded and
    /// closed. Seeding it from the trade table keeps a restart from entering
    /// the same signal twice. A week covers any signal still pending.
    fn seed_judged(&self, app: &AppHandle) {
        const SEED_WINDOW_MS: u64 = 7 * 24 * 3_600_000;
        let Ok(dir) = app.path().app_data_dir() else {
            return;
        };
        let since = engine::now_ms().saturating_sub(SEED_WINDOW_MS);
        let keys = match app.state::<StoreManager>().recent_trade_keys(&dir, since) {
            Ok(k) => k,
            Err(e) => return self.push_skip("—", "storeReadFailed", Some(e)),
        };
        let mut ledger = self.ledger.lock().expect("bot mutex");
        for (id, kind) in keys {
            if let Some(kind) = BotKind::parse(&kind) {
                ledger.judge(kind, &id);
            }
        }
    }

    /// Starts the 4s engine loop once (signal bots, restored positions and
    /// strategy bots all share it).
    pub(crate) fn ensure_loop(&self, app: AppHandle) {
        let mut guard = self.loop_stop.lock().expect("bot mutex");
        if guard.is_some() {
            return;
        }
        let (tx, mut rx) = watch::channel(false);
        *guard = Some(tx);
        drop(guard);

        tauri::async_runtime::spawn(async move {
            loop {
                if *rx.borrow() {
                    break;
                }
                engine::tick(&app).await;
                tokio::select! {
                    _ = tokio::time::sleep(TICK_INTERVAL) => {},
                    _ = rx.changed() => { if *rx.borrow() { break; } }
                }
            }
        });
    }

    // ---- state accessors used by the engine ----

    pub fn running_config(&self, kind: BotKind) -> Option<BotConfig> {
        if !self.running.lock().expect("bot mutex").contains(&kind) {
            return None;
        }
        self.configs.lock().expect("bot mutex").get(&kind).cloned()
    }

    /// Config regardless of running state — open positions outlive a stopped
    /// bot (stopping only halts new entries), so the close phase must still
    /// see the bot's max-loss setting.
    pub fn config_for(&self, kind: BotKind) -> Option<BotConfig> {
        self.configs.lock().expect("bot mutex").get(&kind).cloned()
    }

    pub fn set_btc_regime(&self, regime: engine::btc_break::BtcRegime) {
        *self.btc_regime.lock().expect("bot mutex") = regime;
    }

    pub fn btc_regime(&self) -> engine::btc_break::BtcRegime {
        *self.btc_regime.lock().expect("bot mutex")
    }

    pub fn any_running(&self) -> bool {
        !self.running.lock().expect("bot mutex").is_empty()
    }

    pub fn is_judged(&self, kind: BotKind, signal_id: &str) -> bool {
        self.ledger
            .lock()
            .expect("bot mutex")
            .is_judged(kind, signal_id)
    }

    /// Final decision for `kind`: the signal is never evaluated again.
    pub fn judge(&self, kind: BotKind, signal_id: &str) {
        self.ledger
            .lock()
            .expect("bot mutex")
            .judge(kind, signal_id);
    }

    /// Pushes a skip note only the first time this (bot, signal, reason)
    /// occurs — transient skips retry every tick and must not flood the feed.
    pub fn skip_once(
        &self,
        kind: BotKind,
        signal_id: &str,
        symbol: &str,
        key: &str,
        detail: Option<String>,
    ) {
        let first = self
            .ledger
            .lock()
            .expect("bot mutex")
            .note_once(kind, signal_id, key);
        if first {
            self.push_skip(symbol, key, detail);
        }
    }

    /// Applies a skip: a final one judges the signal and is always shown; a
    /// transient one is shown only the first time (see entry::settle_skip).
    pub fn settle_skip(&self, kind: BotKind, sig: &Signal, skip: engine::precheck::Skip) {
        let show = {
            let mut ledger = self.ledger.lock().expect("bot mutex");
            engine::entry::settle_skip(&mut ledger, kind, &sig.id, &skip)
        };
        if show {
            self.push_skip(&sig.symbol, skip.key, skip.detail);
        }
    }

    /// True when `kind` already holds a position on this symbol+timeframe.
    pub fn holds_market(&self, kind: BotKind, symbol: &str, timeframe: &Option<String>) -> bool {
        self.positions
            .lock()
            .expect("bot mutex")
            .iter()
            .any(|p| p.bot_kind == kind && p.symbol == symbol && &p.timeframe == timeframe)
    }

    pub fn positions_snapshot(&self) -> Vec<OpenPosition> {
        self.positions.lock().expect("bot mutex").clone()
    }

    /// Open positions across all bots.
    pub fn open_count(&self) -> usize {
        self.positions.lock().expect("bot mutex").len()
    }

    /// Open positions held by one bot kind (its own per-bot ceiling).
    pub fn open_count_for(&self, kind: BotKind) -> usize {
        self.positions
            .lock()
            .expect("bot mutex")
            .iter()
            .filter(|p| p.bot_kind == kind)
            .count()
    }

    pub fn add_position(&self, pos: OpenPosition) {
        self.positions.lock().expect("bot mutex").push(pos);
    }

    /// Keyed by (signal, bot): two bots may hold the same signal, and closing
    /// one must not drop the other.
    pub fn remove_position(&self, signal_id: &str, kind: BotKind) {
        self.positions
            .lock()
            .expect("bot mutex")
            .retain(|p| !(p.signal_id == signal_id && p.bot_kind == kind));
    }

    /// Stores updated management state (breakeven armed, partial banked).
    /// `false` when the position is no longer in the book (a concurrent close
    /// removed it): the caller must not persist it back.
    pub fn update_position(&self, pos: &OpenPosition) -> bool {
        let mut g = self.positions.lock().expect("bot mutex");
        match g
            .iter_mut()
            .find(|p| p.signal_id == pos.signal_id && p.bot_kind == pos.bot_kind)
        {
            Some(slot) => {
                *slot = pos.clone();
                true
            }
            None => false,
        }
    }

    /// Claims the exclusive right to act on an OPEN position: the close claim,
    /// kept only if the position is still in the book. `false` = leave it.
    pub fn claim_open(&self, signal_id: &str, kind: BotKind) -> bool {
        if !self.closing.claim(signal_id, kind) {
            return false;
        }
        if !self.has_position(signal_id, kind) {
            self.closing.release(signal_id, kind);
            return false;
        }
        true
    }

    /// Whether (signal, bot) is still open — re-checked after a close claim,
    /// because the snapshot the claim was taken from may be stale.
    pub fn has_position(&self, signal_id: &str, kind: BotKind) -> bool {
        self.positions
            .lock()
            .expect("bot mutex")
            .iter()
            .any(|p| p.signal_id == signal_id && p.bot_kind == kind)
    }

    pub fn push_skip(&self, symbol: &str, reason: &str, detail: Option<String>) {
        let mut g = self.skips.lock().expect("bot mutex");
        if g.len() == SKIP_CAP {
            g.pop_front();
        }
        g.push_back(SkipNote {
            at_ms: engine::now_ms(),
            symbol: symbol.to_string(),
            reason: reason.to_string(),
            detail,
        });
    }

    pub fn cached_equity(&self) -> Option<(f64, u64)> {
        *self.equity.lock().expect("bot mutex")
    }

    pub fn cache_equity(&self, value: f64, at_ms: u64) {
        *self.equity.lock().expect("bot mutex") = Some((value, at_ms));
    }

    pub fn kill_switch_tripped(&self) -> bool {
        self.tripped_day.load(Ordering::Relaxed) == utc_day_start_ms()
    }

    /// Trips the switch for today and stops every bot. The trip day is
    /// persisted: a restart the same UTC day must not re-arm trading (with a
    /// raised balance or level the live re-check would no longer trip).
    pub fn trip_kill_switch(&self, app: &AppHandle) {
        let day = utc_day_start_ms();
        self.tripped_day.store(day, Ordering::Relaxed);
        self.running.lock().expect("bot mutex").clear();
        let saved = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())
            .and_then(|dir| app.state::<StoreManager>().set_meta(&dir, TRIP_KEY, &day.to_string()));
        if let Err(e) = saved {
            self.push_skip("—", "storeWriteFailed", Some(e));
        }
    }

    /// Persists a bot's config so a restart keeps its settings — restored
    /// positions need their bot's max-loss before the user starts anything.
    pub fn save_config(&self, app: &AppHandle, cfg: &BotConfig) {
        let saved = app.path().app_data_dir().map_err(|e| e.to_string()).and_then(|dir| {
            let body = serde_json::to_string(cfg).map_err(|e| e.to_string())?;
            app.state::<StoreManager>()
                .set_meta(&dir, &config_key(cfg.kind), &body)
        });
        if let Err(e) = saved {
            self.push_skip("—", "storeWriteFailed", Some(e));
        }
    }

    /// Reloads saved configs. `live` is never restored: real-money trading is
    /// re-confirmed every session, never re-armed from a file on disk.
    fn restore_configs(&self, app: &AppHandle) {
        let Ok(dir) = app.path().app_data_dir() else {
            return;
        };
        let store = app.state::<StoreManager>();
        for kind in [BotKind::Futures, BotKind::Spot, BotKind::Pump] {
            let Ok(Some(body)) = store.get_meta(&dir, &config_key(kind)) else {
                continue;
            };
            match serde_json::from_str::<BotConfig>(&body) {
                Ok(mut cfg) if cfg.kind == kind && cfg.validate().is_ok() => {
                    cfg.live = false;
                    self.configure(cfg);
                }
                _ => self.push_skip("—", "storeReadFailed", Some(format!("bot config {}", kind.as_str()))),
            }
        }
    }

    /// Reloads a trip recorded earlier today (see `trip_kill_switch`).
    fn restore_trip(&self, app: &AppHandle) {
        let Ok(dir) = app.path().app_data_dir() else {
            return;
        };
        match app.state::<StoreManager>().get_meta(&dir, TRIP_KEY) {
            Ok(Some(v)) => {
                if let Ok(day) = v.parse::<u64>() {
                    self.tripped_day.store(day, Ordering::Relaxed);
                }
            }
            Ok(None) => {}
            Err(e) => self.push_skip("—", "storeReadFailed", Some(e)),
        }
    }

    pub fn status(&self) -> BotDeskStatus {
        let configs = self.configs.lock().expect("bot mutex");
        let running = self.running.lock().expect("bot mutex");
        BotDeskStatus {
            live_trading_enabled: LIVE_TRADING_ENABLED,
            binance_is_production: crate::app::endpoints::binance_is_production(),
            futures: configs.get(&BotKind::Futures).cloned(),
            spot: configs.get(&BotKind::Spot).cloned(),
            pump: configs.get(&BotKind::Pump).cloned(),
            futures_running: running.contains(&BotKind::Futures),
            spot_running: running.contains(&BotKind::Spot),
            pump_running: running.contains(&BotKind::Pump),
            open_positions: self.positions.lock().expect("bot mutex").clone(),
            recent_skips: self
                .skips
                .lock()
                .expect("bot mutex")
                .iter()
                .rev()
                .cloned()
                .collect(),
            kill_switch_tripped: self.kill_switch_tripped(),
            btc_regime: self.btc_regime().as_str().to_string(),
        }
    }
}

impl Default for BotManager {
    fn default() -> Self {
        Self::new()
    }
}
