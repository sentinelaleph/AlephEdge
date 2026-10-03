//! Resolves the user's take-profit target to one price at entry, pure.
//!
//! Sentinel publishes TP1..TP3; the bot used to exit at TP1 only. The user
//! may now pick TP2, TP3 or a custom percent from the fill (bot-wide or per
//! symbol). The SIGNAL is still judged on TP1 — geometry and the fill-drift
//! check in `geometry.rs` are unchanged — only the exit level moves.

use crate::bot::model::TakeProfitTarget;
use crate::signal::model::{Direction, Signal};

use super::precheck::Skip;

/// Refused: custom percent non-finite, out of bounds, or (short) it would
/// put the target at or below zero.
pub const SKIP_TP_CUSTOM_INVALID: &str = "tpCustomInvalid";
/// Refused: the resolved target does not lie beyond the fill in the trade's
/// direction (it would exit at once, or never as a profit).
pub const SKIP_TP_NOT_BEYOND_FILL: &str = "tpNotBeyondFill";

/// The exit level a position opens with.
#[derive(Debug, Clone, PartialEq)]
pub struct TpResolution {
    pub tp: f64,
    /// "tp1" | "tp2" | "tp3" | "custom:40".
    pub target: String,
    /// The REQUESTED target when the signal lacked it and a lower one was
    /// used instead.
    pub fallback_from: Option<String>,
}

impl TpResolution {
    /// The pre-2026-09-22 behaviour, unchecked: the signal's TP1 (0 when the
    /// signal has none — geometry refuses such a signal before any fill).
    pub fn signal_tp1(sig: &Signal) -> Self {
        Self {
            tp: sig.tp1().unwrap_or_default(),
            target: "tp1".into(),
            fallback_from: None,
        }
    }
}

fn index_label(i: usize) -> String {
    format!("tp{}", i + 1)
}

/// "tp2" → "TP2", "custom:40" → "+40%" (the skip note's `detail`).
fn display_label(target: &str) -> String {
    match target.strip_prefix("custom:") {
        Some(pct) => format!("+{pct}%"),
        None => target.to_uppercase(),
    }
}

fn usable(v: f64) -> bool {
    v.is_finite() && v > 0.0
}

/// Resolves `target` for `sig` filled at `fill`.
///
/// * TP2/TP3 the signal lacks: the highest target it does have, recorded in
///   `fallback_from`.
/// * Custom: `fill × (1 ± pct/100)`; pct finite in 0.1–1000, and a short's
///   target must stay above 0.
/// * The result must lie beyond `fill` in the trade's direction.
pub fn resolve_tp(sig: &Signal, target: TakeProfitTarget, fill: f64) -> Result<TpResolution, Skip> {
    let long = sig.direction == Direction::Long;
    let resolution = match target {
        TakeProfitTarget::Custom { pct } => {
            if !target.pct_in_bounds() {
                return Err(Skip::with(SKIP_TP_CUSTOM_INVALID, format!("{pct}")));
            }
            let sign = if long { 1.0 } else { -1.0 };
            let tp = fill * (1.0 + sign * pct / 100.0);
            if !usable(tp) {
                return Err(Skip::with(SKIP_TP_CUSTOM_INVALID, format!("{pct}")));
            }
            TpResolution {
                tp,
                target: format!("custom:{pct}"),
                fallback_from: None,
            }
        }
        indexed => {
            let wanted = match indexed {
                TakeProfitTarget::Tp2 => 1,
                TakeProfitTarget::Tp3 => 2,
                _ => 0,
            };
            // Highest usable target at or below the requested one.
            let Some(used) = (0..=wanted)
                .rev()
                .find(|&i| sig.tp.get(i).is_some_and(|v| usable(*v)))
            else {
                return Err(Skip::new("incoherentGeometry"));
            };
            TpResolution {
                tp: sig.tp[used],
                target: index_label(used),
                fallback_from: (used != wanted).then(|| index_label(wanted)),
            }
        }
    };
    let beyond = if long {
        resolution.tp > fill
    } else {
        resolution.tp < fill
    };
    if !beyond || !usable(fill) {
        return Err(Skip::with(
            SKIP_TP_NOT_BEYOND_FILL,
            display_label(&resolution.target),
        ));
    }
    Ok(resolution)
}
