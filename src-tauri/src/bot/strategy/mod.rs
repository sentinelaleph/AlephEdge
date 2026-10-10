//! DCA and Grid strategy bots — PAPER ONLY in v1.
//!
//! Not `BotKind` variants: `BotKind` is mounted by the paper runner and is
//! part of the signed phone protocol. Strategy bots are multi-instance
//! objects keyed by their own id, in this desktop-only subtree.
//!
//! Layout: pure core (`model`, `costs`, `path`, `cycle`, `dca`, `grid`,
//! `liquidation`, `accounting`, `driver`, `validate`, `limits`, `venue`,
//! `presets`, `stats`, `backtest`) with no Tauri and no I/O; `engine` (tick phase, I/O)
//! and `commands` (Tauri surface). This file holds the managed state and no
//! Tauri import.
//!
//! Three independent locks keep real money out: no `live` field in
//! `StrategyConfig`, `CHECK(paper = 1)` on `strategy_bots`, and only a
//! `PaperVenue` exists (`limits::STRATEGY_LIVE_ALLOWED` = false).

pub mod accounting;
pub mod backtest;
pub mod backtest_commands;
pub mod commands;
pub mod costs;
pub mod cycle;
pub mod dca;
pub mod driver;
pub mod engine;
pub mod grid;
pub mod limits;
pub mod liquidation;
pub mod model;
pub mod path;
pub mod presets;
pub mod stats;
pub mod validate;
pub mod venue;

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use serde::Serialize;

use cycle::CycleState;
use model::{
    BotId, BotRunState, ExitReason, MarketKind, StrategyBot, StrategyKind, StrategyNote,
};
use venue::PaperVenue;

const NOTE_CAP: usize = 50;

/// Price-feed state of one (market, symbol).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Feed {
    /// Close time of the last processed closed bar (ms).
    pub last_close_ms: u64,
    pub last_close: f64,
    /// Last successful fetch (ms).
    pub last_ok_ms: u64,
}

/// Strategy risk settings (tighten-only against the defaults).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyRisk {
    pub portfolio_dd_pct: f64,
    /// User's own budget cap, % of balance; None = the level's value.
    #[serde(default)]
    pub budget_cap_pct: Option<f64>,
}

impl Default for StrategyRisk {
    fn default() -> Self {
        Self {
            portfolio_dd_pct: limits::PORTFOLIO_DD_STOP_PCT,
            budget_cap_pct: None,
        }
    }
}

#[derive(Default)]
struct Inner {
    bots: HashMap<BotId, StrategyBot>,
    cycles: HashMap<BotId, CycleState>,
    venues: HashMap<BotId, PaperVenue>,
    feeds: HashMap<(MarketKind, String), Feed>,
    /// Bots whose feed is stale (fills and new cycles held).
    paused: HashMap<BotId, &'static str>,
    notes: VecDeque<StrategyNote>,
    last_note: HashMap<BotId, String>,
    risk: StrategyRisk,
    portfolio_peak: f64,
    /// UTC day start (ms) of the last portfolio-breaker trip; 0 = never.
    tripped_day: u64,
    /// (UTC day start, bot -> equity at the first tick of that day).
    day_snapshot: (u64, HashMap<BotId, f64>),
    last_compact_ms: u64,
    /// Forced closes (kill switch, breaker, remote kill) that found no
    /// honest price; retried every tick until the cycle is flat. Mirrored
    /// in the store (`engine::META_PENDING_CLOSE`) so a restart keeps them.
    pending_close: HashMap<BotId, ExitReason>,
}

/// Tauri-managed state of every strategy bot.
pub struct StrategyManager {
    inner: Mutex<Inner>,
    /// Serialises the engine tick with the commands that move cycles
    /// (close, close all), so a close can never race a bar walk.
    pub(crate) op_lock: tokio::sync::Mutex<()>,
    pub(crate) client: reqwest::Client,
}

