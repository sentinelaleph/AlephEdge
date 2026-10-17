//! The entry pipeline, step for step, over an abstract data source: market
//! fit → pre-price gate → live price → fill model, take-profit, sizing,
//! liquidation gate → exchange minimum → the §5.4 prechecks (membership,
//! validity, Sentinel status, funding, depth, risk budget).
//!
//! Tauri-free and I/O-free itself: every read goes through `EntryFeeds`. The
//! desktop implements it over its managers (`entry::prepare_entry`,
//! `precheck::run`); the headless bot farm (`crates/aleph-bot-farm`)
//! implements it over Sentinel's DataHub. Both therefore run this exact
//! sequence and these exact rules.

use crate::risk::model::RiskLimits;
use crate::signal::model::{OpenStatus, Signal};

use super::super::model::{BotConfig, BotKind, LIVE_TRADING_ENABLED};
use super::btc_break::BtcRegime;
use super::entry_rules::{self, EntryMode, GateInput, PlannedEntry};
use super::precheck::{self, Budget, PrecheckContext, Skip};

/// What the entry pipeline reads. Errors are `()`: the pipeline only needs
/// to know a feed is unavailable, the skip reason it then gives is its own.
#[allow(async_fn_in_trait)]
pub trait EntryFeeds {
    /// Legacy server-side invalidation (blocks entry, never closes).
    fn is_invalidated(&self, signal_id: &str) -> bool;
    /// BTC-regime veto (blocks entry; the open position is closed elsewhere).
    fn is_regime_vetoed(&self, signal_id: &str) -> bool;
    /// Whether Sentinel still lists the signal as open (see `signal::resolved`).
    fn open_status(&self, sig: &Signal, now_ms: u64) -> OpenStatus;
    fn futures_supported(&self, exchange_id: &str) -> bool;
    fn membership_active(&self) -> bool;
    /// Open positions of the signal bot of `kind` (its own position limit).
    fn open_count_for(&self, kind: BotKind) -> usize;
    /// Live price on the bot's market: futures for futures-market bots, spot
    /// otherwise, no cross-market fallback.
    async fn live_price(&self, exchange_id: &str, symbol: &str, kind: BotKind) -> Option<f64>;
    /// Funding, % per interval. Ok(None) = feed healthy, symbol not covered.
    async fn funding(&self, symbol: &str) -> Result<Option<f64>, ()>;
    /// ±1% book depth (bid USD, ask USD).
    async fn depth(&self, symbol: &str) -> Result<(f64, f64), ()>;
}

/// A signal ready to open: the plan at the live price and the precheck's
/// captured context.
pub struct Prepared {
    pub plan: PlannedEntry,
    pub ctx: PrecheckContext,
}

/// The signal was published for the other Binance market: futures bots
/// (Futures, Pump) take futures signals, the Spot bot spot ones. A signal
/// without a market (older payloads) passes.
pub fn market_mismatch(kind: BotKind, sig: &Signal) -> Option<Skip> {
    let market = sig.market_type.as_deref()?.trim().to_ascii_lowercase();
    let wanted = if kind.uses_futures_market() { "futures" } else { "spot" };
    (market != wanted && (market == "spot" || market == "futures")).then(|| Skip::with("signalOtherMarket", market))
}

