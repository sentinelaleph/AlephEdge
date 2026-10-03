//! The open phase's decisions, pure: the pre-price gate, the fill model, and
//! the planning steps between the live price and the network prechecks
//! (take-profit resolution, leverage, sizing), plus the position builder.
//!
//! Split out of `entry.rs` (2026-09-22) so the headless paper runner
//! (`crates/aleph-paper-runner`) compiles THIS file — the exact code the
//! desktop runs — without Tauri. Nothing here may import Tauri, a manager, or
//! do I/O; `entry.rs` keeps the orchestration and re-exports these items.

use crate::bot::model::TakeProfitTarget;
use crate::signal::model::{Direction, Signal};

use super::super::model::{BotConfig, BotKind, OpenPosition};
use super::btc_break::{self, BtcRegime};
use super::filters;
use super::geometry::{self, FillDecision};
use super::precheck::{self, PrecheckContext, Skip};
use super::sizing::{self, Size};
use super::take_profit::{self, TpResolution};

/// Everything the pre-price gate needs, gathered so the gate stays pure.
pub struct GateInput<'a> {
    pub cfg: &'a BotConfig,
    pub sig: &'a Signal,
    pub regime: BtcRegime,
    pub now_ms: u64,
    pub invalidated: bool,
    /// Vetoed by the NEW "veto" event (2026-09-18): BTC's regime turned
    /// against it. Distinct from `invalidated` — see `GateInput` callers.
    pub regime_vetoed: bool,
    pub pump_allowed: bool,
    pub futures_supported: bool,
    pub holds_market: bool,
}

/// Minutes since the signal was published; None when `created_at` is unparseable.
pub fn signal_age_min(sig: &Signal, now_ms: u64) -> Option<u64> {
    crate::signal::time::parse_rfc3339_ms(&sig.created_at).map(|c| now_ms.saturating_sub(c) / 60_000)
}

/// Static and regime checks before any price is fetched. None = pass.
pub fn pre_price_gate(i: &GateInput) -> Option<Skip> {
    let expired = crate::signal::time::parse_rfc3339_ms(&i.sig.expires_at)
        .map_or(true, |exp| exp <= i.now_ms);
    if expired {
        return Some(Skip::new("expired"));
    }
    if let Some(age_min) = signal_age_min(i.sig, i.now_ms) {
        if i.cfg.max_signal_age_min > 0 && age_min > u64::from(i.cfg.max_signal_age_min) {
            return Some(Skip::with("tooOld", format!("{age_min} min")));
        }
    }
    if i.invalidated {
        return Some(Skip::new("invalidated"));
    }
    if i.regime_vetoed {
        return Some(Skip::new("vetoedByRegime"));
    }
    if i.cfg.kind == BotKind::Pump {
        if !i.pump_allowed {
            return Some(Skip::new("pumpDisabled"));
        }
        if !precheck::is_pump_signal(i.sig) {
            return Some(Skip::new("notPumpSignal"));
        }
    }
    if i.cfg.kind.uses_futures_market() && !i.futures_supported {
        return Some(Skip::new("spotOnlyExchange"));
    }
    if let Some(skip) = filters::passes_filters(i.cfg, i.sig) {
        return Some(skip);
    }
    if !geometry::geometry_coherent(i.sig) {
        return Some(Skip::new("incoherentGeometry"));
    }
    if let Some(key) = btc_break::entry_block(i.regime, i.sig.direction) {
        return Some(Skip::new(key));
    }
    // One position per symbol+timeframe per bot (regeneration re-publishes
    // the same market); transient — it clears when that position closes.
    if i.holds_market {
        return Some(Skip::new("marketHeld"));
    }
    None
}

/// The fill model against the live price. Ok(true) = fill now, Ok(false) =
/// stay pending silently, Err = skip.
pub fn price_gate(sig: &Signal, price: Option<f64>, now_ms: u64) -> Result<bool, Skip> {
    let Some(price) = price.filter(|p| *p > 0.0) else {
        return Err(Skip::new("priceUnavailable"));
    };
    match geometry::fill_decision(sig, price, now_ms) {
        FillDecision::Fill => Ok(true),
        FillDecision::Wait => Ok(false),
        FillDecision::Refuse => Err(Skip::new("fillCrossedLevel")),
    }
}