/// Open-cycle summary for the views.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OpenCycleView {
    pub seq: u32,
    pub opened_at: u64,
    pub anchor_price: f64,
    pub avg_entry: Option<f64>,
    pub signed_qty: f64,
    pub notional: f64,
    pub margin: f64,
    pub reserved: f64,
    pub cash_quote: f64,
    pub unrealized_quote: f64,
    pub fees_quote: f64,
    pub funding_quote: f64,
    pub so_filled: Option<u8>,
    pub grid_closing_fills: Option<u32>,
    pub liq_price: Option<f64>,
    pub max_adverse_pct: f64,
    /// Grid without stop-out, price outside its range: no fill possible.
    pub out_of_range: bool,
    pub funding_unknown: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StrategyBotView {
    pub id: BotId,
    pub kind: StrategyKind,
    pub name: String,
    pub exchange_id: String,
    pub symbol: String,
    pub side: model::Side,
    pub market: MarketKind,
    pub leverage: u8,
    /// Always true in v1.
    pub paper: bool,
    pub run_state: &'static str,
    /// True while the bot may open NEW cycles (Start pressed, not paused by
    /// the user). An open cycle is managed either way, so `run_state` alone
    /// cannot tell a paused bot holding a cycle from a running one.
    pub accepting_new_cycles: bool,
    pub pause_reason: Option<&'static str>,
    pub budget: f64,
    pub realized_quote: f64,
    pub realized_pct: f64,
    pub mtm_pct: f64,
    pub equity: f64,
    pub drawdown_pct: f64,
    pub max_drawdown_pct: f64,
    pub cycles_done: u32,
    pub dead_reason: Option<ExitReason>,
    pub created_at: u64,
    pub preset_id: Option<String>,
    /// Last closed-bar price the MTM uses, and its close time.
    pub mark_price: Option<f64>,
    pub mark_ts: Option<u64>,
    pub open_cycle: Option<OpenCycleView>,
    pub last_note: Option<StrategyNote>,
    pub utilisation_in_position: f64,
    pub utilisation_committed: f64,
}

impl StrategyManager {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            op_lock: tokio::sync::Mutex::new(()),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .build()
                .unwrap_or_default(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().expect("strategy mutex")
    }

    /// Restore: every bot comes back Stopped, except one holding an open
    /// cycle, which keeps being managed but is NOT re-armed for new cycles
    /// until the user presses Start (mirrors `restore_configs`, live=false).
    pub fn load(
        &self,
        bots: Vec<StrategyBot>,
        cycles: Vec<CycleState>,
        venues: Vec<(BotId, PaperVenue)>,
        cursors: Vec<((MarketKind, String), Feed)>,
    ) {
        let mut g = self.lock();
        for mut b in bots {
            if b.state != BotRunState::Dead {
                b.state = BotRunState::Stopped;
            }
            g.bots.insert(b.id.clone(), b);
        }
        for c in cycles {
            if g.bots.contains_key(&c.core.bot_id) && c.is_open() {
                g.cycles.insert(c.core.bot_id.clone(), c);
            }
        }
        for (id, v) in venues {
            g.venues.insert(id, v);
        }
        for (k, f) in cursors {
            g.feeds.insert(k, f);
        }
    }

    /// Re-arms a paper bot that was running before a restart (`load` leaves
    /// every bot Stopped). A bot holding an open cycle comes back InCycle, a
    /// paused one Paused, any other Armed. Only a Stopped bot is touched.
    pub fn resume(&self, id: &str, before: BotRunState) {
        let mut g = self.lock();
        let has_cycle = g.cycles.contains_key(id);
        if let Some(b) = g.bots.get_mut(id) {
            if b.state == BotRunState::Stopped {
                b.state = if before == BotRunState::Paused {
                    BotRunState::Paused
                } else if has_cycle {
                    BotRunState::InCycle
                } else {
                    BotRunState::Armed
                };
            }
        }
    }

    pub fn set_risk(&self, risk: StrategyRisk, tripped_day: u64, peak: f64) {
        let mut g = self.lock();
        g.risk = risk;
        g.tripped_day = tripped_day;
        g.portfolio_peak = peak;
    }

    pub fn risk(&self) -> StrategyRisk {
        self.lock().risk
    }

    /// Any bot armed/running or any open cycle: the engine has work.
    pub fn any_active(&self) -> bool {
        let g = self.lock();
        !g.cycles.is_empty() || g.bots.values().any(|b| driver::is_running(b.state))
    }

