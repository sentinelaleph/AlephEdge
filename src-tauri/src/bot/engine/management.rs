//! The close decision for one open position at one live price, pure.
//!
//! Mirrors Sentinel's evaluator so the paper book is comparable to the
//! ledger. Order per evaluation:
//!   0. isolated liquidation, paper only (see `EXIT_LIQUIDATION`),
//!   1. adverse stop (the signal SL, or the fill entry once breakeven armed
//!      in an EARLIER evaluation),
//!   2. partial — only if its level is nearer than the position's TP;
//!      otherwise it can never fire and the whole position exits at TP
//!      (Sentinel fixed this exact ordering bug on 2026-09-16: checking the
//!      partial first banked phantom 1R fills),
//!   3. TP — the RESOLVED target (`pos.tp`: TP1 by default, or the user's
//!      TP2/TP3/custom, see `take_profit.rs`),
//!   4. breakeven arming (effective from the next evaluation, never this one),
//!   5. horizon (`expires_at + 72h`) at the live price — a far target the
//!      price has not reached by then exits here, on time, not at the target,
//!   6. the user's own max-loss cap.
//!
//! Server invalidation does NOT close: the BTC guard closing filled positions
//! was measured at −163 pts and is OFF on the server. It only yields a note.
//!
//! Exit prices (`Fill`): the desktop books a paper stop, target or partial at
//! its level, as the resting exchange orders of a live position and
//! Sentinel's ledger do. Booking the polled price (about every 8 s) charged
//! every stop the move during the polling gap and credited every target its
//! overshoot (HANA, 7 Oct: a breakeven booked −0.79% instead of −0.08%).
//! The paper runner keeps the polled price for its pre-registered arms.

use super::super::model::OpenPosition;
use super::{pnl, sizing};

/// Exit reason: a PAPER position carried past its isolated liquidation
/// price (only possible for one opened before the entry-time liquidation
/// gate, or with the stop beyond liquidation). The exchange liquidates a live
/// position itself.
pub const EXIT_LIQUIDATION: &str = "liquidation";

/// A close this evaluation decided.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Exit {
    /// "tp" | "sl" | "breakeven" | "horizon" | "max_loss".
    pub reason: &'static str,
    pub price: f64,
}

