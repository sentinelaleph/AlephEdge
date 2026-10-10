//! Bot orchestrator: owns configs, running state, open positions, skip notes,
//! and the daily-loss kill-switch. The tick loop lives in `engine/`; this file
//! is state + lifecycle only (SRP).

mod close_claim;
mod commands;
pub(crate) mod engine;
mod judged;
pub mod model;
pub(crate) mod strategy;
pub mod strategy_live;

pub use commands::{
    bot_close_position, bot_configure, bot_default_config, bot_end_pilot, bot_preview_take, bot_set_live, bot_start,
    bot_status, bot_stop, bot_take_signal,
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
/// A paper position last evaluated longer ago than this was not watched in
/// between (app closed, PC asleep, no price): a stop found crossed is booked
/// at the price seen, not at its level (see management::Fill).
const WATCH_GAP_MS: u64 = 60_000;
const SKIP_CAP: usize = 30;
/// `meta` key holding the UTC day-start millis of the last kill-switch trip.
const TRIP_KEY: &str = "tripped_day";
/// Signal bots running when the app last changed one, comma-separated kinds.
/// A restart resumes them (in paper: `live` is never restored), as DCA/Grid
/// bots already resume; before this a restart left every signal bot stopped
/// with nothing on screen saying why no position opened.
const RUNNING_KEY: &str = "running_kinds";

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
    /// Pilot entries left per bot kind (see model::PILOT_TRADES).
    pilot: Mutex<HashMap<BotKind, u32>>,
    /// When each open position was last evaluated on the safe side of its
    /// levels (memory only: a restart starts unwatched).
    watched: Mutex<HashMap<(String, BotKind), u64>>,
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
            pilot: Mutex::new(HashMap::new()),
            watched: Mutex::new(HashMap::new()),
        }
    }

    /// Starts (or ends, with 0) the pilot of a bot kind.
    pub fn set_pilot(&self, kind: BotKind, entries: u32) {
        self.pilot.lock().expect("bot mutex").insert(kind, entries);
    }

    pub fn pilot_left(&self, kind: BotKind) -> u32 {
        self.pilot.lock().expect("bot mutex").get(&kind).copied().unwrap_or(0)
    }

    /// One real entry made under the pilot.
    pub fn pilot_used(&self, kind: BotKind) {
        if let Some(n) = self.pilot.lock().expect("bot mutex").get_mut(&kind) {
            *n = n.saturating_sub(1);
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
        let Some(cfg) = self.config_for(kind) else {
            return Err("botNotConfigured".to_string());
        };
        // A saved config can predate a tighter rule, and the risk level or
        // balance can change after it was saved: every start re-checks it, so
        // a bot never runs on settings that would skip every signal.
        cfg.validate()?;
        commands::check_capital_cap(&cfg, app.state::<RiskManager>().state().max_capital_quote)?;
        self.running.lock().expect("bot mutex").insert(kind);
        self.save_running(&app);
        self.ensure_loop(app);
        Ok(())
    }

    /// Stops a bot. Open positions stay managed (close phase keeps running)
    /// until they resolve — stopping only halts NEW entries.
    pub fn stop(&self, app: &AppHandle, kind: BotKind) {
        self.running.lock().expect("bot mutex").remove(&kind);
        self.save_running(app);
    }

    /// Only paper bots are saved: a bot on real money comes back stopped and
    /// waits for the typed LIVE confirmation, as before.
    fn save_running(&self, app: &AppHandle) {
        let paper: HashSet<BotKind> = {
            let configs = self.configs.lock().expect("bot mutex");
            self.running
                .lock()
                .expect("bot mutex")
                .iter()
                .copied()
                .filter(|k| configs.get(k).is_some_and(|c| !c.live))
                .collect()
        };
        let body = running_body(&paper);
        let saved = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())
            .and_then(|dir| app.state::<StoreManager>().set_meta(&dir, RUNNING_KEY, &body));
        if let Err(e) = saved {
            self.push_skip("—", "storeWriteFailed", Some(e));
        }
    }

    /// Resumes the signal bots that were running before the restart. Each
    /// goes through `start`, so a tripped daily stop, a missing config or a
    /// capital above the cap keeps it stopped. A bot in `blocked` (a
    /// real-money row of its could not be read) stays stopped too.
    fn restore_running(&self, app: &AppHandle, blocked: &HashSet<BotKind>) {
        let Ok(dir) = app.path().app_data_dir() else {
            return;
        };
        let Ok(Some(body)) = app.state::<StoreManager>().get_meta(&dir, RUNNING_KEY) else {
            return;
        };
        for kind in parse_running(&body) {
            if blocked.contains(&kind) {
                self.push_skip("—", "botNotResumed", Some(format!("{} restoreLiveBlocked", kind.as_str())));
                continue;
            }
            if let Err(key) = self.start(app.clone(), kind) {
                self.push_skip("—", "botNotResumed", Some(format!("{} {key}", kind.as_str())));
            }
        }
    }

    /// Reloads open positions persisted by a previous run and, if any exist,
    /// starts the loop so they keep being managed even before a bot starts
    /// (positions used to live only in memory and vanished on restart).
    pub fn restore(&self, app: AppHandle) {
        self.seed_judged(&app);
        self.restore_trip(&app);
        self.restore_configs(&app);
        // Positions go into the book BEFORE any bot resumes: a resumed bot's
        // first tick must see the markets it already holds.
        let restored = engine::book::load_positions(&app);
        let any = !restored.positions.is_empty();
        {
            let mut ledger = self.ledger.lock().expect("bot mutex");
            for p in &restored.positions {
                ledger.judge(p.bot_kind, &p.signal_id);
            }
        }
        *self.positions.lock().expect("bot mutex") = restored.positions;
        self.restore_running(&app, &restored.live_unreadable);
        if any {
            self.ensure_loop(app);
        }
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

    /// `kind`'s settings changed: pending signals its old settings refused
    /// are judged again under the new ones (see judged.rs).
    pub fn clear_settings_verdicts(&self, kind: BotKind) -> usize {
        self.ledger
            .lock()
            .expect("bot mutex")
            .clear_settings_verdicts(kind)
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
    /// Every shown skip is also written to the store's skip log, so the
    /// reason survives a restart. The log keeps one row per (bot, signal,
    /// reason): a verdict it already holds (the same pending signal settled
    /// again after a restart or a settings change) is neither logged nor
    /// pushed to the feed a second time.
    pub fn settle_skip(&self, app: &AppHandle, kind: BotKind, sig: &Signal, skip: engine::precheck::Skip) {
        let show = {
            let mut ledger = self.ledger.lock().expect("bot mutex");
            engine::entry::settle_skip(&mut ledger, kind, &sig.id, &skip)
        };
        if !show {
            return;
        }
        if let Ok(dir) = app.path().app_data_dir() {
            let row = crate::store::skips::SkipRow {
                at_ms: engine::now_ms(),
                bot_kind: kind.as_str().to_string(),
                signal_id: sig.id.clone(),
                symbol: sig.symbol.clone(),
                direction: if sig.direction == crate::signal::model::Direction::Long { "long" } else { "short" }.to_string(),
                market: sig.market_type.clone().unwrap_or_default(),
                combo: sig.combo.clone().unwrap_or_default(),
                reason: skip.key.to_string(),
                detail: skip.detail.clone().unwrap_or_default(),
            };
            match app.state::<StoreManager>().record_skip(&dir, &row) {
                Ok(true) => {}
                Ok(false) => return,
                Err(e) => self.push_skip(&sig.symbol, "storeWriteFailed", Some(e)),
            }
        }
        self.push_skip(&sig.symbol, skip.key, skip.detail);
    }

    /// True when `kind` already holds a position on this symbol+timeframe.
    pub fn holds_market(&self, kind: BotKind, symbol: &str, timeframe: &Option<String>) -> bool {
        self.positions
            .lock()
            .expect("bot mutex")
            .iter()
            .any(|p| p.bot_kind == kind && p.symbol == symbol && &p.timeframe == timeframe)
    }

    /// True when `kind` holds a position on `symbol`, any timeframe (the
    /// manual entry's rule: one hand-opened trade per symbol and bot).
    pub fn holds_symbol(&self, kind: BotKind, symbol: &str) -> bool {
        self.positions
            .lock()
            .expect("bot mutex")
            .iter()
            .any(|p| p.bot_kind == kind && p.symbol.eq_ignore_ascii_case(symbol))
    }

    /// Adds `pos` unless its bot already holds its signal or its symbol,
    /// checked and added under one lock. False = not added.
    pub fn add_position_if_free(&self, pos: OpenPosition) -> bool {
        let mut g = self.positions.lock().expect("bot mutex");
        let taken = g.iter().any(|p| {
            p.bot_kind == pos.bot_kind && (p.signal_id == pos.signal_id || p.symbol.eq_ignore_ascii_case(&pos.symbol))
        });
        if !taken {
            g.push(pos);
        }
        !taken
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
        self.watched
            .lock()
            .expect("bot mutex")
            .remove(&(signal_id.to_string(), kind));
    }

    /// Records an evaluation of (signal, bot) that left it open.
    pub fn mark_watched(&self, signal_id: &str, kind: BotKind, at_ms: u64) {
        self.watched
            .lock()
            .expect("bot mutex")
            .insert((signal_id.to_string(), kind), at_ms);
    }

    /// Whether (signal, bot) was evaluated within `WATCH_GAP_MS` of `now_ms`.
    pub fn watched_recently(&self, signal_id: &str, kind: BotKind, now_ms: u64) -> bool {
        self.watched
            .lock()
            .expect("bot mutex")
            .get(&(signal_id.to_string(), kind))
            .is_some_and(|&at| now_ms.saturating_sub(at) <= WATCH_GAP_MS)
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
        self.save_running(app);
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
    /// re-confirmed every session, never re-armed from a file on disk. A
    /// config that parses but fails today's validation is still loaded, so
    /// restored positions keep the bot's max-loss; `start` refuses it until
    /// it is saved again, and the feed says why.
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
                Ok(mut cfg) if cfg.kind == kind => {
                    cfg.live = false;
                    if let Err(code) = cfg.validate() {
                        self.push_skip("—", "restoreConfigInvalid", Some(code));
                    }
                    self.configure(cfg);
                }
                _ => self.push_skip("—", "restoreRowUnreadable", Some(format!("{} config", kind.as_str()))),
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

    /// Real money is in play: an open position or a bot switched to live.
    /// Reads the raw flags, not `is_live()`, so the answer is the same in
    /// every build. Updates, vault locks and key changes wait while it holds.
    pub fn has_live_exposure(&self) -> bool {
        self.positions.lock().expect("bot mutex").iter().any(|p| p.live)
            || self.configs.lock().expect("bot mutex").values().any(|c| c.live)
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
            pilot_left: self.pilot_left(BotKind::Futures),
            live_venues: crate::exchange::live_venues(),
            venue_sandbox: crate::exchange::venue::ccxt::sandbox_from_env(),
        }
    }
}

impl Default for BotManager {
    fn default() -> Self {
        Self::new()
    }
}

/// `running_kinds` body: the kinds in a fixed order, comma-separated.
fn running_body(running: &HashSet<BotKind>) -> String {
    [BotKind::Futures, BotKind::Spot, BotKind::Pump]
        .into_iter()
        .filter(|k| running.contains(k))
        .map(BotKind::as_str)
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_running(body: &str) -> Vec<BotKind> {
    body.split(',').filter_map(|s| BotKind::parse(s.trim())).collect()
}

#[cfg(test)]
mod running_tests {
    use super::*;

    // A paper stop books at its level only when price was seen on the safe
    // side moments ago (management::Fill); a restart, a sleep or a closed
    // position starts unwatched.
    #[test]
    fn a_position_is_watched_only_shortly_after_its_last_evaluation() {
        let m = BotManager::new();
        assert!(!m.watched_recently("s1", BotKind::Futures, 1_000), "restored: never seen");
        m.mark_watched("s1", BotKind::Futures, 1_000);
        assert!(m.watched_recently("s1", BotKind::Futures, 1_000 + WATCH_GAP_MS));
        assert!(!m.watched_recently("s1", BotKind::Futures, 1_001 + WATCH_GAP_MS), "a gap");
        assert!(!m.watched_recently("s1", BotKind::Spot, 1_000), "per bot");
        m.remove_position("s1", BotKind::Futures);
        assert!(!m.watched_recently("s1", BotKind::Futures, 1_000));
    }

    #[test]
    fn running_kinds_round_trip_and_skip_junk() {
        let set: HashSet<BotKind> = [BotKind::Spot, BotKind::Futures].into_iter().collect();
        let body = running_body(&set);
        assert_eq!(body, "futures,spot");
        assert_eq!(parse_running(&body), vec![BotKind::Futures, BotKind::Spot]);
        assert_eq!(parse_running(""), Vec::<BotKind>::new());
        assert_eq!(parse_running("pump, nonsense"), vec![BotKind::Pump]);
    }
}
