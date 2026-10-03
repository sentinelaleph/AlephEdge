//! Position size, pure — for paper and live alike.
//!
//! Why risk-based: the stop is 2.5×ATR, so the loss on a stop varies per
//! signal from ~3% to 7%+ of notional (avg 4.1% over 147 ledger stops). Wide
//! stops are not worse signals — the 4.5–6% stop bucket was the best one (168
//! trades, +173 pts) — so filtering them out would cut the edge. The damage is
//! FIXED sizing: capital × x5 turns a 7% stop into −35% of capital. Sizing from
//! the stop distance makes every stop cost the same pre-declared share of the
//! capital instead.
//!
//! `fixed` keeps the previous behaviour exactly (notional = capital ×
//! leverage) and is what a saved config without the field deserializes to, so
//! no existing bot changes size silently.

use super::super::model::{BotConfig, OpenPosition, SizingMode};

/// Round-trip cost added to the stop distance, as a fraction of notional:
/// Sentinel's ledger costs (0.10 round trip + 0.02 funding). Including it
/// makes the realised loss at the stop ≈ the declared risk, not risk + fees.
pub const ROUND_TRIP_COST_FRAC: f64 = 0.0012;
/// Declared risk bounds (% of the bot's capital lost at the stop).
pub const MIN_RISK_PCT: f64 = 0.1;
pub const MAX_RISK_PCT: f64 = 5.0;
pub const DEFAULT_RISK_PCT: f64 = 1.0;
/// Skip key: the stop distance is zero or not a number — risk sizing would
/// divide by it.
pub const SKIP_INVALID_STOP: &str = "sizingInvalidStop";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub mode: SizingMode,
    /// Declared risk % (risk mode only).
    pub risk_pct: Option<f64>,
    pub notional: f64,
    /// notional / entry, NOT step-quantized (the live path quantizes).
    pub qty: f64,
    /// notional / capital. May be below 1 in risk mode.
    pub effective_leverage: f64,
    /// The leverage cap bound the size: the real risk is BELOW the declared
    /// one. Surfaced rather than silently under-sizing.
    pub risk_capped: bool,
}

/// Sizes a position at `entry` (the planned fill) with stop `sl`.
/// `leverage_cap` is the bot's leverage already clamped by the risk level —
/// the notional in fixed mode, the ceiling in risk mode.
pub fn size_position(
    cfg: &BotConfig,
    leverage_cap: u8,
    entry: f64,
    sl: f64,
) -> Result<Size, &'static str> {
    let cap = cfg.capital * f64::from(leverage_cap);
    let qty_of = |notional: f64| if entry > 0.0 { notional / entry } else { 0.0 };
    match cfg.sizing {
        SizingMode::Fixed => Ok(Size {
            mode: SizingMode::Fixed,
            risk_pct: None,
            notional: cap,
            qty: qty_of(cap),
            effective_leverage: f64::from(leverage_cap),
            risk_capped: false,
        }),
        SizingMode::Risk => {
            let stop_frac = (entry - sl).abs() / entry;
            if !(entry.is_finite() && entry > 0.0 && stop_frac.is_finite() && stop_frac > 0.0) {
                return Err(SKIP_INVALID_STOP);
            }
            // Validated at configure time; clamped again so a hand-edited or
            // legacy value can never size outside the declared bounds.
            let risk_pct = cfg.risk_per_trade_pct.clamp(MIN_RISK_PCT, MAX_RISK_PCT);
            let risk_usdt = cfg.capital * risk_pct / 100.0;
            let wanted = risk_usdt / (stop_frac + ROUND_TRIP_COST_FRAC);
            let risk_capped = wanted > cap;
            let notional = wanted.min(cap);
            Ok(Size {
                mode: SizingMode::Risk,
                risk_pct: Some(risk_pct),
                notional,
                qty: qty_of(notional),
                effective_leverage: if cfg.capital > 0.0 {
                    notional / cfg.capital
                } else {
                    0.0
                },
                risk_capped,
            })
        }
    }
}

// ---- Isolated-margin liquidation, shared by paper and live ----
//
// The live path refuses an entry whose stop sits beyond the liquidation
// price; the paper book used to accept the same entry and then book losses no
// isolated position can make (fixed sizing, 20x, a 7% stop: −140% of capital).
// One rule for both, here, because the headless paper runner mounts this file.

/// Skip key (final): at this leverage the position would be liquidated before
/// price reached the stop. The key predates the paper check, hence `live`.
pub const SKIP_STOP_BEYOND_LIQUIDATION: &str = "liveStopBeyondLiquidation";
/// Maintenance-margin allowance in the liquidation check: Binance's rates
/// run 0.4%–2.5% on small notional, so assume the high end.
pub const MAINT_MARGIN_FRAC: f64 = 0.025;