/// Everything between a buffered signal and its opening. Ok(None) = the fill
/// model waits (Auto only: a manual entry fills at the live price inside the
/// stop–target band). `holds_market` is the caller's held-market rule;
/// `limits` and `balance` are the risk level's limits and the simulated
/// balance at decision time.
#[allow(clippy::too_many_arguments)]
pub async fn prepare_entry_with<F: EntryFeeds>(
    feeds: &F,
    cfg: &BotConfig,
    sig: &Signal,
    regime: BtcRegime,
    mode: EntryMode,
    holds_market: bool,
    limits: &RiskLimits,
    balance: f64,
    now: u64,
) -> Result<Option<Prepared>, Skip> {
    // A bot takes only signals published for its own market. A spot signal's
    // levels and its graded outcome come from spot candles; traded on a perp it
    // is a different instrument.
    if let Some(skip) = market_mismatch(cfg.kind, sig) {
        return Err(skip);
    }
    let input = GateInput {
        cfg,
        sig,
        regime,
        now_ms: now,
        invalidated: feeds.is_invalidated(&sig.id),
        regime_vetoed: feeds.is_regime_vetoed(&sig.id),
        // Pump is not unlocked by the global risk level any more
        // (risk/model.rs); it trades paper only, labelled untested.
        pump_allowed: true,
        futures_supported: feeds.futures_supported(&cfg.exchange_id),
        holds_market,
    };
    if let Some(skip) = entry_rules::pre_price_gate_in(&input, mode) {
        return Err(skip);
    }
    let price = feeds.live_price(&cfg.exchange_id, &sig.symbol, cfg.kind).await;
    // Fill model → the user's take-profit target resolved at this
    // fill → size (before the prechecks: the depth check needs the
    // real notional, and paper and live use this one size).
    let leverage = precheck::effective_leverage(cfg, limits);
    let Some(plan) = entry_rules::plan_entry_in(cfg, sig, price, now, leverage, mode)? else {
        return Ok(None);
    };
    entry_rules::exchange_minimum_gate(&plan)?;
    let budget = signal_bot_budget(feeds.open_count_for(cfg.kind), balance, plan.size.notional);
    let ctx = precheck_with(feeds, limits, cfg, sig, &budget, now).await?;
    Ok(Some(Prepared { plan, ctx }))
}

/// The risk level's position limit applies per signal bot: Spot, Futures and
/// Pump are separate categories, so Spot holding 5 must not stop Futures.
/// judge_budget's "global" ceiling therefore counts this bot only.
pub fn signal_bot_budget(open: usize, balance: f64, notional: f64) -> Budget {
    Budget {
        global_open: open,
        bot_open: open,
        balance,
        notional,
    }
}

/// The precheck pipeline. Returns the captured FR/LD context on pass.
pub async fn precheck_with<F: EntryFeeds>(
    feeds: &F,
    limits: &RiskLimits,
    cfg: &BotConfig,
    sig: &Signal,
    budget: &Budget,
    now_ms: u64,
) -> Result<PrecheckContext, Skip> {
    // 1. Membership valid.
    if !feeds.membership_active() {
        return Err(Skip::new("membershipInactive"));
    }

    // 2. Signal still valid (see `signal_still_valid`).
    precheck::signal_still_valid(
        feeds.is_invalidated(&sig.id),
        feeds.is_regime_vetoed(&sig.id),
        &sig.expires_at,
        now_ms,
    )?;

    // Missing data (funding, depth, signal status): the level decides on
    // paper; a live bot is always strict.
    let strict = precheck::strict_data(limits) || (LIVE_TRADING_ENABLED && cfg.live);

    // 2b. Sentinel has not already resolved it (see `judge_still_open`).
    // A "New trade" (engine/user_trade.rs) was never published: Sentinel
    // has no status for it, and none is asked.
    if !entry_rules::is_user_trade_id(&sig.id) {
        precheck::judge_still_open(feeds.open_status(sig, now_ms), strict)?;
    }

    // 3. Funding rate (futures-market bots only).
    let mut fr_at_open = None;
    if cfg.kind.uses_futures_market() {
        let fetched = feeds.funding(&sig.symbol).await;
        fr_at_open = precheck::judge_funding_with(limits, strict, sig.direction, fetched)?;
    }

    // 4. Liquidity depth.
    let fetched = feeds.depth(&sig.symbol).await;
    let ld_at_open = precheck::judge_depth_with(limits, strict, budget.notional, fetched)?;

    // 5. Risk budget.
    precheck::judge_budget(limits, cfg, budget)?;

    // No R:R floor here: live TP1 sits at ≈0.6R by design (see geometry.rs);
    // coherence and fill drift are checked before this pipeline runs.

    Ok(PrecheckContext {
        fr_at_open,
        ld_at_open,
    })
}