    pub fn open_cycle_count(&self) -> usize {
        self.lock().cycles.len()
    }

    pub fn bot(&self, id: &str) -> Option<StrategyBot> {
        self.lock().bots.get(id).cloned()
    }

    pub fn bots(&self) -> Vec<StrategyBot> {
        let mut v: Vec<StrategyBot> = self.lock().bots.values().cloned().collect();
        v.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        v
    }

    /// Resting paper orders of a bot (its venue book).
    pub fn open_orders(&self, id: &str) -> Vec<model::SimOrder> {
        use venue::OrderVenue;
        self.lock().venues.get(id).map(|v| v.open_orders()).unwrap_or_default()
    }

    pub fn has_open_cycle(&self, id: &str) -> bool {
        self.lock().cycles.contains_key(id)
    }

    /// Records a forced close that could not be priced (retried each tick).
    pub fn mark_pending_close(&self, id: &str, reason: ExitReason) {
        self.lock().pending_close.insert(id.to_string(), reason);
    }

    /// True when a close was owed (the stored list must be rewritten).
    pub fn clear_pending_close(&self, id: &str) -> bool {
        self.lock().pending_close.remove(id).is_some()
    }

    /// Forced closes still owed, oldest bot first.
    pub fn pending_closes(&self) -> Vec<(BotId, ExitReason)> {
        let mut v: Vec<(BotId, ExitReason)> =
            self.lock().pending_close.iter().map(|(k, r)| (k.clone(), *r)).collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    /// Close time of the last bar the bot's open cycle walked.
    pub fn cycle_last_close_ms(&self, id: &str) -> Option<u64> {
        self.lock().cycles.get(id).map(|c| c.core.last_bar_close_ms)
    }

    pub fn put_bot(&self, bot: StrategyBot) {
        self.lock().bots.insert(bot.id.clone(), bot);
    }

    pub fn remove_bot(&self, id: &str) {
        let mut g = self.lock();
        g.bots.remove(id);
        g.venues.remove(id);
        g.paused.remove(id);
        g.pending_close.remove(id);
    }

    /// Takes a bot's state out for a bar walk / close (put back with `put_state`).
    pub fn take_state(&self, id: &str) -> Option<(StrategyBot, Option<CycleState>, PaperVenue)> {
        let mut g = self.lock();
        let bot = g.bots.get(id)?.clone();
        let cyc = g.cycles.remove(id);
        let venue = g.venues.remove(id).unwrap_or_default();
        Some((bot, cyc, venue))
    }

    pub fn put_state(&self, bot: StrategyBot, cycle: Option<CycleState>, venue: PaperVenue) {
        let mut g = self.lock();
        let id = bot.id.clone();
        match cycle {
            Some(c) => {
                g.cycles.insert(id.clone(), c);
            }
            None => {
                g.cycles.remove(&id);
            }
        }
        g.venues.insert(id.clone(), venue);
        // An archived bot leaves memory; its rows stay in the store.
        if bot.archived_at.is_some() {
            g.bots.remove(&id);
        } else {
            g.bots.insert(id, bot);
        }
    }

    pub fn feed(&self, market: MarketKind, symbol: &str) -> Option<Feed> {
        self.lock().feeds.get(&(market, symbol.to_string())).copied()
    }

    pub fn set_feed(&self, market: MarketKind, symbol: &str, feed: Feed) {
        self.lock().feeds.insert((market, symbol.to_string()), feed);
    }

    pub fn set_paused(&self, id: &str, reason: Option<&'static str>) {
        let mut g = self.lock();
        match reason {
            Some(r) => {
                g.paused.insert(id.to_string(), r);
            }
            None => {
                g.paused.remove(id);
            }
        }
    }

    /// Pushes a note; a bot's identical consecutive note is shown once.
    pub fn note(&self, bot_id: &str, symbol: &str, key: &str, detail: Option<String>, now: u64) {
        let mut g = self.lock();
        if !bot_id.is_empty() {
            if g.last_note.get(bot_id).map(String::as_str) == Some(key) {
                return;
            }
            g.last_note.insert(bot_id.to_string(), key.to_string());
        }
        if g.notes.len() == NOTE_CAP {
            g.notes.pop_front();
        }
        g.notes.push_back(StrategyNote {
            at_ms: now,
            bot_id: bot_id.to_string(),
            symbol: symbol.to_string(),
            key: key.to_string(),
            detail,
        });
    }

    /// Clears the once-filter so the same reason can be noted again after
    /// something else happened (a cycle opened).
    pub fn clear_last_note(&self, bot_id: &str) {
        self.lock().last_note.remove(bot_id);
    }

    /// Newest first.
    pub fn notes(&self) -> Vec<StrategyNote> {
        self.lock().notes.iter().rev().cloned().collect()
    }

    fn mark_of(g: &Inner, b: &StrategyBot) -> Option<Feed> {
        g.feeds.get(&(b.cfg.market, b.cfg.symbol.clone())).copied().filter(|f| f.last_close > 0.0)
    }

    fn equity_of(g: &Inner, b: &StrategyBot) -> f64 {
        let mtm = match (g.cycles.get(&b.id), Self::mark_of(g, b)) {
            (Some(c), Some(f)) => c.mtm(f.last_close),
            (Some(c), None) => c.core.cash,
            _ => 0.0,
        };
        accounting::equity(b.cfg.budget, b.realized_quote, mtm)
    }

    /// P&L (closed + open MTM) of the bots inside the portfolio breaker.
    pub fn breaker_pnl(&self) -> f64 {
        let g = self.lock();
        g.bots
            .values()
            .filter(|b| b.cfg.portfolio_breaker)
            .map(|b| Self::equity_of(&g, b) - b.cfg.budget)
            .sum()
    }

    /// Sum of the budgets of the bots inside the portfolio breaker; 0 when
    /// none is, and a 0 budget never trips.
    pub fn breaker_budget(&self) -> f64 {
        self.lock()
            .bots
            .values()
            .filter(|b| b.cfg.portfolio_breaker)
            .map(|b| b.cfg.budget)
            .sum()
    }

    /// (bots inside the breaker, bots in memory).
    pub fn breaker_coverage(&self) -> (u32, u32) {
        let g = self.lock();
        let covered = g.bots.values().filter(|b| b.cfg.portfolio_breaker).count();
        (covered as u32, g.bots.len() as u32)
    }

    /// One bot's P&L (closed + open MTM), quote; 0 for an unknown id.
    pub fn bot_pnl(&self, id: &str) -> f64 {
        let g = self.lock();
        g.bots.get(id).map(|b| Self::equity_of(&g, b) - b.cfg.budget).unwrap_or(0.0)
    }

    /// Does the tripped breaker hold this bot (no start, no new cycles)?
    pub fn breaker_holds(&self, bot: &StrategyBot) -> bool {
        bot.cfg.portfolio_breaker && self.portfolio_tripped()
    }

    /// Budget reserved by running bots or open cycles.
    pub fn reserved_budget(&self, except: Option<&str>) -> f64 {
        let g = self.lock();
        g.bots
            .values()
            .filter(|b| Some(b.id.as_str()) != except)
            .filter(|b| driver::is_running(b.state) || g.cycles.contains_key(&b.id))
            .map(|b| b.cfg.budget)
            .sum()
    }

    /// Today's strategy P&L (realised AND unrealised, paper scope):
    /// sum(equity now - equity at the day's first tick). A bot created today
    /// starts at its budget. 0 until today's snapshot exists (never
    /// yesterday's base).
    pub fn day_pnl_quote(&self, today: u64) -> f64 {
        let g = self.lock();
        if g.day_snapshot.0 != today {
            return 0.0;
        }
        g.bots
            .values()
            .map(|b| {
                let start = g.day_snapshot.1.get(&b.id).copied().unwrap_or(b.cfg.budget);
                Self::equity_of(&g, b) - start
            })
            .sum()
    }

    /// Installs today's snapshot when the UTC day changed. Returns the new
    /// snapshot when one was taken (to persist).
    pub fn roll_day(&self, day: u64, stored: Option<HashMap<BotId, f64>>) -> Option<HashMap<BotId, f64>> {
        let mut g = self.lock();
        if g.day_snapshot.0 == day {
            return None;
        }
        if let Some(s) = stored {
            g.day_snapshot = (day, s);
            return None;
        }
        let snap: HashMap<BotId, f64> = g.bots.values().map(|b| (b.id.clone(), Self::equity_of(&g, b))).collect();
        g.day_snapshot = (day, snap.clone());
        Some(snap)
    }

    pub fn day_snapshot_day(&self) -> u64 {
        self.lock().day_snapshot.0
    }

    /// A trip holds until the user re-arms it (on a later UTC day).
    pub fn portfolio_tripped(&self) -> bool {
        self.lock().tripped_day != 0
    }

    pub fn tripped_day(&self) -> u64 {
        self.lock().tripped_day
    }

    pub fn portfolio_peak(&self) -> f64 {
        self.lock().portfolio_peak
    }

    /// Portfolio breaker on P&L: tracks the P&L peak; true when P&L fell
    /// `portfolio_dd_pct` of the total budget below it. Tracking P&L, not
    /// equity, keeps a new bot's budget from moving the peak.
    pub fn breaker_check(&self, pnl: f64, budget: f64) -> bool {
        let mut g = self.lock();
        if pnl > g.portfolio_peak {
            g.portfolio_peak = pnl;
            return false;
        }
        budget > 0.0 && (g.portfolio_peak - pnl) >= g.risk.portfolio_dd_pct / 100.0 * budget
    }

    /// A bot's P&L leaves (archive, breaker switched off) or joins (switched
    /// on) the breaker's P&L; the peak moves with it so the drawdown holds.
    pub fn shift_peak(&self, delta: f64) {
        self.lock().portfolio_peak += delta;
    }

    pub fn trip_breaker(&self, day: u64) {
        self.lock().tripped_day = day;
    }

    /// Re-arm after a trip: allowed only on a later UTC day; the peak
    /// restarts at the current P&L of the bots inside the breaker.
    pub fn rearm_breaker(&self, today: u64) -> Result<(), &'static str> {
        let eq = self.breaker_pnl();
        let mut g = self.lock();
        if g.tripped_day == today {
            return Err("portfolioDdTripped");
        }
        g.tripped_day = 0;
        g.portfolio_peak = eq;
        Ok(())
    }

