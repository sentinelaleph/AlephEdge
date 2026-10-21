//! Open phase: every buffered signal, for each running bot, through the
//! gates (cheap, pure) → fill model (live price) → network prechecks → open.
//!
//! A signal is judged only on a FINAL outcome (see `precheck::is_final`);
//! transient skips retry every tick until expiry and are reported once.

use tauri::{AppHandle, Manager};

use crate::exchange::ExchangeManager;
use crate::market::model::DepthMarket;
use crate::market::MarketManager;
use crate::membership::MembershipManager;
use crate::risk::RiskManager;
use crate::signal::SignalManager;
use crate::signal::model::Signal;

use super::super::judged::JudgeLedger;
use super::super::model::{BotConfig, BotKind};
use super::super::BotManager;
use super::btc_break::BtcRegime;
use super::pipeline::{self, EntryFeeds};
use super::precheck::{self, Skip};
use super::{book, entry_rules, live, live_price, now_ms};

// The sequence itself is `pipeline.rs` (shared with the headless bot farm);
// this file supplies the desktop's managers as its data source.
pub use super::pipeline::Prepared;
#[cfg(test)]
pub use super::pipeline::market_mismatch;

// The pure decisions live in `entry_rules.rs` (shared with the headless paper
// runner); re-exported so every existing `entry::` path keeps working (some
// only from the tests, hence the allow).
#[allow(unused_imports)]
pub use super::entry_rules::{
    finish_position, new_position, plan_entry, pre_price_gate, price_gate, EntryMode, GateInput, PlannedEntry,
};

/// Applies a skip to the ledger. Returns true when the note should be shown:
/// always for a final skip (which also judges the signal, keeping the key so
/// a settings change can lift a settings-based refusal), only the first time
/// for a transient one.
pub fn settle_skip(ledger: &mut JudgeLedger, kind: BotKind, sig_id: &str, skip: &Skip) -> bool {
    if precheck::is_final(skip.key) {
        ledger.judge_for(kind, sig_id, skip.key)
    } else {
        ledger.note_once(kind, sig_id, skip.key)
    }
}

/// The desktop's managers as the entry pipeline's data source. The Sentinel
/// session token is read at most once per entry, at the first feed that
/// needs it (funding or depth), as the precheck always did.
struct DeskFeeds<'a> {
    signals: &'a SignalManager,
    exchange: &'a ExchangeManager,
    membership: &'a MembershipManager,
    market: &'a MarketManager,
    bots: &'a BotManager,
    token: tokio::sync::OnceCell<Option<String>>,
}

impl DeskFeeds<'_> {
    /// Both market feeds sit behind the API's session check, so no token
    /// means no data at all -- the same condition as an unreachable feed.
    async fn token(&self) -> Option<&str> {
        self.token
            .get_or_init(|| self.membership.fresh_access_token())
            .await
            .as_deref()
    }
}

impl EntryFeeds for DeskFeeds<'_> {
    fn is_invalidated(&self, signal_id: &str) -> bool {
        self.signals.is_invalidated(signal_id)
    }
    fn is_regime_vetoed(&self, signal_id: &str) -> bool {
        self.signals.is_regime_vetoed(signal_id)
    }
    fn open_status(&self, sig: &Signal, now_ms: u64) -> crate::signal::model::OpenStatus {
        self.signals.open_status(sig, now_ms)
    }
    fn futures_supported(&self, exchange_id: &str) -> bool {
        self.exchange.supports_futures(exchange_id)
    }
    fn membership_active(&self) -> bool {
        self.membership.view().active
    }
    fn open_count_for(&self, kind: BotKind) -> usize {
        self.bots.open_count_for(kind)
    }
    async fn live_price(&self, exchange_id: &str, symbol: &str, kind: BotKind) -> Option<f64> {
        live_price(self.exchange, exchange_id, symbol, kind).await
    }
    async fn funding(&self, symbol: &str) -> Result<Option<f64>, ()> {
        let base = self.membership.base_url();
        match self.token().await {
            Some(token) => self
                .market
                .funding_for(&base, token, symbol)
                .await
                .map(|rate| rate.map(|r| r.rate))
                .map_err(|_| ()),
            None => Err(()),
        }
    }
    async fn depth(&self, symbol: &str, market: DepthMarket) -> Result<(f64, f64), ()> {
        let base = self.membership.base_url();
        match self.token().await {
            Some(token) => self
                .market
                .depth(&base, token, symbol, market)
                .await
                .map(|d| (d.bid_depth_usd, d.ask_depth_usd))
                .map_err(|_| ()),
            None => Err(()),
        }
    }
}

