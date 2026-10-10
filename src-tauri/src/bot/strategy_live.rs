//! Real-money mirror for DCA / Grid bots (owner decision 2026-10-04: "the user
//! chooses paper or real"). Binance USDT-M futures only, `--features live`
//! builds only, per bot behind the typed LIVE word and a verified trade-only
//! key.
//!
//! HOW IT TRADES. The simulation (bot/strategy) still makes every decision on
//! closed 1m bars, exactly as on paper, and never touches the exchange. After
//! each engine pass this module compares the cycle's position with the
//! quantity it has already sent and sends the DIFFERENCE as one market order:
//! a safety-order fill becomes a market buy, a take-profit or a stop becomes a
//! reduce-only close, a grid level becomes a market buy or sell. The exchange
//! holds one closePosition stop at the cycle's protective level (stop-loss,
//! grid stop-out or drawdown stop), so a closed or offline app still exits.
//!
//! WHAT IT NEVER DOES. It never re-opens exposure the exchange took away. Its
//! own exchange stop firing is the bot's stop exit. A position closed any
//! other way (liquidation, a manual close) closes the simulated cycle and
//! stops the bot. A position that differs in size (another bot or a manual
//! trade on the symbol) halts the bot and keeps it under a stop. After a pass
//! that could not act (vault locked, Binance unreadable) it never catches up
//! at today's price: it halts. The user decides what happens next.
//!
//! The simulation rows stay paper rows (`strategy_bots.paper = 1`): they are
//! the decision book. What reached Binance is recorded separately in
//! `strategy_live_fills`, so real results are never mixed with simulated ones.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::exchange::providers::binance_requests::Protective;
use crate::exchange::providers::binance_rules::{close_parts, quantize_qty, quantize_price, LotStep};
use crate::exchange::ExchangeManager;
use crate::store::StoreManager;
use crate::vault::VaultManager;

use super::model::LIVE_TRADING_ENABLED;
use super::strategy::model::{BotRunState, ExitReason, MarketKind, StrategyConfig, StrategyParams};
use super::strategy::StrategyManager;
use super::BotManager;

/// The word the user types to switch one strategy bot to real money.
pub const LIVE_WORD: &str = "LIVE";
const META_KEY: &str = "strategy_live_state";
/// A stop moved by less than this (fraction of price) is left in place:
/// re-placing it every tick for noise would only burn requests.
const STOP_MOVE_MIN: f64 = 0.0005;

/// Per-bot real-money state. Persisted as one JSON map in the meta table.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LiveState {
    pub enabled: bool,
    /// Signed quantity this module believes is on the exchange for the bot.
    pub synced_qty: f64,
    /// Cycle the synced quantity belongs to.
    pub seq: u32,
    /// Orders sent in this cycle (client-id counter).
    pub order_n: u32,
    /// Cycle whose symbol was set to isolated margin + leverage.
    pub prepared_seq: u32,
    pub stop_algo_id: Option<i64>,
    pub stop_px: Option<f64>,
    /// Why the mirror stopped acting for this bot (i18n key), until the user
    /// switches live off and on again.
    pub halted: Option<String>,
    pub last_real_qty: f64,
    pub last_entry_price: f64,
    pub last_unrealized: f64,
    pub last_sync_ms: u64,
    /// An order was sent and its outcome not yet read back.
    #[serde(default)]
    pub pending_check: bool,
    /// Cycle the exchange closed while its simulated close is still owed.
    #[serde(default)]
    pub closed_seq: Option<u32>,
    /// Since when passes could not act (vault locked, Binance unreadable).
    #[serde(default)]
    pub blind_since: Option<u64>,
    /// Consecutive rejected orders.
    #[serde(default)]
    pub fail_count: u32,
    /// The bot's symbol, kept so a removed bot's position is still read.
    #[serde(default)]
    pub symbol: Option<String>,
    /// A real position has no exchange stop and the close failed: every pass
    /// retries the close until the account reads flat.
    #[serde(default)]
    pub unprotected: bool,
    /// Pilot: real cycles left at a reduced size (0 = full size).
    #[serde(default)]
    pub pilot_cycles_left: u32,
    /// Real size / simulated size for the current cycle (0 reads as 1).
    #[serde(default)]
    pub cycle_factor: f64,
    /// Exchange the real orders go to. The bot still decides on Binance
    /// prices; a state saved before venues existed reads as Binance.
    #[serde(default = "default_venue")]
    pub venue: String,
}

fn default_venue() -> String {
    "binance".into()
}

impl LiveState {
    fn venue(&self) -> &str {
        if self.venue.is_empty() {
            "binance"
        } else {
            &self.venue
        }
    }
}

/// Pilot (owner 2026-10-05): the first real cycles after LIVE is switched on
/// mirror a fraction of the simulated position, so a setup mistake costs
/// little. Prices, stops and decisions are the simulation's own.
pub const PILOT_CYCLES: u32 = 3;
/// Target real position notional (USDT) while the pilot runs.
pub const PILOT_STRATEGY_USDT: f64 = 100.0;

/// Pure: the pilot's real / simulated size ratio for one cycle. The full
/// position notional is budget x leverage; the factor brings it to
/// PILOT_STRATEGY_USDT, but never below what keeps the smallest order above
/// Binance's minimum (x1.1), and never above 1.
pub fn pilot_factor(budget: f64, leverage: f64, smallest_order: f64, min_notional: f64) -> f64 {
    let full = budget * leverage.max(1.0);
    if full <= 0.0 {
        return 1.0;
    }
    let mut f = (PILOT_STRATEGY_USDT / full).min(1.0);
    if smallest_order > 0.0 {
        f = f.max(min_notional * 1.1 / smallest_order);
    }
    f.clamp(0.0, 1.0)
}

/// What the UI shows per bot.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LiveView {
    pub bot_id: String,
    pub enabled: bool,
    pub halted: Option<String>,
    pub real_qty: f64,
    pub entry_price: f64,
    pub unrealized_usdt: f64,
    pub stop_price: Option<f64>,
    pub last_sync_ms: u64,
    /// Closed-cycle cash flow from the recorded fills (USDT), before fees.
    pub realized_gross_usdt: f64,
    /// Taker fee estimate on every recorded fill (0.05% of notional).
    pub fees_est_usdt: f64,
    pub fills: u32,
    /// Pilot cycles left, and the current cycle's real / simulated size.
    pub pilot_cycles_left: u32,
    pub cycle_factor: f64,
    /// Exchange the real orders go to.
    pub venue: String,
}

#[derive(Default)]
pub struct StrategyLiveManager {
    states: Mutex<HashMap<String, LiveState>>,
}

impl StrategyLiveManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: &str) -> Option<LiveState> {
        self.states.lock().expect("live mutex").get(id).cloned()
    }

    fn put(&self, id: &str, st: LiveState) {
        self.states.lock().expect("live mutex").insert(id.to_string(), st);
    }

    pub fn enabled_ids(&self) -> Vec<String> {
        self.states
            .lock()
            .expect("live mutex")
            .iter()
            .filter(|(_, s)| s.enabled)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Live strategy bots: (switched on, halted).
    pub fn counts(&self) -> (usize, usize) {
        let g = self.states.lock().expect("live mutex");
        (g.values().filter(|s| s.enabled).count(), g.values().filter(|s| s.halted.is_some()).count())
    }

    /// Real money in play: a bot switched to live, or a position this module
    /// still holds.
    pub fn has_exposure(&self) -> bool {
        self.states
            .lock()
            .expect("live mutex")
            .values()
            .any(|s| s.enabled || s.synced_qty != 0.0 || s.last_real_qty != 0.0)
    }

    fn snapshot(&self) -> HashMap<String, LiveState> {
        self.states.lock().expect("live mutex").clone()
    }
}

/// Real money is in play anywhere in the app: a signal position or bot, or a
/// live strategy bot. Updates, vault locks and key changes wait while it holds.
pub fn live_exposure(app: &AppHandle) -> bool {
    app.state::<BotManager>().has_live_exposure() || app.state::<StrategyLiveManager>().has_exposure()
}