/// Where an exit triggered by a level is booked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fill {
    /// At the price this evaluation saw. Live positions (their real fills
    /// are recorded from the exchange) and the paper runner's arms.
    Polled,
    /// Desktop paper. Targets and the partial at their level. The stop at
    /// its level when `watched` (this position was evaluated moments ago on
    /// the safe side, so price crossed the level since); otherwise (first
    /// look after a restart, a sleep or a feed outage) at the price seen
    /// now, the worse of the two: the crossing was not observed and may have
    /// been a gap.
    Levels { watched: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Step {
    pub exit: Option<Exit>,
    /// Management state changed (breakeven armed / partial banked): persist.
    pub changed: bool,
    /// The signal is invalidated server-side: surface a note, keep holding.
    pub note_invalidation: bool,
}

impl Step {
    /// Whether acting on this step must hold the close claim. A close does,
    /// a live position does (its exchange stop moves), and so does a mere
    /// state save: writing a paper position's row back while a veto close
    /// was deleting it resurrected the closed position on the next restart.
    pub fn needs_claim(&self, live: bool) -> bool {
        self.exit.is_some() || self.changed || live
    }
}

fn favourable(long: bool, price: f64, level: f64) -> bool {
    if long {
        price >= level
    } else {
        price <= level
    }
}

/// Evaluates `pos` at `price`, mutating its management state; every exit at
/// the polled price (`Fill::Polled`).
// The desktop calls `close_step_with`; the paper runner calls this.
#[allow(dead_code)]
pub fn close_step(
    pos: &mut OpenPosition,
    price: f64,
    now_ms: u64,
    invalidated: bool,
    max_loss_pct: Option<f64>,
) -> Step {
    close_step_with(pos, price, now_ms, invalidated, max_loss_pct, Fill::Polled)
}

/// `close_step` with the exit-price model explicit (see `Fill`).
pub fn close_step_with(
    pos: &mut OpenPosition,
    price: f64,
    now_ms: u64,
    invalidated: bool,
    max_loss_pct: Option<f64>,
    fill: Fill,
) -> Step {
    let mut step = Step {
        note_invalidation: invalidated,
        ..Step::default()
    };
    let long = pos.direction == "long";
    let sign = if long { 1.0 } else { -1.0 };
    // Legacy rows persisted before these fields existed: fall back to the
    // fill entry and plain TP/SL.
    let base = if pos.signal_entry > 0.0 {
        pos.signal_entry
    } else {
        pos.entry
    };
    let r = pos.risk_r;
    let plan = pos.plan.filter(|_| r > 0.0);

    let exit = |reason, step: &mut Step| {
        step.exit = Some(Exit { reason, price });
        *step
    };
    // A level exit's booked price (see `Fill`).
    let at_level = |level: f64| match fill {
        Fill::Polled => price,
        Fill::Levels { .. } => level,
    };
    let at_stop = |level: f64| match fill {
        Fill::Polled => price,
        Fill::Levels { watched: true } => level,
        Fill::Levels { watched: false } if long => level.min(price),
        Fill::Levels { watched: false } => level.max(price),
    };

    // The adverse stop level (step 1), needed by step 0 too.
    let stop = if pos.breakeven_armed {
        pos.entry
    } else {
        pos.sl
    };
    let stopped = if long { price <= stop } else { price >= stop };

    // 0. Isolated liquidation (paper only): price past the liquidation
    //    price with the stop BEYOND it — the stop could never have fired.
    //    When the stop is nearer it fires first and the loss is capped at
    //    the margin by `pnl`. Checked before the stop for that reason.
    if !pos.live {
        if let Some(liq) = sizing::liquidation_price(pos) {
            let liquidated = if long { price <= liq } else { price >= liq };
            let stop_first = if long { stop > liq } else { stop < liq };
            if liquidated && !stop_first {
                step.exit = Some(Exit {
                    reason: EXIT_LIQUIDATION,
                    price: liq,
                });
                return step;
            }
        }
    }

    // 1. Adverse stop.
    if stopped {
        let reason = if pos.breakeven_armed {
            "breakeven"
        } else {
            "sl"
        };
        step.exit = Some(Exit {
            reason,
            price: at_stop(stop),
        });
        return step;
    }

    // 2. Partial, only when nearer than the resolved TP.
    if let Some(plan) = plan {
        if let (Some(at_r), Some(frac)) = (plan.partial_at_r, plan.partial_fraction) {
            let nearer = at_r > 0.0 && at_r * r < (pos.tp - base).abs();
            let level = base + sign * at_r * r;
            if nearer && frac > 0.0 && pos.partial_price.is_none() && favourable(long, price, level)
            {
                pos.partial_fraction = frac.min(1.0);
                pos.partial_price = Some(at_level(level));
                step.changed = true;
            }
        }
    }

    // 3. TP — the remaining position exits whole.
    if favourable(long, price, pos.tp) {
        step.exit = Some(Exit {
            reason: "tp",
            price: at_level(pos.tp),
        });
        return step;
    }

    // 4. Breakeven arming — protects only from the next evaluation.
    if let Some(at_r) = plan.and_then(|p| p.breakeven_at_r) {
        if !pos.breakeven_armed && at_r > 0.0 && favourable(long, price, base + sign * at_r * r) {
            pos.breakeven_armed = true;
            step.changed = true;
        }
    }

    // 5. Horizon.
    if pos.horizon_ms > 0 && now_ms >= pos.horizon_ms {
        return exit("horizon", &mut step);
    }

    // 6. The user's per-position loss tolerance, regardless of BTC regime.
    if let Some(cap) = max_loss_pct {
        if pnl::breaches_cap(pos, price, cap) {
            return exit("max_loss", &mut step);
        }
    }
    step
}