    pub fn set_risk_values(&self, risk: StrategyRisk) {
        self.lock().risk = risk;
    }

    pub fn compact_due(&self, now: u64) -> bool {
        let mut g = self.lock();
        if now.saturating_sub(g.last_compact_ms) < 3_600_000 {
            return false;
        }
        g.last_compact_ms = now;
        true
    }

    /// Stops every bot (no new cycles); open cycles keep being managed.
    pub fn stop_all(&self) -> Vec<StrategyBot> {
        self.stop_where(false)
    }

    /// Stops the bots inside the portfolio breaker only.
    pub fn stop_breaker_bots(&self) -> Vec<StrategyBot> {
        self.stop_where(true)
    }

    /// Ids of the bots a breaker trip closes, oldest first.
    pub fn breaker_bot_ids(&self) -> Vec<BotId> {
        self.bots().into_iter().filter(|b| b.cfg.portfolio_breaker).map(|b| b.id).collect()
    }

    fn stop_where(&self, breaker_only: bool) -> Vec<StrategyBot> {
        let mut g = self.lock();
        let mut changed = Vec::new();
        for b in g.bots.values_mut() {
            if breaker_only && !b.cfg.portfolio_breaker {
                continue;
            }
            if driver::is_running(b.state) {
                b.state = BotRunState::Stopped;
                changed.push(b.clone());
            }
        }
        changed
    }

