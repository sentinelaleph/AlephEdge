//! Mirrors the management plan's state changes onto the exchange for LIVE
//! positions: breakeven moves the exchange stop, a partial reduces the
//! exchange quantity.
//!
//! Before this, both happened only in the book. With the app closed after
//! breakeven armed, the exchange stop still sat at the original SL — the user
//! was exposed to a full stop the book believed was gone. The planners are
//! pure so the ORDER of real orders is tested, not hoped for.

use tauri::{AppHandle, Manager};

use crate::exchange::providers::binance_requests::Protective;
use crate::exchange::providers::binance_rules::{quantize_price, quantize_qty, LotStep};
use crate::exchange::{BreakevenMove, ExchangeManager};
use crate::vault::VaultManager;

use super::super::model::OpenPosition;
use super::super::BotManager;
use super::live::{
    NOTE_BREAKEVEN_APP_ONLY, NOTE_BREAKEVEN_FAILED, NOTE_PARTIAL_BELOW_MIN, NOTE_PARTIAL_FAILED,
    NOTE_STALE_STOP, SKIP_VAULT_LOCKED,
};

/// One real order step of a stop replacement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StopAction {
    /// Place a reduce-only quantity stop at `trigger` (Binance).
    Place { trigger: f64 },
    /// Cancel the previous stop by id (Binance, after `Place`).
    Cancel { algo_id: i64 },
    /// Change the whole-position stop `algo_id` to `trigger` in place
    /// (Bybit / OKX). Nothing is cancelled afterwards.
    Move { algo_id: i64, trigger: f64 },
    /// No stop to move (Bybit / OKX): place a whole-position stop.
    PlaceWhole { trigger: f64 },
}