/// A fill decided at the live price, before the network prechecks.
#[derive(Debug, Clone)]
pub struct PlannedEntry {
    /// The live price the paper book fills at.
    pub price: f64,
    /// The user's target for this symbol (bot-wide or per-symbol).
    pub target: TakeProfitTarget,
    /// That target resolved at `price`.
    pub tp: TpResolution,
    /// Leverage already clamped by the risk level.
    pub leverage: u8,
    pub size: Size,
}

/// The steps between the live price and the prechecks, in the desktop's
/// order: fill model → take-profit resolution → sizing. Ok(None) = stay
/// pending silently; Err = skip (final or transient, see `precheck::is_final`).
/// `leverage` is `precheck::effective_leverage(cfg, limits)`.
pub fn plan_entry(
    cfg: &BotConfig,
    sig: &Signal,
    price: Option<f64>,
    now_ms: u64,
    leverage: u8,
) -> Result<Option<PlannedEntry>, Skip> {
    if !price_gate(sig, price, now_ms)? {
        return Ok(None);
    }
    let price = price.unwrap_or_default();
    // The user's target (bot-wide or per-symbol) resolved at this fill; a
    // target that cannot apply refuses the entry, visibly.
    let target = cfg.take_profit_for(&sig.symbol);
    let tp = take_profit::resolve_tp(sig, target, price)?;
    // Size before the prechecks: the depth check needs the real notional,
    // and paper and live use this one size.
    let size = sizing::size_position(cfg, leverage, price, sig.sl).map_err(Skip::new)?;
    // The live path's liquidation rule, applied to paper too: an isolated
    // position whose stop sits beyond its liquidation price can never reach
    // the stop, and a paper book that took it recorded impossible losses.
    sizing::liquidation_gate(cfg, &size, leverage, price, sig.sl).map_err(Skip::new)?;
    Ok(Some(PlannedEntry {
        price,
        target,
        tp,
        leverage,
        size,
    }))
}

/// The position a passed entry opens: filled at `entry` with take-profit `tp`
/// (the plan's own for paper; re-resolved at the real fill for live), sized
/// by the plan, carrying the precheck's FR/LD context.
pub fn finish_position(
    cfg: &BotConfig,
    sig: &Signal,
    entry: f64,
    tp: &TpResolution,
    plan: &PlannedEntry,
    now_ms: u64,
    ctx: &PrecheckContext,
) -> OpenPosition {
    let mut pos = new_position(cfg, sig, entry, plan.leverage, now_ms, tp);
    sizing::apply_size(&mut pos, &plan.size);
    (pos.fr_at_open, pos.ld_at_open) = (ctx.fr_at_open, ctx.ld_at_open);
    pos
}

/// A simulated position filled at `entry`, carrying the signal's plan state
/// and the resolved take-profit.
pub fn new_position(
    cfg: &BotConfig,
    sig: &Signal,
    entry: f64,
    leverage: u8,
    now: u64,
    tp: &TpResolution,
) -> OpenPosition {
    OpenPosition {
        signal_id: sig.id.clone(),
        bot_kind: cfg.kind,
        exchange_id: cfg.exchange_id.clone(),
        symbol: sig.symbol.clone(),
        timeframe: sig.timeframe.clone(),
        direction: match sig.direction {
            Direction::Long => "long".into(),
            Direction::Short => "short".into(),
        },
        entry,
        tp: tp.tp,
        sl: sig.sl,
        signal_entry: sig.entry,
        risk_r: geometry::risk_r(sig),
        plan: sig.management_plan,
        breakeven_armed: false,
        partial_fraction: 0.0,
        partial_price: None,
        horizon_ms: geometry::horizon_ms(sig),
        leverage,
        capital: cfg.capital,
        fr_at_open: None,
        ld_at_open: None,
        opened_at: now,
        live: false,
        qty: 0.0,
        entry_order_id: None,
        stop_algo_id: None,
        tp_algo_id: None,
        stop_at_breakeven: false,
        unprotected: false,
        partial_qty: 0.0,
        // Unsized until `sizing::apply_size` (0 ⇒ PnL uses `leverage`, the
        // same fallback a pre-sizing persisted row takes).
        sizing_mode: cfg.sizing,
        risk_pct: None,
        notional_usdt: 0.0,
        effective_leverage: 0.0,
        risk_capped: false,
        tp_target: tp.target.clone(),
        tp_fallback_from: tp.fallback_from.clone(),
    }
}