    /// What the real-money mirror (bot/strategy_live.rs) reads from a bot's
    /// open cycle: (cycle seq, signed position qty, protective stop price).
    pub fn live_target(&self, id: &str) -> Option<(u32, f64, Option<f64>)> {
        let g = self.lock();
        g.cycles
            .get(id)
            .filter(|c| c.core.open)
            .map(|c| (c.core.seq, c.signed_qty(), c.protective_stop()))
    }

    pub fn views(&self) -> Vec<StrategyBotView> {
        let ids: Vec<BotId> = self.bots().into_iter().map(|b| b.id).collect();
        ids.iter().filter_map(|id| self.view(id)).collect()
    }

    pub fn view(&self, id: &str) -> Option<StrategyBotView> {
        let g = self.lock();
        let b = g.bots.get(id)?;
        let mark = Self::mark_of(&g, b);
        let cyc = g.cycles.get(id);
        let equity = Self::equity_of(&g, b);
        let budget = b.cfg.budget;
        let mtm_quote = equity - budget - b.realized_quote;
        let open_cycle = cyc.map(|c| {
            let px = mark.map(|f| f.last_close);
            let out_of_range = match (&c.engine, px) {
                (cycle::Engine::Grid(gs), Some(p)) => gs.out_of_range(p),
                _ => false,
            };
            OpenCycleView {
                seq: c.core.seq,
                opened_at: c.core.opened_at,
                anchor_price: c.core.anchor_price,
                avg_entry: c.avg_entry(),
                signed_qty: c.signed_qty(),
                notional: c.notional(),
                margin: c.margin(),
                reserved: c.committed(),
                cash_quote: c.core.cash,
                unrealized_quote: px.map(|p| c.unreal(p)).unwrap_or(0.0),
                fees_quote: c.core.fees,
                funding_quote: c.core.funding,
                so_filled: c.so_filled(),
                grid_closing_fills: c.grid_closing_fills(),
                liq_price: c.liq_price(),
                max_adverse_pct: c.core.max_adverse_quote / budget * 100.0,
                out_of_range,
                funding_unknown: c.core.funding_unknown,
            }
        });
        let paused = g.paused.get(id).copied();
        let pause_reason = paused.or_else(|| open_cycle.as_ref().filter(|c| c.out_of_range).map(|_| "outOfRange"));
        let run_state = if b.state == BotRunState::Dead {
            BotRunState::Dead
        } else if pause_reason.is_some() && (driver::is_running(b.state) || cyc.is_some()) {
            BotRunState::Paused
        } else if cyc.is_some() {
            BotRunState::InCycle
        } else if driver::is_running(b.state) {
            BotRunState::Armed
        } else {
            BotRunState::Stopped
        };
        let (u_pos, u_commit) = accounting::utilisation(&b.util, budget);
        Some(StrategyBotView {
            id: b.id.clone(),
            kind: b.cfg.kind(),
            name: b.cfg.name.clone(),
            exchange_id: b.cfg.exchange_id.clone(),
            symbol: b.cfg.symbol.clone(),
            side: b.cfg.side,
            market: b.cfg.market,
            leverage: b.cfg.leverage,
            paper: !limits::STRATEGY_LIVE_ALLOWED,
            run_state: run_state.as_str(),
            accepting_new_cycles: driver::is_running(b.state),
            pause_reason,
            budget,
            realized_quote: b.realized_quote,
            realized_pct: b.realized_quote / budget * 100.0,
            mtm_pct: mtm_quote / budget * 100.0,
            equity,
            drawdown_pct: accounting::drawdown_pct(b.peak_equity.max(equity), equity, budget),
            max_drawdown_pct: b.max_dd_quote / budget * 100.0,
            cycles_done: b.cycles_done,
            dead_reason: b.dead_reason,
            created_at: b.created_at,
            preset_id: b.cfg.preset_id.clone(),
            mark_price: mark.map(|f| f.last_close),
            mark_ts: mark.map(|f| f.last_close_ms),
            open_cycle,
            last_note: g.notes.iter().rev().find(|n| n.bot_id == b.id).cloned(),
            utilisation_in_position: u_pos,
            utilisation_committed: u_commit,
        })
    }
}

impl Default for StrategyManager {
    fn default() -> Self {
        Self::new()
    }
}

/// "sb_" + 12 lowercase base32 characters.
pub fn new_bot_id() -> BotId {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let bytes = uuid::Uuid::new_v4().into_bytes();
    let id: String = bytes
        .iter()
        .take(12)
        .map(|b| ALPHABET[usize::from(*b) % 32] as char)
        .collect();
    format!("sb_{id}")
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod isolation_tests;
#[cfg(test)]
mod manager_tests;
