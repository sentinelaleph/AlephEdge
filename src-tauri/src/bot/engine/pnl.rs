//! PnL formulas shared by the close decision and the trade record, so the
//! max-loss cap and the recorded trade can never disagree.
//!
//! A banked partial makes the trade two legs:
//! realised = partial_fraction·leg1 + (1 − partial_fraction)·leg2.
//! Fees are linear in notional, so blending per-leg NET figures is exact.

use super::super::model::{OpenPosition, FUTURES_FEE_RATE, SPOT_FEE_RATE};

/// Sentinel ledger costs, in % of the UNLEVERED move.
pub const LEDGER_ROUND_TRIP_PCT: f64 = 0.10;
pub const LEDGER_FUNDING_PCT: f64 = 0.02;

fn is_long(pos: &OpenPosition) -> bool {
    pos.direction == "long"
}

/// Unlevered price move in % from the fill entry, favourable positive.
fn move_pct(pos: &OpenPosition, price: f64) -> f64 {
    if pos.entry <= 0.0 {
        return 0.0;
    }
    let raw = (price - pos.entry) / pos.entry * 100.0;
    if is_long(pos) {
        raw
    } else {
        -raw
    }
}

/// Partial-blended unlevered move %: the banked leg at its fraction, the
/// remainder at `price`.
pub fn blended_move_pct(pos: &OpenPosition, price: f64) -> f64 {
    let f = pos.partial_fraction.clamp(0.0, 1.0);
    let leg1 = pos.partial_price.map(|p| move_pct(pos, p)).unwrap_or(0.0);
    f * leg1 + (1.0 - f) * move_pct(pos, price)
}

/// User-facing NET PnL % of capital: leveraged blended move minus both taker
/// fees on the leveraged notional (the existing desk fee model). The leverage
/// is the SIZED one (notional / capital), so in risk mode this is the real
/// share of capital; in fixed mode it equals the configured leverage exactly.
pub fn unrealized_net_pnl_pct(pos: &OpenPosition, price: f64) -> f64 {
    let lev = super::sizing::leverage_of(pos);
    let fee_rate = if pos.bot_kind.uses_futures_market() {
        FUTURES_FEE_RATE
    } else {
        SPOT_FEE_RATE
    };
    let raw = blended_move_pct(pos, price) * lev - 2.0 * fee_rate * lev * 100.0;
    // An isolated futures position can never lose more than its margin: the
    // exchange liquidates it first. Without this floor a paper stop gapped
    // through (or a pre-gate 20x / 7% stop) booked −140% of capital.
    match super::sizing::isolated_margin_pct(pos) {
        Some(margin) => raw.max(-margin),
        None => raw,
    }
}

/// A liquidation loses the whole isolated margin (what is left of it at the
/// liquidation price goes to the exchange's insurance fund).
pub fn liquidation_pnl_pct(pos: &OpenPosition) -> f64 {
    super::sizing::isolated_margin_pct(pos).map_or(-100.0, |margin| -margin)
}

/// Ledger-parity figure: unlevered blended move − 0.10 round trip − 0.02
/// funding (a long pays it, a short receives it), exactly Sentinel's costs.
pub fn unlevered_net_pct(pos: &OpenPosition, exit: f64) -> f64 {
    let funding = if is_long(pos) {
        LEDGER_FUNDING_PCT
    } else {
        -LEDGER_FUNDING_PCT
    };
    blended_move_pct(pos, exit) - LEDGER_ROUND_TRIP_PCT - funding
}

/// True when the unrealized net loss reaches the user's cap (`cap_pct` is
/// positive, e.g. 2.0 = "close at −2%"). Non-positive caps are ignored.
pub fn breaches_cap(pos: &OpenPosition, price: f64, cap_pct: f64) -> bool {
    cap_pct > 0.0 && unrealized_net_pnl_pct(pos, price) <= -cap_pct
}