/// Exchange leverage for an entry: enough that the isolated margin is the
/// bot's capital, never above the risk level's cap. Lower leverage on the
/// exchange puts liquidation further away for the same position.
pub fn exchange_leverage(notional: f64, capital: f64, cap: u8) -> u8 {
    let cap = cap.max(1);
    // NaN-safe: NaN is neither above nor at-or-below zero.
    if capital.is_nan() || notional.is_nan() || capital <= 0.0 || notional <= 0.0 {
        return cap;
    }
    let needed = (notional / capital).ceil().min(f64::from(cap));
    (needed as u8).clamp(1, cap)
}

/// True when an ISOLATED position at `leverage` would be liquidated before
/// price reaches the stop (`stop_frac` = |entry − stop| / entry), with a 10%
/// margin of safety. Liquidation sits roughly 1/leverage − maintenance away.
pub fn stop_beyond_liquidation(stop_frac: f64, leverage: u8) -> bool {
    let liq_frac = 1.0 / f64::from(leverage.max(1)) - MAINT_MARGIN_FRAC;
    stop_frac.is_nan() || stop_frac >= liq_frac * 0.9
}

/// The entry-time liquidation rule for a sized position, identical for paper
/// and live: futures-market bots only (spot is unlevered and cannot be
/// liquidated). `leverage_cap` is the risk-level-clamped leverage.
pub fn liquidation_gate(
    cfg: &BotConfig,
    size: &Size,
    leverage_cap: u8,
    entry: f64,
    sl: f64,
) -> Result<(), &'static str> {
    if !cfg.kind.uses_futures_market() {
        return Ok(());
    }
    let leverage = exchange_leverage(size.notional, cfg.capital, leverage_cap);
    let stop_frac = (entry - sl).abs() / entry;
    if stop_beyond_liquidation(stop_frac, leverage) {
        return Err(SKIP_STOP_BEYOND_LIQUIDATION);
    }
    Ok(())
}

/// The position's notional: the sized one, or capital × leverage for rows
/// persisted before sizing existed.
fn notional_of(pos: &OpenPosition) -> f64 {
    if pos.notional_usdt > 0.0 {
        pos.notional_usdt
    } else {
        pos.capital * leverage_of(pos)
    }
}

/// The exchange leverage this position runs (or, on paper, would run) at.
pub fn isolated_leverage(pos: &OpenPosition) -> u8 {
    exchange_leverage(notional_of(pos), pos.capital, pos.leverage)
}

/// The isolated margin as % of the bot's capital: the most a futures-market
/// position can lose. None for spot (no margin) or a capital-less row.
pub fn isolated_margin_pct(pos: &OpenPosition) -> Option<f64> {
    if !pos.bot_kind.uses_futures_market() || pos.capital.is_nan() || pos.capital <= 0.0 {
        return None;
    }
    let margin = notional_of(pos) / f64::from(isolated_leverage(pos));
    (margin.is_finite() && margin > 0.0).then(|| margin / pos.capital * 100.0)
}

/// Isolated-margin liquidation price (flat maintenance rate, no tiers):
/// long `entry·(1 − 1/L)/(1 − mmr)`, short `entry·(1 + 1/L)/(1 + mmr)`.
/// None for spot, a non-positive entry, or a 1x long (fully funded: it
/// cannot be liquidated). Any entry `liquidation_gate` passes has its stop
/// strictly before this price.
pub fn liquidation_price(pos: &OpenPosition) -> Option<f64> {
    if !pos.bot_kind.uses_futures_market() || pos.entry.is_nan() || pos.entry <= 0.0 {
        return None;
    }
    let inv = 1.0 / f64::from(isolated_leverage(pos));
    let liq = if pos.direction == "long" {
        pos.entry * (1.0 - inv) / (1.0 - MAINT_MARGIN_FRAC)
    } else {
        pos.entry * (1.0 + inv) / (1.0 + MAINT_MARGIN_FRAC)
    };
    (liq.is_finite() && liq > 0.0).then_some(liq)
}

/// Stamps the size onto a new position.
pub fn apply_size(pos: &mut OpenPosition, size: &Size) {
    pos.sizing_mode = size.mode;
    pos.risk_pct = size.risk_pct;
    pos.notional_usdt = size.notional;
    pos.qty = size.qty;
    pos.effective_leverage = size.effective_leverage;
    pos.risk_capped = size.risk_capped;
}

/// The leverage PnL is computed with: the sized one, or the configured
/// leverage for positions persisted before sizing existed.
pub fn leverage_of(pos: &OpenPosition) -> f64 {
    if pos.effective_leverage > 0.0 {
        pos.effective_leverage
    } else {
        f64::from(pos.leverage)
    }
}
