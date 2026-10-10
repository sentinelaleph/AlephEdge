//! Open phase: every buffered signal, for each running bot, through the
//! gates (cheap, pure) → fill model (live price) → network prechecks → open.
//!
//! A signal is judged only on a FINAL outcome (see `precheck::is_final`);
//! transient skips retry every tick until expiry and are reported once.

use tauri::{AppHandle, Manager};

use crate::exchange::ExchangeManager;
use crate::market::MarketManager;
use crate::membership::MembershipManager;
use crate::risk::RiskManager;
use crate::signal::SignalManager;
use crate::signal::model::Signal;

use super::super::judged::JudgeLedger;
use super::super::model::{BotConfig, BotKind};
use super::super::BotManager;
use super::btc_break::BtcRegime;
use super::precheck::{self, PrecheckContext, Skip};
use super::{book, entry_rules, live, live_price, now_ms};

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

/// The signal was published for the other Binance market: futures bots
/// (Futures, Pump) take futures signals, the Spot bot spot ones. A signal
/// without a market (older payloads) passes.
pub fn market_mismatch(kind: BotKind, sig: &Signal) -> Option<Skip> {
    let market = sig.market_type.as_deref()?.trim().to_ascii_lowercase();
    let wanted = if kind.uses_futures_market() { "futures" } else { "spot" };
    (market != wanted && (market == "spot" || market == "futures")).then(|| Skip::with("signalOtherMarket", market))
}

/// A signal ready to open: the plan at the live price and the precheck's
/// captured context.
pub struct Prepared {
    pub plan: PlannedEntry,
    pub ctx: PrecheckContext,
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
    // Owner decision 2026-10-05: a bot takes only signals published for
    // its own market. A spot signal's levels and its graded outcome come
    // from spot candles; traded on a perp it is a different instrument.
    if let Some(skip) = market_mismatch(cfg.kind, sig) {
        return Err(skip);
    }
    let signals = app.state::<SignalManager>();
    let exchange = app.state::<ExchangeManager>();
    let risk = app.state::<RiskManager>();
    let (limits, balance) = (risk.limits(), risk.balance());
    let bots = app.state::<BotManager>();
    let input = GateInput {
        cfg,
        sig,
        regime,
        now_ms: now,
        invalidated: signals.is_invalidated(&sig.id),
        regime_vetoed: signals.is_regime_vetoed(&sig.id),
        // Pump is not unlocked by the global risk level any more
        // (risk/model.rs); it trades paper only, labelled untested.
        pump_allowed: true,
        futures_supported: exchange.supports_futures(&cfg.exchange_id),
        holds_market,
    };
    if let Some(skip) = entry_rules::pre_price_gate_in(&input, mode) {
        return Err(skip);
    }
    let price = live_price(&exchange, &cfg.exchange_id, &sig.symbol, cfg.kind).await;
    // Fill model → the user's take-profit target resolved at this
    // fill → size (before the prechecks: the depth check needs the
    // real notional, and paper and live use this one size).
    let leverage = precheck::effective_leverage(cfg, &limits);
    let Some(plan) = entry_rules::plan_entry_in(cfg, sig, price, now, leverage, mode)? else {
        return Ok(None);
    };
    entry_rules::exchange_minimum_gate(&plan)?;
    let budget = signal_bot_budget(&bots, cfg.kind, balance, plan.size.notional);
    let membership = app.state::<MembershipManager>();
    let market = app.state::<MarketManager>();
    let ctx = precheck::run(&membership, &signals, &market, &limits, cfg, sig, &budget, now).await?;
    Ok(Some(Prepared { plan, ctx }))
}

/// The risk level's position limit applies per signal bot (owner, 10 Oct
/// 2026): Spot, Futures and Pump are separate categories, so Spot holding 5
/// must not stop Futures. judge_budget's "global" ceiling therefore counts
/// this bot only here; the paper runner (one bot per arm) is unchanged.
pub(crate) fn signal_bot_budget(bots: &BotManager, kind: BotKind, balance: f64, notional: f64) -> precheck::Budget {
    let open = bots.open_count_for(kind);
    precheck::Budget { global_open: open, bot_open: open, balance, notional }
}

pub async fn open_phase(app: &AppHandle, regime: BtcRegime) {
    let bots = app.state::<BotManager>();
    if bots.kill_switch_tripped() {
        return;
    }
    let signals = app.state::<SignalManager>();
    let limits = app.state::<RiskManager>().limits();

    // Newest first (owner rule, 2026-09-23): when slots are scarce the
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
                        bots.pilot_used(kind);
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
