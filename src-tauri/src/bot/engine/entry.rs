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

use super::super::judged::JudgeLedger;
use super::super::model::BotKind;
use super::super::BotManager;
use super::btc_break::BtcRegime;
use super::precheck::{self, Skip};
use super::{book, entry_rules, live, live_price, now_ms};

// The pure decisions live in `entry_rules.rs` (shared with the headless paper
// runner); re-exported so every existing `entry::` path keeps working (some
// only from the tests, hence the allow).
#[allow(unused_imports)]
pub use super::entry_rules::{
    finish_position, new_position, plan_entry, pre_price_gate, price_gate, GateInput, PlannedEntry,
};

/// Applies a skip to the ledger. Returns true when the note should be shown:
/// always for a final skip (which also judges the signal), only the first
/// time for a transient one.
pub fn settle_skip(ledger: &mut JudgeLedger, kind: BotKind, sig_id: &str, skip: &Skip) -> bool {
    if precheck::is_final(skip.key) {
        ledger.judge(kind, sig_id)
    } else {
        ledger.note_once(kind, sig_id, skip.key)
    }
}

pub async fn open_phase(app: &AppHandle, regime: BtcRegime) {
    let bots = app.state::<BotManager>();
    if bots.kill_switch_tripped() {
        return;
    }
    let signals = app.state::<SignalManager>();
    let exchange = app.state::<ExchangeManager>();
    let risk = app.state::<RiskManager>();
    let (limits, balance) = (risk.limits(), risk.balance());

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
            let input = GateInput {
                cfg: &cfg,
                sig: &sig,
                regime,
                now_ms: now,
                invalidated: signals.is_invalidated(&sig.id),
                regime_vetoed: signals.is_regime_vetoed(&sig.id),
                pump_allowed: risk.allows_pump(),
                futures_supported: exchange.supports_futures(&cfg.exchange_id),
                holds_market: bots.holds_market(kind, &sig.symbol, &sig.timeframe),
            };
            if let Some(skip) = pre_price_gate(&input) {
                bots.settle_skip(kind, &sig, skip);
                continue;
            }
            let price = live_price(&exchange, &cfg.exchange_id, &sig.symbol, kind).await;
            // Fill model → the user's take-profit target resolved at this
            // fill → size (before the prechecks: the depth check needs the
            // real notional, and paper and live use this one size).
            let leverage = precheck::effective_leverage(&cfg, &limits);
            let plan = match entry_rules::plan_entry(&cfg, &sig, price, now, leverage) {
                Ok(Some(plan)) => plan,
                Ok(None) => continue,
                Err(skip) => {
                    bots.settle_skip(kind, &sig, skip);
                    continue;
                }
            };
            let budget = precheck::Budget {
                global_open: bots.open_count(),
                bot_open: bots.open_count_for(kind),
                balance,
                notional: plan.size.notional,
            };
            let membership = app.state::<MembershipManager>();
            let market = app.state::<MarketManager>();
            let ctx = match precheck::run(
                &membership,
                &signals,
                &market,
                &limits,
                &cfg,
                &sig,
                &budget,
                now,
            )
            .await
            {
                Ok(ctx) => ctx,
                Err(skip) => {
                    bots.settle_skip(kind, &sig, skip);
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
                    Ok(open) => Some(open),
                    Err(key) => {
                        bots.settle_skip(kind, &sig, Skip::new(key));
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
