//! Signal geometry and the fill model, pure.
//!
//! There is deliberately NO absolute R:R floor. Live geometry is a 2.5×ATR
//! stop with TP1 at a median ≈0.6R (98.5% of winners have TP1 below 1R), so
//! the old `rr < 1.0` check refused nearly every published signal. Only
//! incoherent geometry is refused.
//!
//! Fill semantics mirror Sentinel's evaluator so the paper book is comparable:
//! a signal fills when price touches `entry` before `expires_at`, or at once
//! when the live price is within 0.5% ADVERSE drift of entry within 5 minutes
//! of `created_at` (a marketable entry). Otherwise it stays pending.

use crate::signal::model::{Direction, Signal};
use crate::signal::time::parse_rfc3339_ms;

/// Max adverse drift from the signal entry a fill may carry (fraction).
pub const MAX_ADVERSE_DRIFT: f64 = 0.005;
/// Window after `created_at` in which a signal is marketable.
pub const MARKETABLE_WINDOW_MS: u64 = 5 * 60 * 1000;
/// Positions touching neither level close this long after `expires_at`.
pub const HORIZON_AFTER_EXPIRY_MS: u64 = 72 * 60 * 60 * 1000;

/// Long: sl < entry < tp1. Short: tp1 < entry < sl. Missing TP1, non-finite
/// or non-positive prices are incoherent.
pub fn geometry_coherent(sig: &Signal) -> bool {
    let Some(tp1) = sig.tp1() else {
        return false;
    };
    let all = [sig.entry, sig.sl, tp1];
    if all.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return false;
    }
    match sig.direction {
        Direction::Long => sig.sl < sig.entry && sig.entry < tp1,
        Direction::Short => tp1 < sig.entry && sig.entry < sig.sl,
    }
}

/// Whether `fill` is an acceptable execution of `sig`: it must not have
/// crossed TP1 or SL already, nor drifted adversely more than 0.5% from the
/// signal entry (long: above entry·1.005; short: below entry·0.995).
pub fn fill_acceptable(sig: &Signal, fill: f64) -> bool {
    let Some(tp1) = sig.tp1() else {
        return false;
    };
    if !fill.is_finite() || fill <= 0.0 {
        return false;
    }
    match sig.direction {
        Direction::Long => {
            fill < tp1 && fill > sig.sl && fill <= sig.entry * (1.0 + MAX_ADVERSE_DRIFT)
        }
        Direction::Short => {
            fill > tp1 && fill < sig.sl && fill >= sig.entry * (1.0 - MAX_ADVERSE_DRIFT)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillDecision {
    /// Open now at the live price.
    Fill,
    /// Not touched and not marketable: stay pending.
    Wait,
    /// Touched/marketable but the price is unusable (crossed a level). Final.
    Refuse,
}

/// Decides a pending signal against the live price.
pub fn fill_decision(sig: &Signal, price: f64, now_ms: u64) -> FillDecision {
    let touched = match sig.direction {
        Direction::Long => price <= sig.entry,
        Direction::Short => price >= sig.entry,
    };
    let marketable = parse_rfc3339_ms(&sig.created_at)
        .is_some_and(|created| now_ms <= created.saturating_add(MARKETABLE_WINDOW_MS))
        && fill_acceptable(sig, price);
    if !(touched || marketable) {
        return FillDecision::Wait;
    }
    if fill_acceptable(sig, price) {
        FillDecision::Fill
    } else {
        FillDecision::Refuse
    }
}

/// R = |entry − sl| of the SIGNAL (not the fill), as the ledger measures it.
pub fn risk_r(sig: &Signal) -> f64 {
    (sig.entry - sig.sl).abs()
}

/// `expires_at + 72h` in millis; 0 when the expiry is unparseable.
pub fn horizon_ms(sig: &Signal) -> u64 {
    parse_rfc3339_ms(&sig.expires_at)
        .map(|exp| exp + HORIZON_AFTER_EXPIRY_MS)
        .unwrap_or(0)
}