/// Whether a live DCA / Grid bot owns `symbol` (one real position per
/// symbol: one-way mode nets every order on it into one position).
pub fn owns_symbol(app: &AppHandle, symbol: &str) -> bool {
    let smgr = app.state::<StrategyManager>();
    app.state::<StrategyLiveManager>()
        .snapshot()
        .iter()
        .filter(|(_, s)| s.enabled || s.synced_qty != 0.0)
        .any(|(id, _)| smgr.bot(id).is_some_and(|b| b.cfg.symbol == symbol))
}

fn dir(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok()
}

fn now_ms() -> u64 {
    super::engine::now_ms()
}

fn save(app: &AppHandle) {
    let Some(d) = dir(app) else { return };
    let snap = app.state::<StrategyLiveManager>().snapshot();
    if let Ok(v) = serde_json::to_string(&snap) {
        let _ = app.state::<StoreManager>().set_meta(&d, META_KEY, &v);
    }
}

/// The stored real-money states, read entry by entry so one entry that no
/// longer parses does not take the others with it.
#[derive(Debug, Default)]
pub(crate) struct StoredLive {
    pub states: HashMap<String, LiveState>,
    /// Bots whose entry no longer parses.
    pub unreadable: Vec<String>,
    /// The value (or the read itself) failed: no bot can be told apart.
    pub whole_unreadable: bool,
}

/// `raw`: Ok(None) = nothing stored, Err = the store could not be read.
pub(crate) fn parse_stored(raw: Result<Option<&str>, ()>) -> StoredLive {
    let mut out = StoredLive::default();
    let raw = match raw {
        Ok(Some(raw)) => raw,
        Ok(None) => return out,
        Err(()) => {
            out.whole_unreadable = true;
            return out;
        }
    };
    let Ok(map) = serde_json::from_str::<HashMap<String, serde_json::Value>>(raw) else {
        out.whole_unreadable = true;
        return out;
    };
    for (id, v) in map {
        match serde_json::from_value::<LiveState>(v) {
            Ok(st) => {
                out.states.insert(id, st);
            }
            Err(_) => out.unreadable.push(id),
        }
    }
    out.unreadable.sort();
    out
}

fn read_stored(app: &AppHandle) -> (StoredLive, Option<String>) {
    let Some(d) = dir(app) else {
        return (parse_stored(Err(())), None);
    };
    match app.state::<StoreManager>().get_meta(&d, META_KEY) {
        Ok(raw) => (parse_stored(Ok(raw.as_deref())), raw),
        Err(_) => (parse_stored(Err(())), None),
    }
}

/// Loads the persisted states at startup. A build without the `live` feature
/// keeps them (a later live build resumes) but stops those bots from starting
/// new cycles: it cannot reach the exchange to mirror them.
///
/// A state that cannot be read fails closed. It used to read as "no live
/// bots": a real DCA bot resumed on paper and the mirror forgot the real
/// position it holds. Now the raw value is copied to its own meta key before
/// anything can overwrite it, every bot it may concern stays stopped
/// (`stored_live_ids`), and the feed says so.
pub fn restore(app: &AppHandle) {
    let Some(d) = dir(app) else { return };
    let (stored, raw) = read_stored(app);
    if stored.whole_unreadable || !stored.unreadable.is_empty() {
        if let Some(raw) = raw {
            let key = format!("{META_KEY}.unreadable.{}", now_ms());
            let _ = app.state::<StoreManager>().set_meta(&d, &key, &raw);
        }
        let bots = app.state::<BotManager>();
        if stored.whole_unreadable {
            bots.push_skip("—", "restoreLiveBlocked", Some("DCA/Grid".to_string()));
        }
        let smgr = app.state::<StrategyManager>();
        for id in &stored.unreadable {
            stop_bot(app, id);
            let sym = smgr.bot(id).map(|b| b.cfg.symbol).unwrap_or_default();
            smgr.note(id, &sym, "liveStateUnreadable", None, now_ms());
            bots.push_skip(if sym.is_empty() { "—" } else { &sym }, "restoreLiveBlocked", Some(id.clone()));
        }
    }
    let mgr = app.state::<StrategyLiveManager>();
    for (id, mut st) in stored.states {
        // While the app was closed the mirror could not act, but the
        // simulation replays those bars on the first pass. Count the downtime
        // as blind, so orders it missed are never sent at today's price.
        if st.enabled && LIVE_TRADING_ENABLED {
            st.blind_since = Some(st.last_sync_ms.min(now_ms().saturating_sub(BLIND_GRACE_MS + 1)));
        }
        if st.enabled && !LIVE_TRADING_ENABLED {
            stop_bot(app, &id);
            let sym = app.state::<StrategyManager>().bot(&id).map(|b| b.cfg.symbol).unwrap_or_default();
            app.state::<StrategyManager>().note(&id, &sym, "liveBuildRequired", None, now_ms());
        }
        mgr.put(&id, st);
    }
    // Real money held or switched on: the engine loop must run even with no
    // open simulated cycle (a close owed to the exchange, a halted bot).
    if LIVE_TRADING_ENABLED && mgr.has_exposure() {
        app.state::<BotManager>().ensure_loop(app.clone());
    }
}

fn ensure_table(c: &rusqlite::Connection) -> Result<(), String> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS strategy_live_fills (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            bot_id TEXT NOT NULL,
            seq INTEGER NOT NULL,
            ts INTEGER NOT NULL,
            client_id TEXT,
            order_id INTEGER,
            kind TEXT NOT NULL,
            signed_qty REAL NOT NULL,
            price REAL NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_strategy_live_fills_bot ON strategy_live_fills(bot_id, seq);",
    )
    .map_err(|e| e.to_string())
}