/// Everything between a buffered signal and its opening, shared by the
/// bot's loop (`EntryMode::Auto`) and the user's manual entry
/// (`EntryMode::Manual`, see manual.rs): market fit → pre-price gate → live
/// price → fill model, take-profit, sizing, liquidation gate → exchange
/// minimum → the §5.4 prechecks (membership, validity, Sentinel status,
/// funding, depth, risk budget). Ok(None) = the fill model waits (Auto
/// only: a manual entry fills at the live price inside the stop–target band).
/// `holds_market` is the caller's held-market rule.
pub async fn prepare_entry(
    app: &AppHandle,
    cfg: &BotConfig,
    sig: &Signal,
    regime: BtcRegime,
    mode: EntryMode,
    holds_market: bool,
    now: u64,
) -> Result<Option<Prepared>, Skip> {
    let risk = app.state::<RiskManager>();
    let (limits, balance) = (risk.limits(), risk.balance());
    let feeds = DeskFeeds {
        signals: &app.state::<SignalManager>(),
        exchange: &app.state::<ExchangeManager>(),
        membership: &app.state::<MembershipManager>(),
        market: &app.state::<MarketManager>(),
        bots: &app.state::<BotManager>(),
        token: tokio::sync::OnceCell::new(),
    };
    pipeline::prepare_entry_with(&feeds, cfg, sig, regime, mode, holds_market, &limits, balance, now).await
}

/// The risk level's position limit applies per signal bot: Spot, Futures and
/// Pump are separate categories, so Spot holding 5
/// must not stop Futures. judge_budget's "global" ceiling therefore counts
/// this bot only here (the bot farm runs each bot as its own account).
#[cfg(test)]
pub(crate) fn signal_bot_budget(bots: &BotManager, kind: BotKind, balance: f64, notional: f64) -> precheck::Budget {
    pipeline::signal_bot_budget(bots.open_count_for(kind), balance, notional)
}

pub async fn open_phase(app: &AppHandle, regime: BtcRegime) {
    let bots = app.state::<BotManager>();
    if bots.kill_switch_tripped() {
        return;
    }
    let signals = app.state::<SignalManager>();
    let limits = app.state::<RiskManager>().limits();

    // Newest first: when slots are scarce the
    // freshest setup claims them; a regenerated signal replaces its stale
    // predecessor rather than queueing behind it. The age gate
    // (`max_signal_age_min`) drops anything older than the bot allows.
    for sig in signals.recent() {
        for kind in [BotKind::Futures, BotKind::Spot, BotKind::Pump] {
            let Some(cfg) = bots.running_config(kind) else {
                continue;
            };
            if bots.is_judged(kind, &sig.id) {
                continue;
            }
            let now = now_ms();
            let holds = bots.holds_market(kind, &sig.symbol, &sig.timeframe);
            let Prepared { plan, ctx } = match prepare_entry(app, &cfg, &sig, regime, EntryMode::Auto, holds, now).await {
                Ok(Some(prepared)) => prepared,
                Ok(None) => continue,
                Err(skip) => {
                    bots.settle_skip(app, kind, &sig, skip);
                    continue;
                }
            };
            // LIVE path (gated, see live.rs), both directions: statically
            // unreachable while LIVE_TRADING_ENABLED is false.
            let is_live = live::live_trading_allowed(&cfg, kind);
            let live_open = if is_live {
                let cap = precheck::effective_leverage(&cfg, &limits);
                match live::open_live(
                    app,
                    &cfg,
                    &sig,
                    plan.price,
                    plan.size.notional,
                    cap,
                    plan.target,
                )
                .await
                {
                    Ok(open) => {
                        bots.pilot_used_saved(app, kind);
                        Some(open)
                    }
                    Err(key) => {
                        bots.settle_skip(app, kind, &sig, Skip::new(key));
                        continue;
                    }
                }
            } else {
                None
            };
            let entry = live_open.as_ref().map_or(plan.price, |o| o.entry);
            // Live: re-resolved at the real fill (a custom % moves with it).
            let tp = live_open.as_ref().map_or(plan.tp.clone(), |o| o.tp.clone());
            let mut pos = entry_rules::finish_position(&cfg, &sig, entry, &tp, &plan, now, &ctx);
            if let Some(open) = live_open {
                pos.live = true;
                pos.qty = open.qty;
                pos.entry_order_id = Some(open.entry_order_id);
                pos.unprotected = open.stop_algo_id.is_none();
                pos.stop_algo_id = open.stop_algo_id;
                pos.tp_algo_id = open.tp_algo_id;
            }
            let (id, live_now) = (pos.signal_id.clone(), pos.live);
            book::open_position(app, pos);
            bots.judge(kind, &sig.id);
            // A remote kill or the daily stop can land while the entry was
            // on the wire; its close-all ran on a book without this position.
            if live_now && (bots.running_config(kind).is_none() || bots.kill_switch_tripped()) {
                book::close_now(app, &id, kind, "stopped").await;
            }
        }
    }
}