/// The breakeven replacement for `pos`, in execution order. Empty when
/// nothing changes.
///
/// Binance (`StopBeside`): NEW STOP FIRST, then cancel the old one: the
/// reverse order opens a window — however short — in which a leveraged
/// position has no stop at all. The new stop is a reduce-only QUANTITY stop
/// (see `reduce_stop_query`), so it can sit beside the original
/// closePosition stop; whichever fires first closes what is left and the
/// other has nothing to reduce.
///
/// Bybit / OKX (`MoveInPlace`): the position-level stop is changed where it
/// is, one request, no cancel. On Bybit the stop is a slot on the position:
/// the Binance plan's cancel would write "0" into the stop just moved.
pub fn plan_breakeven_move(pos: &OpenPosition, how: BreakevenMove) -> Vec<StopAction> {
    // Planning only; the caller (book.rs) checks `is_live()` before any of
    // these actions reach the exchange.
    if !pos.live || !pos.breakeven_armed || pos.stop_at_breakeven || pos.entry <= 0.0 {
        return Vec::new();
    }
    let trigger = pos.entry;
    match (how, pos.stop_algo_id) {
        (BreakevenMove::StopBeside, old) => {
            let mut actions = vec![StopAction::Place { trigger }];
            if let Some(algo_id) = old {
                actions.push(StopAction::Cancel { algo_id });
            }
            actions
        }
        (BreakevenMove::MoveInPlace, Some(algo_id)) => vec![StopAction::Move { algo_id, trigger }],
        (BreakevenMove::MoveInPlace, None) => vec![StopAction::PlaceWhole { trigger }],
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PartialPlan {
    /// Reduce-only MARKET for this (step-quantized) quantity.
    Reduce(f64),
    /// The fraction floors below the symbol minimum: no order.
    BelowMinimum,
}

/// Quantizes the partial leg to LOT_SIZE (floor, never up).
pub fn plan_partial(qty: f64, fraction: f64, lot: LotStep) -> PartialPlan {
    match quantize_qty(qty * fraction.clamp(0.0, 1.0), lot) {
        q if q > 0.0 && q < qty => PartialPlan::Reduce(q),
        _ => PartialPlan::BelowMinimum,
    }
}

/// Book state when the exchange did NOT reduce: the leg is dropped (fraction
/// 0, so PnL blends nothing in) but `partial_price` stays set so the plan does
/// not fire again every tick. The trade then settles on real fills anyway.
pub fn drop_partial(pos: &mut OpenPosition) {
    pos.partial_fraction = 0.0;
    pos.partial_qty = 0.0;
}

/// Executes a stop replacement if one is due. Returns true when position
/// state changed (persist it).
pub async fn sync_breakeven(app: &AppHandle, pos: &mut OpenPosition) -> bool {
    let bots = app.state::<BotManager>();
    let Some(how) = crate::exchange::breakeven_move(&pos.exchange_id) else {
        if plan_breakeven_move(pos, BreakevenMove::MoveInPlace).is_empty() {
            return false;
        }
        // A venue whose order path has not passed its dry run (a position
        // left from a sandbox session): no exchange-side breakeven. The
        // book's own breakeven exit works while the app runs; with the app
        // closed the ORIGINAL stop is what the exchange holds. Said once,
        // not retried (a retry can never succeed here).
        bots.skip_once(pos.bot_kind, &pos.signal_id, &pos.symbol, NOTE_BREAKEVEN_APP_ONLY, None);
        return false;
    };
    let actions = plan_breakeven_move(pos, how);
    if actions.is_empty() {
        return false;
    }
    let exchange = app.state::<ExchangeManager>();
    let Some(cred) = app.state::<VaultManager>().credential(&pos.exchange_id) else {
        bots.skip_once(
            pos.bot_kind,
            &pos.signal_id,
            &pos.symbol,
            SKIP_VAULT_LOCKED,
            None,
        );
        return false;
    };
    let long = pos.direction == "long";
    let mut changed = false;
    for action in actions {
        match action {
            StopAction::Place { trigger } => {
                // What is still open on the exchange: the fill minus any
                // partial already reduced.
                let remaining = (pos.qty - pos.partial_qty).max(0.0);
                let placed = match exchange.symbol_rules(&cred, &pos.symbol).await {
                    Ok(rules) if remaining > 0.0 => {
                        let trigger = quantize_price(trigger, rules.tick);
                        let qty = quantize_qty(remaining, rules.lot);
                        if qty <= 0.0 {
                            None
                        } else {
                            exchange
                                .place_reduce_stop(&cred, &pos.symbol, long, qty, trigger)
                                .await
                                .ok()
                        }
                    }
                    _ => None,
                };
                let Some(id) = placed else {
                    // Old stop untouched: still protected at the original SL,
                    // and the book's own breakeven exit keeps working. Retried
                    // next tick.
                    bots.skip_once(
                        pos.bot_kind,
                        &pos.signal_id,
                        &pos.symbol,
                        NOTE_BREAKEVEN_FAILED,
                        None,
                    );
                    return changed;
                };
                (pos.stop_algo_id, pos.stop_at_breakeven, changed) = (Some(id), true, true);
            }
            StopAction::Move { algo_id, trigger } => {
                // One request on the venue; on any failure the old stop is
                // where it was (still protected at the original SL) and the
                // book's own breakeven exit keeps working. Retried next tick.
                let moved = match exchange.symbol_rules(&cred, &pos.symbol).await {
                    Ok(rules) => exchange
                        .move_stop(&cred, &pos.symbol, algo_id, quantize_price(trigger, rules.tick))
                        .await
                        .ok(),
                    Err(_) => None,
                };
                let Some(id) = moved else {
                    bots.skip_once(pos.bot_kind, &pos.signal_id, &pos.symbol, NOTE_BREAKEVEN_FAILED, None);
                    return changed;
                };
                (pos.stop_algo_id, pos.stop_at_breakeven, changed) = (Some(id), true, true);
            }
            StopAction::PlaceWhole { trigger } => {
                let placed = match exchange.symbol_rules(&cred, &pos.symbol).await {
                    Ok(rules) => exchange
                        .place_protective(&cred, &pos.symbol, long, Protective::Stop, quantize_price(trigger, rules.tick))
                        .await
                        .ok(),
                    Err(_) => None,
                };
                let Some(id) = placed else {
                    bots.skip_once(pos.bot_kind, &pos.signal_id, &pos.symbol, NOTE_BREAKEVEN_FAILED, None);
                    return changed;
                };
                (pos.stop_algo_id, pos.stop_at_breakeven, changed) = (Some(id), true, true);
            }
            StopAction::Cancel { algo_id } => {
                // A stale wider stop is harmless while the position lives and
                // is swept by the symbol cancel-all when it closes.
                if exchange.cancel_algo(&cred, &pos.symbol, algo_id).await.is_err() {
                    bots.skip_once(
                        pos.bot_kind,
                        &pos.signal_id,
                        &pos.symbol,
                        NOTE_STALE_STOP,
                        None,
                    );
                }
            }
        }
    }
    changed
}

/// Sends the partial leg the plan just banked in the book, and rewrites the
/// book leg with the REAL fill (price and actual fraction).
pub async fn execute_partial(app: &AppHandle, pos: &mut OpenPosition) {
    let bots = app.state::<BotManager>();
    let exchange = app.state::<ExchangeManager>();
    let Some(cred) = app.state::<VaultManager>().credential(&pos.exchange_id) else {
        drop_partial(pos);
        bots.push_skip(&pos.symbol, NOTE_PARTIAL_FAILED, None);
        return;
    };
    let Ok(rules) = exchange.symbol_rules(&cred, &pos.symbol).await else {
        drop_partial(pos);
        bots.push_skip(&pos.symbol, NOTE_PARTIAL_FAILED, None);
        return;
    };
    let q = match plan_partial(pos.qty, pos.partial_fraction, rules.lot) {
        PartialPlan::Reduce(q) => q,
        PartialPlan::BelowMinimum => {
            drop_partial(pos);
            bots.push_skip(&pos.symbol, NOTE_PARTIAL_BELOW_MIN, None);
            return;
        }
    };
    let long = pos.direction == "long";
    match exchange
        .reduce_market(&cred, &pos.symbol, long, q)
        .await
    {
        Ok(fill) if pos.qty > 0.0 => {
            pos.partial_price = Some(fill.avg_price);
            pos.partial_qty = fill.executed_qty;
            pos.partial_fraction = (fill.executed_qty / pos.qty).clamp(0.0, 1.0);
        }
        _ => {
            drop_partial(pos);
            bots.push_skip(&pos.symbol, NOTE_PARTIAL_FAILED, None);
        }
    }
}