#[allow(clippy::too_many_arguments)]
fn record_fill(app: &AppHandle, bot_id: &str, seq: u32, client_id: Option<&str>, order_id: i64, kind: &str, signed_qty: f64, price: f64) {
    let Some(d) = dir(app) else { return };
    let ts = now_ms();
    let _ = app.state::<StoreManager>().strategy(&d, |c| {
        ensure_table(c)?;
        c.execute(
            "INSERT INTO strategy_live_fills(bot_id, seq, ts, client_id, order_id, kind, signed_qty, price) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            rusqlite::params![bot_id, seq, ts as i64, client_id, order_id, kind, signed_qty, price],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    });
}

/// (realized gross over flat cycles, fee estimate, fill count).
fn fill_summary(app: &AppHandle, bot_id: &str) -> (f64, f64, u32) {
    let Some(d) = dir(app) else { return (0.0, 0.0, 0) };
    app.state::<StoreManager>()
        .strategy(&d, |c| {
            ensure_table(c)?;
            let mut stmt = c
                .prepare("SELECT seq, signed_qty, price FROM strategy_live_fills WHERE bot_id = ?1 ORDER BY id")
                .map_err(|e| e.to_string())?;
            let rows: Vec<(u32, f64, f64)> = stmt
                .query_map([bot_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map_err(|e| e.to_string())?
                .filter_map(Result::ok)
                .collect();
            Ok(summarize(&rows))
        })
        .unwrap_or((0.0, 0.0, 0))
}

/// Pure: cash flow of every cycle whose fills net to zero (closed), the
/// taker-fee estimate over all fills, and the count.
pub fn summarize(rows: &[(u32, f64, f64)]) -> (f64, f64, u32) {
    let mut per: HashMap<u32, (f64, f64)> = HashMap::new();
    let mut fees = 0.0;
    for (seq, q, p) in rows {
        let e = per.entry(*seq).or_default();
        e.0 += q;
        e.1 -= q * p;
        fees += (q * p).abs() * 0.0005;
    }
    let realized = per
        .values()
        .filter(|(q, _)| q.abs() < 1e-9)
        .map(|(_, cash)| cash)
        .sum();
    (realized, fees, rows.len() as u32)
}

/// Stops one strategy bot from starting new cycles (open cycle untouched).
fn stop_bot(app: &AppHandle, id: &str) {
    let mgr = app.state::<StrategyManager>();
    if let Some(mut b) = mgr.bot(id) {
        if b.state != BotRunState::Dead {
            b.state = BotRunState::Stopped;
        }
        super::strategy::engine::save_bot(app, &b);
        mgr.put_bot(b);
    }
}

/// Whether `cfg` has a protective level the exchange stop can rest at.
pub fn has_protective_level(cfg: &StrategyConfig) -> bool {
    if cfg.max_drawdown_pct.is_some_and(|p| p > 0.0) {
        return true;
    }
    match &cfg.params {
        StrategyParams::Dca(p) => p.sl_pct.is_some_and(|v| v > 0.0),
        StrategyParams::Grid(p) => p.stop_out_pct.is_some_and(|v| v > 0.0),
    }
}

/// "as" + last 8 of the bot id + "-" + seq + "-" + n (<= 36 chars, the
/// Binance newClientOrderId limit). "as" keeps these apart from the signal
/// bot's "ae" ids, which its reconciler treats as its own.
pub fn client_id(bot_id: &str, seq: u32, n: u32) -> String {
    let short: String = bot_id.chars().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect();
    format!("as{short}-{seq}-{n}")
}

/// The order the mirror sends this tick, decided purely.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Nothing,
    /// Close the whole real position (reduce-only, in parts if needed).
    Flatten { long: bool, qty: f64 },
    /// Add exposure: BUY when `long`, SELL when not.
    Open { long: bool, qty: f64 },
    /// Reduce part of a position held on side `long`.
    Reduce { long: bool, qty: f64 },
    /// The difference is below what the exchange accepts; wait for more.
    Wait,
}

/// Pure: the order that moves the real position from `synced` toward
/// `desired`. A sign change is done in two passes (reduce to flat first).
pub fn plan_step(synced: f64, desired: f64, lot: LotStep, price: f64, min_notional: f64) -> Step {
    let half_step = lot.step.max(1e-12) / 2.0;
    if (desired - synced).abs() < half_step {
        return Step::Nothing;
    }
    if desired.abs() < half_step {
        return if synced.abs() < half_step {
            Step::Nothing
        } else {
            Step::Flatten { long: synced > 0.0, qty: synced.abs() }
        };
    }
    let same_side = synced.abs() < half_step || (synced > 0.0) == (desired > 0.0);
    if same_side && desired.abs() > synced.abs() {
        let qty = quantize_qty(desired.abs() - synced.abs(), lot);
        if qty <= 0.0 || qty * price < min_notional {
            return Step::Wait;
        }
        return Step::Open { long: desired > 0.0, qty };
    }
    if same_side {
        // Smaller position on the same side.
        let qty = quantize_qty(synced.abs() - desired.abs(), lot);
        if qty <= 0.0 {
            return Step::Wait;
        }
        return Step::Reduce { long: synced > 0.0, qty };
    }
    // Opposite side: flat first, the new side on the next pass.
    Step::Flatten { long: synced > 0.0, qty: synced.abs() }
}

/// Pure: a stop is only placed on the adverse side of the market (a sell
/// stop below a long, a buy stop above a short); one on the wrong side would
/// fire at once. The simulation exits there on its next bar anyway.
pub fn stop_is_placeable(long: bool, stop: f64, mark: f64) -> bool {
    stop > 0.0 && mark > 0.0 && if long { stop < mark } else { stop > mark }
}

/// One mirror pass over every live strategy bot. Called from the engine tick
/// right after the strategy phase.
pub async fn sync_all(app: &AppHandle) {
    if !LIVE_TRADING_ENABLED {
        return;
    }
    let lmgr = app.state::<StrategyLiveManager>();
    let ids: Vec<String> = lmgr
        .snapshot()
        .into_iter()
        .filter(|(_, s)| s.enabled || s.synced_qty != 0.0 || s.last_real_qty != 0.0)
        .map(|(id, _)| id)
        .collect();
    if ids.is_empty() {
        return;
    }
    let smgr = app.state::<StrategyManager>();
    let _guard = smgr.op_lock.lock().await;
    let vault = app.state::<VaultManager>();
    let mut locked = false;
    for id in ids {
        let venue = lmgr.get(&id).map(|st| st.venue().to_string()).unwrap_or_else(default_venue);
        match vault.credential(&venue) {
            Some(cred) => sync_bot(app, &id, &cred).await,
            None => {
                let sym = smgr.bot(&id).map(|b| b.cfg.symbol).unwrap_or_default();
                smgr.note(&id, &sym, "liveVaultLocked", None, now_ms());
                mark_blind(&lmgr, &id);
                locked = true;
            }
        }
    }
    if locked {
        save(app);
    }
}

/// The mirror could not act this pass (vault locked, Binance unreadable).
fn mark_blind(lmgr: &StrategyLiveManager, id: &str) {
    if let Some(mut st) = lmgr.get(id) {
        st.blind_since.get_or_insert(now_ms());
        lmgr.put(id, st);
    }
}

async fn real_position(exchange: &ExchangeManager, cred: &crate::vault::model::ExchangeCredential, symbol: &str) -> Option<(f64, f64, f64)> {
    let acc = exchange.futures_account(cred).await.ok()?;
    Some(
        acc.positions
            .iter()
            .find(|p| p.symbol == symbol)
            .map(|p| (p.position_amt, p.entry_price, p.unrealized_pnl))
            .unwrap_or((0.0, 0.0, 0.0)),
    )
}

/// A blind spell shorter than this (a network blip, a few ticks) is caught
/// up normally; a longer one halts instead of trading at an unrelated price.
const BLIND_GRACE_MS: u64 = 120_000;

/// Consecutive order rejections after which the mirror stops retrying.
const MAX_ORDER_FAILURES: u32 = 3;

/// Outcome of keeping the exchange stop in line.
enum StopOutcome {
    Ok,
    /// A real position has no stop on the exchange and none could be placed.
    Unprotected,
}

/// Keeps one closePosition stop at `stop` for a real position of `real`.
/// The old stop is cancelled BEFORE the new one is placed (two closePosition
/// stops on one side are not documented as allowed); a cancel that fails
/// keeps the old stop and retries next pass, and a place that fails leaves
/// the position unprotected, which the caller closes.
#[allow(clippy::too_many_arguments)]
async fn keep_stop(
    exchange: &ExchangeManager,
    cred: &crate::vault::model::ExchangeCredential,
    symbol: &str,
    st: &mut LiveState,
    real: f64,
    stop: Option<f64>,
    price: f64,
    tick: f64,
) -> StopOutcome {
    if real == 0.0 {
        if let Some(algo) = st.stop_algo_id {
            if exchange.cancel_algo(cred, symbol, algo).await.is_ok() {
                st.stop_algo_id = None;
                st.stop_px = None;
            }
        }
        return StopOutcome::Ok;
    }
    let long = real > 0.0;
    let Some(stop) = stop.map(|s| quantize_price(s, tick)) else {
        return StopOutcome::Ok;
    };
    let moved = st.stop_px.map_or(true, |old| (old - stop).abs() / stop > STOP_MOVE_MIN);
    if !moved {
        return StopOutcome::Ok;
    }
    if !stop_is_placeable(long, stop, price) {
        // The market is already past the level: the simulation exits on its
        // next bar. An existing stop stays where it is.
        return StopOutcome::Ok;
    }
    if let Some(old) = st.stop_algo_id {
        if exchange.cancel_algo(cred, symbol, old).await.is_err() {
            return StopOutcome::Ok;
        }
        st.stop_algo_id = None;
        st.stop_px = None;
    }
    match exchange.place_protective(cred, symbol, long, Protective::Stop, stop).await {
        Ok(new_id) => {
            st.stop_algo_id = Some(new_id);
            st.stop_px = Some(stop);
            StopOutcome::Ok
        }
        Err(_) => StopOutcome::Unprotected,
    }
}

/// Closes a real position that could not be protected, follows with the
/// simulated cycle, and halts the bot.
async fn close_unprotected(app: &AppHandle, id: &str, cred: &crate::vault::model::ExchangeCredential, symbol: &str, st: &mut LiveState, price: f64) {
    let exchange = app.state::<ExchangeManager>();
    let smgr = app.state::<StrategyManager>();
    let real = st.synced_qty;
    let _ = exchange.reduce_market_all(cred, symbol, real > 0.0, real.abs()).await;
    match real_position(&exchange, cred, symbol).await {
        Some((q, _, _)) => {
            record_fill(app, id, st.seq, None, 0, "close", q - real, price);
            st.synced_qty = q;
            st.last_real_qty = q;
            st.unprotected = q != 0.0;
        }
        None => st.unprotected = true,
    }
    close_sim(app, &smgr, id, st, ExitReason::Stop).await;
    st.halted = Some("liveUnprotected".into());
    stop_bot(app, id);
    smgr.note(id, symbol, "liveUnprotected", None, now_ms());
}

/// Closes the simulated cycle after the real position left. If the close
/// cannot be priced it is retried by the engine (`owe_close`) and
/// `closed_seq` keeps the mirror from buying the position back meanwhile.
async fn close_sim(app: &AppHandle, smgr: &StrategyManager, id: &str, st: &mut LiveState, reason: ExitReason) {
    let Some((seq, _, _)) = smgr.live_target(id) else { return };
    st.closed_seq = Some(seq);
    if super::strategy::engine::close_one_locked(app, smgr, id, reason, false).await.is_err() {
        super::strategy::engine::owe_close(app, smgr, id, reason);
    }
}

/// Caller holds the strategy `op_lock`.
async fn sync_bot(app: &AppHandle, id: &str, cred: &crate::vault::model::ExchangeCredential) {
    let smgr = app.state::<StrategyManager>();
    let lmgr = app.state::<StrategyLiveManager>();
    let exchange = app.state::<ExchangeManager>();
    let Some(mut st) = lmgr.get(id) else { return };
    // A bot removed while it still held a real position keeps its last
    // symbol in the state, so the position is still read and closed.
    let bot = smgr.bot(id);
    let Some(symbol) = bot.as_ref().map(|b| b.cfg.symbol.clone()).or_else(|| st.symbol.clone()) else {
        return;
    };
    st.symbol = Some(symbol.clone());
    let now = now_ms();
    let note = |k: &str, d: Option<String>| smgr.note(id, &symbol, k, d, now);

    let Some((real, entry, unreal)) = real_position(&exchange, cred, &symbol).await else {
        note("liveExchangeCheckFailed", None);
        st.blind_since.get_or_insert(now_ms());
        lmgr.put(id, st);
        save(app);
        return;
    };
    (st.last_real_qty, st.last_entry_price, st.last_unrealized, st.last_sync_ms) = (real, entry, unreal, now);
    let Ok(rules) = exchange.symbol_rules(cred, &symbol).await else {
        note("liveExchangeCheckFailed", None);
        st.blind_since.get_or_insert(now_ms());
        lmgr.put(id, st);
        save(app);
        return;
    };
    let tol = rules.lot.step.max(1e-12) / 2.0;
    let price = smgr.feed(MarketKind::Futures, &symbol).map(|f| f.last_close).filter(|p| *p > 0.0);
    let Some(price) = exchange.status(&cred.exchange_id, &symbol).await.futures_price.or(price) else {
        note("priceUnavailable", None);
        st.blind_since.get_or_insert(now_ms());
        lmgr.put(id, st);
        save(app);
        return;
    };

    // 0. An order whose outcome was never read (a failed read after it, or
    //    the app stopped in between): the exchange holds the answer. Only
    //    this module trades the symbol, so its position is adopted.
    if st.pending_check {
        st.synced_qty = real;
        st.pending_check = false;
    }
    let target = smgr.live_target(id);
    if st.closed_seq.is_some() && target.map(|t| t.0) != st.closed_seq {
        st.closed_seq = None;
    }

    // Real money on a venue whose order path never passed a test-network
    // dry run (a state switched on before that gate existed): no new
    // exposure from here. It halts like any other refusal below.
    if st.enabled && st.halted.is_none() && !crate::exchange::live_venue_allowed(st.venue()) {
        st.halted = Some(crate::exchange::VENUE_NOT_DRY_RUN.into());
        stop_bot(app, id);
        note(crate::exchange::VENUE_NOT_DRY_RUN, Some(st.venue().to_string()));
    }

    // Halted (or the bot was removed): no new exposure. The position is
    // still read and kept under a stop; once the user has closed it on
    // Binance the bot can go back to paper.
    if st.halted.is_some() || bot.is_none() {
        st.synced_qty = real;
        // A position left without a stop after a failed close: close it now.
        if st.unprotected {
            if real.abs() < tol {
                st.unprotected = false;
            } else {
                let _ = exchange.reduce_market_all(cred, &symbol, real > 0.0, real.abs()).await;
                if let Some((q, _, _)) = real_position(&exchange, cred, &symbol).await {
                    if q.abs() < tol {
                        record_fill(app, id, st.seq, None, 0, "close", -real, price);
                        st.unprotected = false;
                    }
                    st.synced_qty = q;
                    st.last_real_qty = q;
                }
                note("liveUnprotected", None);
            }
            lmgr.put(id, st);
            save(app);
            return;
        }
        let stop = target.and_then(|t| t.2);
        if real.abs() < tol || stop.is_some() || st.stop_algo_id.is_some() {
            let qty = if real.abs() < tol { 0.0 } else { real };
            if let StopOutcome::Unprotected = keep_stop(&exchange, cred, &symbol, &mut st, qty, stop, price, rules.tick).await {
                close_unprotected(app, id, cred, &symbol, &mut st, price).await;
            }
        }
        if bot.is_none() && st.synced_qty.abs() >= tol {
            // Removed bot: nothing will manage this position. Close it.
            let _ = exchange.reduce_market_all(cred, &symbol, real > 0.0, real.abs()).await;
            if let Some((q, _, _)) = real_position(&exchange, cred, &symbol).await {
                st.synced_qty = q;
                st.last_real_qty = q;
            }
        }
        if bot.is_none() && st.synced_qty.abs() < tol && st.stop_algo_id.is_none() {
            st.enabled = false;
        }
        lmgr.put(id, st);
        save(app);
        return;
    }
    let bot = bot.expect("checked above");

    // 1. The exchange must hold what was sent. Anything else was done by
    //    someone else: never trade over it.
    if (real - st.synced_qty).abs() > tol {
        if real.abs() < tol {
            // Our own exchange stop firing is the bot's normal stop exit: the
            // exchange (live price) reaches it before the simulation (closed
            // 1m bars). Anything else (liquidation, a manual close) is not
            // the bot's doing: it stops and the user decides.
            let own_stop = match st.stop_algo_id {
                Some(algo) => exchange
                    .algo_status(cred, &symbol, algo)
                    .await
                    .is_ok_and(|s| crate::exchange::providers::binance_parse::algo_fired(&s)),
                None => false,
            };
            let was_running = super::strategy::driver::is_running(bot.state);
            let exit_px = if own_stop { st.stop_px } else { None }.unwrap_or(price);
            record_fill(app, id, st.seq, None, 0, "external", -st.synced_qty, exit_px);
            close_sim(app, &smgr, id, &mut st, if own_stop { ExitReason::Sl } else { ExitReason::Stop }).await;
            st.synced_qty = 0.0;
            st.stop_algo_id = None; // fired (own) or the position is gone
            st.stop_px = None;
            let _ = exchange.cancel_symbol_orders(cred, &symbol).await;
            if own_stop && was_running {
                // close_one_locked stops the bot; a normal stop exit leaves
                // it armed, as on paper.
                if let Some(mut b) = smgr.bot(id) {
                    b.state = bot.state;
                    super::strategy::engine::save_bot(app, &b);
                    smgr.put_bot(b);
                }
                note("liveStopFilled", None);
            } else {
                stop_bot(app, id);
                note("liveClosedOnExchange", None);
            }
        } else {
            st.halted = Some("liveMismatch".into());
            stop_bot(app, id);
            note("liveMismatch", Some(format!("{real} / {}", st.synced_qty)));
        }
        lmgr.put(id, st);
        save(app);
        return;
    }

    // 2. Move the real position to the simulated one.
    let (seq, mut desired, stop) = match target {
        Some((seq, q, s)) => (seq, q, s),
        None => (st.seq, 0.0, None),
    };
    if st.closed_seq == Some(seq) {
        // The exchange closed this cycle; its simulated close is still owed.
        desired = 0.0;
    }
    // Pilot / full size: the real position is the simulated one times the
    // cycle's ratio (fixed when the cycle started below).
    let factor = if st.cycle_factor > 0.0 { st.cycle_factor } else { 1.0 };
    if seq != st.seq {
        if st.synced_qty.abs() < tol {
            // A finished real cycle counts toward the pilot; the new one gets
            // its size ratio now and keeps it to the end (changing it mid-cycle
            // would buy or sell the difference at an unrelated price).
            if st.order_n > 0 && st.pilot_cycles_left > 0 {
                st.pilot_cycles_left -= 1;
            }
            st.cycle_factor = if st.pilot_cycles_left > 0 {
                let smallest = super::strategy::validate::smallest_order_notional(&bot.cfg);
                pilot_factor(bot.cfg.budget, f64::from(bot.cfg.leverage), smallest, rules.min_notional)
            } else {
                1.0
            };
            (st.seq, st.order_n) = (seq, 0);
        } else {
            // The simulation closed one cycle and opened the next on the same
            // bar: close the old real position first, open the new one on the
            // next pass, so each cycle's real fills stand on their own.
            desired = 0.0;
        }
    }
    let factor = if seq == st.seq && st.cycle_factor > 0.0 { st.cycle_factor } else { factor };
    let step = plan_step(st.synced_qty, desired * factor, rules.lot, price, rules.min_notional);
    // While the mirror could not act, the simulation kept trading. Sending
    // the whole difference now would buy or sell at today's price, not at
    // the levels the simulation used: stop instead, the user decides.
    let blind_long = st.blind_since.is_some_and(|t| now.saturating_sub(t) > BLIND_GRACE_MS);
    if blind_long && matches!(step, Step::Open { .. } | Step::Reduce { .. }) {
        st.halted = Some("liveMissedOrders".into());
        stop_bot(app, id);
        note("liveMissedOrders", None);
        lmgr.put(id, st);
        save(app);
        return;
    }
    st.blind_since = None;
    let mut sent = false;
    let mut failed = false;
    match step {
        Step::Nothing => {}
        Step::Wait if st.synced_qty.abs() < tol && desired.abs() >= tol => {
            // The cycle's first order is under Binance's minimum (losses
            // shrank it): the simulated position could never be real.
            close_sim(app, &smgr, id, &mut st, ExitReason::Manual).await;
            stop_bot(app, id);
            note("liveQtyBelowMinimum", None);
        }
        Step::Wait => note("livePartialBelowMinimum", None),
        Step::Open { long, qty } => {
            if st.prepared_seq != st.seq {
                match exchange.hedge_mode(cred, &symbol).await {
                    Ok(false) => {}
                    Ok(true) => {
                        st.halted = Some("liveHedgeMode".into());
                        stop_bot(app, id);
                        note("liveHedgeMode", None);
                        lmgr.put(id, st);
                        save(app);
                        return;
                    }
                    Err(_) => {
                        note("liveExchangeCheckFailed", None);
                        lmgr.put(id, st);
                        return;
                    }
                }
                // A leftover order on the flat symbol (an old stop, the
                // user's own limit) would act on the position about to open.
                match exchange.open_order_count(cred, &symbol).await {
                    Ok(0) => {}
                    Ok(_) => {
                        st.halted = Some("liveSymbolHasOrders".into());
                        stop_bot(app, id);
                        note("liveSymbolHasOrders", None);
                        lmgr.put(id, st);
                        save(app);
                        return;
                    }
                    Err(_) => {
                        note("liveExchangeCheckFailed", None);
                        lmgr.put(id, st);
                        return;
                    }
                }
                let lev = bot.cfg.leverage.max(1);
                if exchange.set_isolated(cred, &symbol).await.is_err()
                    || exchange.set_leverage(cred, &symbol, lev).await.is_err()
                {
                    note("liveLeverageRejected", None);
                    st.fail_count += 1;
                    if st.fail_count >= MAX_ORDER_FAILURES {
                        st.halted = Some("liveLeverageRejected".into());
                        stop_bot(app, id);
                    }
                    lmgr.put(id, st);
                    save(app);
                    return;
                }
                st.prepared_seq = st.seq;
            }
            // Split at MARKET_LOT_SIZE, like the closes.
            let parts = close_parts(qty, rules.market_max_qty, rules.lot);
            for part in parts {
                st.order_n += 1;
                st.pending_check = true;
                lmgr.put(id, st.clone());
                save(app); // the intent survives a crash: the next pass adopts
                let cid = client_id(id, st.seq, st.order_n);
                sent = true;
                match exchange.open_market(cred, &symbol, long, part, &cid).await {
                    Ok(fill) => {
                        record_fill(app, id, st.seq, Some(&cid), fill.order_id, "open", if long { fill.executed_qty } else { -fill.executed_qty }, fill.avg_price);
                    }
                    Err(e) => {
                        note("liveOrderFailed", Some(format!("{e:?}")));
                        failed = true;
                        break;
                    }
                }
            }
        }
        Step::Reduce { long, qty } => {
            st.pending_check = true;
            lmgr.put(id, st.clone());
            save(app);
            sent = true;
            match exchange.reduce_market_all(cred, &symbol, long, qty).await {
                Ok(fill) => {
                    record_fill(app, id, st.seq, None, fill.order_id, "reduce", if long { -fill.executed_qty } else { fill.executed_qty }, fill.avg_price);
                }
                Err(e) => {
                    note("livePartialFailed", Some(format!("{e:?}")));
                    failed = true;
                }
            }
        }
        Step::Flatten { long, qty } => {
            st.pending_check = true;
            lmgr.put(id, st.clone());
            save(app);
            sent = true;
            match exchange.reduce_market_all(cred, &symbol, long, qty).await {
                Ok(fill) => {
                    record_fill(app, id, st.seq, None, fill.order_id, "close", if long { -fill.executed_qty } else { fill.executed_qty }, fill.avg_price);
                }
                Err(e) => {
                    note("liveCloseFailed", Some(format!("{e:?}")));
                    failed = true;
                }
            }
        }
    }
    if failed {
        st.fail_count += 1;
        if st.fail_count >= MAX_ORDER_FAILURES && matches!(step, Step::Open { .. }) {
            // Rejected again and again (margin, size): stop rather than fill
            // the whole gap later at an unrelated price. Closes keep retrying.
            st.halted = Some("liveOrderFailed".into());
            stop_bot(app, id);
        }
    } else if sent {
        st.fail_count = 0;
    }
    // What the exchange now holds is what was sent: read, never assumed. A
    // failed read leaves `pending_check` set; the next pass adopts the
    // position, and the stop below uses the best known quantity meanwhile.
    if sent {
        if let Some((q, e, u)) = real_position(&exchange, cred, &symbol).await {
            st.synced_qty = q;
            st.pending_check = false;
            (st.last_real_qty, st.last_entry_price, st.last_unrealized) = (q, e, u);
        } else {
            note("liveExchangeCheckFailed", None);
            if !failed {
                st.synced_qty = desired_after(&step, st.synced_qty);
            }
        }
    }

    // 3. The exchange stop follows the cycle's protective level.
    let real_now = if st.synced_qty.abs() < tol { 0.0 } else { st.synced_qty };
    if let StopOutcome::Unprotected = keep_stop(&exchange, cred, &symbol, &mut st, real_now, stop, price, rules.tick).await {
        note("liveProtectiveStopFailed", None);
        close_unprotected(app, id, cred, &symbol, &mut st, price).await;
    }
    lmgr.put(id, st);
    save(app);
}

/// Best guess of the position after `step` when the account could not be
/// read back: used only to keep a stop on it until the next pass adopts the
/// real quantity (`pending_check`).
fn desired_after(step: &Step, synced: f64) -> f64 {
    match *step {
        Step::Open { long, qty } => synced + if long { qty } else { -qty },
        Step::Reduce { long, qty } => synced - if long { qty } else { -qty },
        Step::Flatten { .. } => 0.0,
        Step::Nothing | Step::Wait => synced,
    }
}

/// Why a live bot may not start new cycles now, or None. A halted mirror
/// would leave the cycles unmirrored, and with the vault locked the first
/// orders would be missed (and halt the bot two minutes later).
pub fn start_refusal(app: &AppHandle, id: &str) -> Option<&'static str> {
    let st = app.state::<StrategyLiveManager>().get(id)?;
    if !st.enabled {
        return None;
    }
    if st.halted.is_some() {
        return Some("liveHaltedStart");
    }
    if !crate::exchange::live_venue_allowed(st.venue()) {
        return Some(crate::exchange::VENUE_NOT_DRY_RUN);
    }
    if LIVE_TRADING_ENABLED && app.state::<VaultManager>().credential(st.venue()).is_none() {
        return Some("liveVaultLocked");
    }
    None
}

/// Whether the bot's settings are locked: real money is on, or a real
/// position is still held. Edits and archiving wait for paper.
/// Bots the stored real-money state still ties to the exchange (switched on,
/// or holding a real quantity): read from disk before `restore` has run, so
/// the strategy restore can keep exactly these bots stopped. An entry that
/// no longer parses counts as tied. None: the stored state cannot be read at
/// all, so no bot can be told apart and none may resume.
pub fn stored_live_ids(app: &AppHandle) -> Option<std::collections::HashSet<String>> {
    live_ids(&read_stored(app).0)
}

fn live_ids(stored: &StoredLive) -> Option<std::collections::HashSet<String>> {
    if stored.whole_unreadable {
        return None;
    }
    let tied = stored
        .states
        .iter()
        .filter(|(_, s)| s.enabled || s.synced_qty != 0.0 || s.last_real_qty != 0.0)
        .map(|(id, _)| id.clone());
    Some(tied.chain(stored.unreadable.iter().cloned()).collect())
}

pub fn locks_bot(app: &AppHandle, id: &str) -> bool {
    app.state::<StrategyLiveManager>()
        .get(id)
        .is_some_and(|s| s.enabled || s.synced_qty != 0.0 || s.last_real_qty != 0.0)
}

// ---------------------------------------------------------------- report

/// One real fill as recorded by the mirror.
#[derive(Clone, Debug, PartialEq)]
pub struct RealFill {
    pub seq: u32,
    pub ts: u64,
    pub kind: String,
    pub signed_qty: f64,
    pub price: f64,
}

/// One simulated fill (strategy_fills, kind "fill").
#[derive(Clone, Debug, PartialEq)]
pub struct SimFill {
    pub seq: u32,
    pub ts: u64,
    pub signed_qty: f64,
    pub price: f64,
}

/// A real fill against the simulated fills it mirrored.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiffRow {
    pub seq: u32,
    pub ts: u64,
    pub kind: String,
    /// "buy" | "sell"
    pub side: &'static str,
    pub real_qty: f64,
    pub real_price: f64,
    /// Volume-weighted simulated price; None when the exchange made the fill
    /// (its own stop) or no simulated fill matched.
    pub sim_price: Option<f64>,
    /// Positive = worse than the simulation (paid more, received less).
    pub slippage_bps: Option<f64>,
    pub slippage_usdt: Option<f64>,
    /// Seconds from the last matched simulated fill to the real one.
    pub delay_s: Option<f64>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiffSummary {
    pub matched: u32,
    pub unmatched: u32,
    /// Notional-weighted mean slippage of the matched fills.
    pub avg_slippage_bps: Option<f64>,
    pub slippage_usdt: f64,
    pub avg_delay_s: Option<f64>,
    /// 0.05% taker estimate over every real fill.
    pub fees_est_usdt: f64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiffReport {
    pub rows: Vec<DiffRow>,
    pub summary: DiffSummary,
}

/// Pure: each real fill against the simulated fills of the same cycle and
/// side made since the previous real fill of that cycle (one market order
/// can mirror several simulated fills: a grid crossing two levels in a bar).
pub fn diff_report(real: &[RealFill], sim: &[SimFill]) -> DiffReport {
    let mut real: Vec<RealFill> = real.to_vec();
    real.sort_by_key(|r| (r.seq, r.ts));
    let mut used = vec![false; sim.len()];
    let mut rows = Vec::with_capacity(real.len());
    let mut summary = DiffSummary::default();
    let (mut w_bps, mut w_notional, mut delay_sum) = (0.0, 0.0, 0.0);
    for r in &real {
        let sign = if r.signed_qty >= 0.0 { 1.0 } else { -1.0 };
        let notional = (r.signed_qty * r.price).abs();
        summary.fees_est_usdt += notional * 0.0005;
        let (mut q, mut v, mut last_ts) = (0.0, 0.0, 0u64);
        if r.kind != "external" {
            for (i, s) in sim.iter().enumerate() {
                if used[i] || s.seq != r.seq || s.ts > r.ts || (s.signed_qty >= 0.0) != (sign > 0.0) {
                    continue;
                }
                used[i] = true;
                q += s.signed_qty.abs();
                v += s.signed_qty.abs() * s.price;
                last_ts = last_ts.max(s.ts);
            }
        }
        let sim_price = (q > 0.0).then(|| v / q);
        let slippage_bps = sim_price.map(|p| sign * (r.price - p) / p * 10_000.0);
        let slippage_usdt = sim_price.map(|p| sign * (r.price - p) * r.signed_qty.abs());
        let delay_s = sim_price.map(|_| r.ts.saturating_sub(last_ts) as f64 / 1000.0);
        match (slippage_bps, slippage_usdt, delay_s) {
            (Some(b), Some(u), Some(d)) => {
                summary.matched += 1;
                w_bps += b * notional;
                w_notional += notional;
                summary.slippage_usdt += u;
                delay_sum += d;
            }
            _ => summary.unmatched += 1,
        }
        rows.push(DiffRow {
            seq: r.seq,
            ts: r.ts,
            kind: r.kind.clone(),
            side: if sign > 0.0 { "buy" } else { "sell" },
            real_qty: r.signed_qty.abs(),
            real_price: r.price,
            sim_price,
            slippage_bps,
            slippage_usdt,
            delay_s,
        });
    }
    summary.avg_slippage_bps = (w_notional > 0.0).then(|| w_bps / w_notional);
    summary.avg_delay_s = (summary.matched > 0).then(|| delay_sum / f64::from(summary.matched));
    rows.reverse(); // newest first
    DiffReport { rows, summary }
}

/// Real against simulated fills of one live DCA / Grid bot.
#[tauri::command]
pub fn strategy_live_report(app: AppHandle, id: String) -> Result<DiffReport, String> {
    let d = dir(&app).ok_or_else(|| "storeReadFailed".to_string())?;
    let (real, sim) = app.state::<StoreManager>().strategy(&d, |c| {
        ensure_table(c)?;
        let mut st = c
            .prepare("SELECT seq, ts, kind, signed_qty, price FROM strategy_live_fills WHERE bot_id = ?1 ORDER BY id DESC LIMIT 500")
            .map_err(|e| e.to_string())?;
        let real: Vec<RealFill> = st
            .query_map([&id], |r| {
                Ok(RealFill { seq: r.get(0)?, ts: r.get::<_, i64>(1)? as u64, kind: r.get(2)?, signed_qty: r.get(3)?, price: r.get(4)? })
            })
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect();
        let first_seq = real.iter().map(|r| r.seq).min().unwrap_or(0);
        let mut st = c
            .prepare("SELECT seq, ts, qty, price FROM strategy_fills WHERE bot_id = ?1 AND kind = 'fill' AND seq >= ?2 ORDER BY ts")
            .map_err(|e| e.to_string())?;
        let sim: Vec<SimFill> = st
            .query_map(rusqlite::params![&id, first_seq], |r| {
                Ok(SimFill { seq: r.get(0)?, ts: r.get::<_, i64>(1)? as u64, signed_qty: r.get(2)?, price: r.get(3)? })
            })
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect();
        Ok((real, sim))
    })?;
    Ok(diff_report(&real, &sim))
}

/// Every live bot's real-money view.
pub fn views(app: &AppHandle) -> Vec<LiveView> {
    let states = app.state::<StrategyLiveManager>().snapshot();
    states
        .into_iter()
        .map(|(id, s)| {
            let (realized, fees, fills) = fill_summary(app, &id);
            let venue = s.venue().to_string();
            LiveView {
                bot_id: id,
                enabled: s.enabled,
                halted: s.halted,
                real_qty: s.last_real_qty,
                entry_price: s.last_entry_price,
                unrealized_usdt: s.last_unrealized,
                stop_price: s.stop_px,
                last_sync_ms: s.last_sync_ms,
                realized_gross_usdt: realized,
                fees_est_usdt: fees,
                fills,
                pilot_cycles_left: s.pilot_cycles_left,
                cycle_factor: if s.cycle_factor > 0.0 { s.cycle_factor } else { 1.0 },
                venue,
            }
        })
        .collect()
}

#[tauri::command]
pub fn strategy_live_status(app: AppHandle) -> Result<Vec<LiveView>, String> {
    Ok(views(&app))
}

/// Ends the pilot: from the next cycle the bot mirrors at full size. The
/// running cycle keeps its ratio (changing it mid-cycle would trade the
/// difference at an unrelated price).
#[tauri::command]
pub fn strategy_end_pilot(app: AppHandle, id: String) -> Result<Vec<LiveView>, String> {
    let lmgr = app.state::<StrategyLiveManager>();
    let mut st = lmgr.get(&id).ok_or_else(|| "botUnknown".to_string())?;
    st.pilot_cycles_left = 0;
    lmgr.put(&id, st);
    save(&app);
    Ok(views(&app))
}

/// Why one DCA / Grid bot may not switch to real money on `venue`, or None.
/// Pure (build switch and sandbox passed in); the key, cycle, symbol and
/// exchange checks follow in the command. Decisions come from Binance
/// futures prices; orders go only to a venue whose order path passed its
/// test-network dry run (`exchange::DRY_RUN_PASSED`), or to any order venue
/// while the app runs against the test networks for that dry run.
pub fn enable_refusal(live_build: bool, cfg: &StrategyConfig, venue: &str, confirmation: &str, sandbox: bool) -> Option<String> {
    if !live_build {
        return Some("liveBuildDisabled".into());
    }
    if cfg.exchange_id != "binance" || cfg.market != MarketKind::Futures || !crate::exchange::has_order_path(venue) {
        return Some("liveFuturesBinanceOnly".into());
    }
    if let Err(code) = crate::exchange::live_venue_check_in(venue, sandbox) {
        return Some(code);
    }
    if confirmation.trim() != LIVE_WORD {
        return Some(format!("liveConfirmRequired|{LIVE_WORD}"));
    }
    None
}

/// Switches one DCA / Grid bot between paper and real money.
#[tauri::command]
pub async fn strategy_set_live(
    app: AppHandle,
    id: String,
    enabled: bool,
    confirmation: String,
    venue: Option<String>,
) -> Result<Vec<LiveView>, String> {
    let venue = venue.filter(|v| !v.is_empty()).unwrap_or_else(default_venue);
    let smgr = app.state::<StrategyManager>();
    let lmgr = app.state::<StrategyLiveManager>();
    let _guard = smgr.op_lock.lock().await;
    let bot = smgr.bot(&id).ok_or_else(|| "botUnknown".to_string())?;
    let cur = lmgr.get(&id).unwrap_or_default();
    if !enabled {
        // Off only once nothing is held: the mirror is what closes it.
        if cur.synced_qty != 0.0 || cur.last_real_qty != 0.0 || smgr.has_open_cycle(&id) && cur.enabled {
            return Err("liveCloseFirst".into());
        }
        // A stop left on the flat symbol would act on the next position
        // anyone opens there: it goes before live is switched off.
        if let Some(algo) = cur.stop_algo_id {
            if LIVE_TRADING_ENABLED {
                let cred = app.state::<VaultManager>().credential(cur.venue()).ok_or_else(|| "liveNeedsTradeKey".to_string())?;
                app.state::<ExchangeManager>()
                    .cancel_algo(&cred, &bot.cfg.symbol, algo)
                    .await
                    .map_err(|_| "liveExchangeCheckFailed".to_string())?;
            }
        }
        lmgr.put(&id, LiveState { enabled: false, stop_algo_id: None, stop_px: None, halted: None, ..cur });
        save(&app);
        return Ok(views(&app));
    }
    let sandbox = crate::exchange::venue::ccxt::sandbox_from_env();
    if let Some(refusal) = enable_refusal(LIVE_TRADING_ENABLED, &bot.cfg, &venue, &confirmation, sandbox) {
        return Err(refusal);
    }
    match app.state::<VaultManager>().credential(&venue).map(|c| c.permission) {
        Some(crate::vault::model::CredentialPermission::TradeOnly) => {}
        Some(_) => return Err("liveNeedsVerifiedKey".into()),
        None => return Err("liveNeedsTradeKey".into()),
    }
    // A paper cycle already running would be mirrored mid-way at today's
    // price; start real money on a fresh cycle.
    if smgr.has_open_cycle(&id) {
        return Err("liveNeedsIdleBot".into());
    }
    if !has_protective_level(&bot.cfg) {
        return Err("liveNeedsStop".into());
    }
    // One real position per symbol (one-way mode nets them).
    let busy_strategy = lmgr
        .enabled_ids()
        .into_iter()
        .filter(|other| other != &id)
        .any(|other| smgr.bot(&other).is_some_and(|b| b.cfg.symbol == bot.cfg.symbol));
    let busy_signal = app
        .state::<BotManager>()
        .positions_snapshot()
        .iter()
        .any(|p| p.live && p.symbol == bot.cfg.symbol);
    if busy_strategy || busy_signal {
        return Err("liveSymbolBusy".into());
    }
    // The smallest order must clear the venue's minimum for this symbol.
    let exchange = app.state::<ExchangeManager>();
    let cred = app.state::<VaultManager>().credential(&venue).ok_or_else(|| "liveNeedsTradeKey".to_string())?;
    let rules = exchange.symbol_rules(&cred, &bot.cfg.symbol).await.map_err(|_| "liveExchangeCheckFailed".to_string())?;
    if !rules.trading {
        return Err("liveSymbolNotTrading".into());
    }
    let smallest = super::strategy::validate::smallest_order_notional(&bot.cfg);
    if smallest < rules.min_notional {
        return Err(format!("liveOrderBelowMinNotional|{:.2}", rules.min_notional));
    }
    // The symbol must be flat on the account: the mirror only ever trades
    // the difference from what it sent itself.
    let held = real_position(&exchange, &cred, &bot.cfg.symbol)
        .await
        .ok_or_else(|| "liveExchangeCheckFailed".to_string())?;
    if held.0 != 0.0 {
        return Err("liveSymbolBusy".into());
    }
    lmgr.put(
        &id,
        LiveState {
            enabled: true,
            halted: None,
            synced_qty: 0.0,
            stop_algo_id: None,
            stop_px: None,
            pending_check: false,
            closed_seq: None,
            blind_since: None,
            fail_count: 0,
            symbol: Some(bot.cfg.symbol.clone()),
            unprotected: false,
            pilot_cycles_left: PILOT_CYCLES,
            cycle_factor: 0.0,
            venue,
            ..cur
        },
    );
    save(&app);
    app.state::<BotManager>().ensure_loop(app.clone());
    Ok(views(&app))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOT: LotStep = LotStep { step: 0.001, min_qty: 0.001 };

    // A real-money state that cannot be read must keep its bots stopped. It
    // used to read as "no live bots", and a live DCA bot resumed on paper.
    #[test]
    fn an_unreadable_live_state_fails_closed() {
        let on = serde_json::to_value(LiveState { enabled: true, ..Default::default() }).unwrap();
        let off = serde_json::to_value(LiveState::default()).unwrap();
        let raw = serde_json::json!({ "sb_on": on, "sb_off": off, "sb_bad": {"enabled": "yes"} }).to_string();
        let got = parse_stored(Ok(Some(&raw)));
        assert!(!got.whole_unreadable);
        assert_eq!(got.unreadable, vec!["sb_bad".to_string()]);
        assert_eq!(got.states.len(), 2, "one bad entry does not take the others with it");
        let ids = live_ids(&got).expect("readable");
        assert!(ids.contains("sb_on") && ids.contains("sb_bad") && !ids.contains("sb_off"));

        for broken in [parse_stored(Ok(Some("{"))), parse_stored(Ok(Some("[1]"))), parse_stored(Err(()))] {
            assert!(broken.whole_unreadable);
            assert_eq!(live_ids(&broken), None, "no bot can be told apart");
        }
        let none = parse_stored(Ok(None));
        assert_eq!(live_ids(&none), Some(Default::default()), "nothing stored: nothing tied");
    }

    #[test]
    fn steps_follow_the_simulated_position() {
        // first buy
        assert_eq!(plan_step(0.0, 0.010, LOT, 100_000.0, 100.0), Step::Open { long: true, qty: 0.010 });
        // a safety order adds
        assert_eq!(plan_step(0.010, 0.025, LOT, 100_000.0, 100.0), Step::Open { long: true, qty: 0.015 });
        // take profit closes all
        assert_eq!(plan_step(0.025, 0.0, LOT, 100_000.0, 100.0), Step::Flatten { long: true, qty: 0.025 });
        // a grid sell reduces a long
        assert_eq!(plan_step(0.030, 0.020, LOT, 100_000.0, 100.0), Step::Reduce { long: true, qty: 0.010 });
        // short side mirrors
        assert_eq!(plan_step(0.0, -0.002, LOT, 100_000.0, 100.0), Step::Open { long: false, qty: 0.002 });
        assert_eq!(plan_step(-0.002, -0.001, LOT, 100_000.0, 100.0), Step::Reduce { long: false, qty: 0.001 });
        // crossing zero goes flat first
        assert_eq!(plan_step(0.002, -0.003, LOT, 100_000.0, 100.0), Step::Flatten { long: true, qty: 0.002 });
        // nothing to do
        assert_eq!(plan_step(0.010, 0.0104, LOT, 100_000.0, 100.0), Step::Nothing);
    }

    #[test]
    fn an_add_below_the_exchange_minimum_waits() {
        // 0.0019 floors to 0.001 BTC = 50 USDT at 50k, under a 100 minimum
        assert_eq!(plan_step(0.010, 0.0119, LOT, 50_000.0, 100.0), Step::Wait);
        // under one lot
        assert_eq!(plan_step(0.0, 0.0009, LOT, 100_000.0, 5.0), Step::Wait);
    }

    #[test]
    fn the_pilot_shrinks_a_cycle_but_keeps_orders_above_the_minimum() {
        // 1000 USDT at 2x = 2000 notional -> 100 / 2000 = 5%
        assert!((pilot_factor(1000.0, 2.0, 200.0, 5.0) - 0.05).abs() < 1e-12);
        // smallest order 200 x 5% = 10 < 1.1 x 100 (BTC minimum) -> raised to 0.55
        assert!((pilot_factor(1000.0, 2.0, 200.0, 100.0) - 0.55).abs() < 1e-12);
        // a small bot is already under the pilot size -> full size
        assert_eq!(pilot_factor(40.0, 2.0, 20.0, 5.0), 1.0);
        // never above 1
        assert_eq!(pilot_factor(1000.0, 2.0, 50.0, 100.0), 1.0);
    }

    #[test]
    fn the_report_matches_each_real_fill_to_its_simulated_fills() {
        let sim = vec![
            SimFill { seq: 1, ts: 1_000, signed_qty: 0.01, price: 100.0 },
            SimFill { seq: 1, ts: 61_000, signed_qty: 0.01, price: 98.0 },
            SimFill { seq: 1, ts: 61_000, signed_qty: 0.01, price: 96.0 },
            SimFill { seq: 1, ts: 200_000, signed_qty: -0.03, price: 101.0 },
        ];
        let real = vec![
            RealFill { seq: 1, ts: 4_000, kind: "open".into(), signed_qty: 0.01, price: 100.1 },
            // one market order mirrored two grid levels: VWAP 97
            RealFill { seq: 1, ts: 65_000, kind: "open".into(), signed_qty: 0.02, price: 97.2 },
            RealFill { seq: 1, ts: 203_000, kind: "close".into(), signed_qty: -0.03, price: 100.9 },
            RealFill { seq: 2, ts: 300_000, kind: "external".into(), signed_qty: -0.01, price: 90.0 },
        ];
        let rep = diff_report(&real, &sim);
        assert_eq!(rep.rows.len(), 4);
        let by_ts = |ts| rep.rows.iter().find(|r| r.ts == ts).unwrap().clone();
        let a = by_ts(4_000);
        assert!((a.slippage_bps.unwrap() - 10.0).abs() < 1e-6, "bought 0.1 above: +10 bps worse");
        assert!((a.delay_s.unwrap() - 3.0).abs() < 1e-9);
        let b = by_ts(65_000);
        assert!((b.sim_price.unwrap() - 97.0).abs() < 1e-9);
        let c = by_ts(203_000);
        assert_eq!(c.side, "sell");
        assert!(c.slippage_bps.unwrap() > 0.0, "sold below the simulation: worse");
        assert!(by_ts(300_000).sim_price.is_none(), "an exchange-made fill has no simulated twin");
        assert_eq!(rep.summary.matched, 3);
        assert_eq!(rep.summary.unmatched, 1);
        assert!(rep.summary.slippage_usdt > 0.0);
        assert_eq!(rep.rows[0].ts, 300_000, "newest first");
    }

    #[test]
    fn stops_only_rest_on_the_adverse_side() {
        assert!(stop_is_placeable(true, 95.0, 100.0));
        assert!(!stop_is_placeable(true, 101.0, 100.0));
        assert!(stop_is_placeable(false, 105.0, 100.0));
        assert!(!stop_is_placeable(false, 99.0, 100.0));
        assert!(!stop_is_placeable(true, 0.0, 100.0));
    }

    #[test]
    fn real_money_goes_only_to_a_dry_run_venue() {
        let cfg = super::super::strategy::commands::default_config(
            super::super::strategy::model::StrategyKind::Dca,
            "binance".into(),
            "BTCUSDT".into(),
        );
        assert_eq!(enable_refusal(true, &cfg, "binance", "LIVE", false), None);
        for venue in ["bybit", "okx"] {
            assert_eq!(
                enable_refusal(true, &cfg, venue, "LIVE", false),
                Some(format!("liveVenueNotDryRun|{venue}")),
                "{venue}"
            );
        }
        // Bitget has no order path (removed 2026-10-09), sandbox or not.
        for sandbox in [false, true] {
            assert_eq!(enable_refusal(true, &cfg, "bitget", "LIVE", sandbox), Some("liveFuturesBinanceOnly".into()));
        }
        // The sandbox dry run may send them (orders go to test networks).
        assert_eq!(enable_refusal(true, &cfg, "bybit", "LIVE", true), None);
        assert_eq!(enable_refusal(true, &cfg, "okx", "LIVE", true), None);
        assert_eq!(enable_refusal(true, &cfg, "mexc", "LIVE", true), Some("liveFuturesBinanceOnly".into()));
        assert_eq!(enable_refusal(true, &cfg, "binance", "", false), Some("liveConfirmRequired|LIVE".into()));
        assert_eq!(enable_refusal(false, &cfg, "binance", "LIVE", false), Some("liveBuildDisabled".into()));
    }

    #[test]
    fn client_ids_fit_binance_and_stay_apart_from_signal_ids() {
        let id = client_id("sb_abcdefghijkl", 12, 345);
        assert_eq!(id, "asefghijkl-12-345");
        assert!(id.starts_with("as"));
        assert!(id.len() <= 36);
    }

    #[test]
    fn realized_counts_only_flat_cycles() {
        let rows = vec![
            (1, 0.01, 100_000.0),
            (1, -0.01, 101_000.0), // +10 USDT
            (2, 0.02, 100_000.0),  // still open
        ];
        let (realized, fees, n) = summarize(&rows);
        assert!((realized - 10.0).abs() < 1e-9);
        assert_eq!(n, 3);
        assert!((fees - (1000.0 + 1010.0 + 2000.0) * 0.0005).abs() < 1e-9);
    }
}
