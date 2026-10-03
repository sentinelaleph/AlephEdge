//! Pre-trade checks — the PRD §5.4 pipeline, in order, every check with a
//! visible reason when it fails. No silent skips, no "assume fine" paths.
//!
//! This file fetches (membership, Sentinel's funding and depth feeds); every
//! decision is made by the pure rules in `precheck_rules.rs`, which the
//! headless paper runner compiles too. Re-exported here so `precheck::Skip`,
//! `precheck::is_final` etc. keep their paths.

use crate::market::MarketManager;
use crate::membership::MembershipManager;
use crate::risk::model::RiskLimits;
use crate::signal::model::Signal;
use crate::signal::SignalManager;

use super::super::model::{BotConfig, LIVE_TRADING_ENABLED};

pub use super::precheck_rules::*;

/// The PRD §5.4 pipeline. Returns the captured FR/LD context on pass.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    membership: &MembershipManager,
    signals: &SignalManager,
    market: &MarketManager,
    limits: &RiskLimits,
    cfg: &BotConfig,
    sig: &Signal,
    budget: &Budget,
    now_ms: u64,
) -> Result<PrecheckContext, Skip> {
    // 1. Membership valid.
    if !membership.view().active {
        return Err(Skip::new("membershipInactive"));
    }

    // 2. Signal still valid (see `signal_still_valid`).
    signal_still_valid(
        signals.is_invalidated(&sig.id),
        signals.is_regime_vetoed(&sig.id),
        &sig.expires_at,
        now_ms,
    )?;

    // Missing data (funding, depth, signal status): the level decides on
    // paper; a live bot is always strict.
    let strict = strict_data(limits) || (LIVE_TRADING_ENABLED && cfg.live);

    // 2b. Sentinel has not already resolved it (see `judge_still_open`).
    judge_still_open(signals.open_status(sig, now_ms), strict)?;

    let base = membership.base_url();
    // Both market feeds sit behind the API's session check, so no token means
    // no data at all -- the same condition as an unreachable feed.
    let token = membership.fresh_access_token().await;

    // 3. Funding rate (futures-market bots only).
    let mut fr_at_open = None;
    if cfg.kind.uses_futures_market() {
        let fetched = match token.as_deref() {
            Some(token) => market
                .funding_for(&base, token, &sig.symbol)
                .await
                .map(|rate| rate.map(|r| r.rate))
                .map_err(|_| ()),
            None => Err(()),
        };
        fr_at_open = judge_funding_with(limits, strict, sig.direction, fetched)?;
    }

    // 4. Liquidity depth.
    let fetched = match token.as_deref() {
        Some(token) => market
            .depth(&base, token, &sig.symbol)
            .await
            .map(|d| (d.bid_depth_usd, d.ask_depth_usd))
            .map_err(|_| ()),
        None => Err(()),
    };
    let ld_at_open = judge_depth_with(limits, strict, budget.notional, fetched)?;

    // 5. Risk budget.
    judge_budget(limits, cfg, budget)?;

    // No R:R floor here: live TP1 sits at ≈0.6R by design (see geometry.rs);
    // coherence and fill drift are checked before this pipeline runs.

    Ok(PrecheckContext {
        fr_at_open,
        ld_at_open,
    })
}
