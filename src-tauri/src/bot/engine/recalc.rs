//! Recalculating a PAPER trade booked before the catch-up (`catch_up.rs`)
//! existed, pure: when Sentinel graded the signal's close more than two
//! minutes before the book closed it, the book had missed that close (the app
//! was shut, asleep or off the feed), and the trade is recomputed on
//! Sentinel's exit with the same NET formulas as any close. The stored row
//! is never rewritten (`store::recalc` keeps the result beside it).
//!
//! Only what the row can prove is recomputed. A trade row does not keep the
//! stop or whether a partial was banked, so:
//!   - a Sentinel invalidation is followed on any paper trade;
//!   - a Sentinel stop / target / breakeven only on a TP1 trade the bot
//!     closed by its plan (tp / sl / breakeven / horizon / veto), never on a
//!     user exit (loss cap, manual close, stop, kill);
//!   - a target or breakeven whose partial could fire is left as booked;
//!   - Sentinel's price is used only on the market Sentinel graded.

use crate::store::model::{TradeRecalc, TradeRecord};

use super::super::model::{BotKind, OpenPosition};
use super::catch_up::{followed_reason, SentinelOutcome};
use super::record::trade_record;

/// Sentinel's close must come this much earlier than the book's.
const MISSED_BY_MS: u64 = 2 * 60_000;

/// What a trade's recalculation found. `Kept` carries the reason code (kept
/// in the table, so a trade is checked once).
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Recalculated(TradeRecalc),
    Kept(&'static str),
}

/// Exits the bot booked by its plan; anything else was the user's own.
const PLAN_EXITS: [&str; 6] = ["tp", "sl", "breakeven", "horizon", "veto", "invalidated"];

/// `None` = Sentinel has graded nothing to compare yet (ask again later).
pub fn recalculate(t: &TradeRecord, o: &SentinelOutcome) -> Option<Verdict> {
    if t.live {
        return Some(Verdict::Kept("live"));
    }
    let Some(reason) = followed_reason(&o.exit_type) else {
        // Expired: no exit price on Sentinel's side. Open: not graded yet.
        return (o.exit_type == "expired_unresolved").then_some(Verdict::Kept("expired"));
    };
    let at_ms = crate::signal::time::parse_rfc3339_ms(&o.exit_at)?;
    if at_ms < t.opened_at {
        return Some(Verdict::Kept("beforeFill"));
    }
    if at_ms + MISSED_BY_MS > t.closed_at {
        return Some(Verdict::Kept("matches"));
    }
    if !PLAN_EXITS.contains(&t.exit_reason.as_str()) {
        return Some(Verdict::Kept("userExit"));
    }
    if reason != "invalidated" {
        if t.tp_target.as_deref().is_some_and(|tp| tp != "tp1") {
            return Some(Verdict::Kept("ownTarget"));
        }
        if reason != "sl" && o.execution.as_ref().is_some_and(|e| e.partial_reachable) {
            return Some(Verdict::Kept("partial"));
        }
    }
    let Some(kind) = BotKind::parse(&t.bot_kind) else {
        return Some(Verdict::Kept("unknownBot"));
    };
    let market = if kind.uses_futures_market() { "futures" } else { "spot" };
    let Some(price) = o.exit_price.filter(|p| p.is_finite() && *p > 0.0) else {
        return Some(Verdict::Kept("noPrice"));
    };
    if o.market_type != market {
        return Some(Verdict::Kept("otherMarket"));
    }
    let Some(pos) = position_of(t) else {
        return Some(Verdict::Kept("unreadable"));
    };
    let r = trade_record(&pos, price, reason, at_ms);
    Some(Verdict::Recalculated(TradeRecalc {
        exit: r.exit,
        exit_reason: r.exit_reason,
        closed_at: r.closed_at,
        pnl_pct: r.pnl_pct,
        pnl_quote: r.pnl_quote,
        unlevered_net_pct: r.unlevered_net_pct,
    }))
}

/// The position a trade row closed, as far as the NET formulas read it: the
/// fill, size and leverage. No partial (the row does not keep one; a trade
/// whose partial could have fired is never recalculated).
fn position_of(t: &TradeRecord) -> Option<OpenPosition> {
    serde_json::from_value(serde_json::json!({
        "signalId": t.signal_id,
        "botKind": t.bot_kind,
        "exchangeId": t.exchange_id,
        "symbol": t.symbol,
        "direction": t.direction,
        "entry": t.entry,
        "tp": 0.0,
        "sl": 0.0,
        "leverage": t.leverage,
        "capital": t.capital,
        "frAtOpen": t.fr_at_open,
        "ldAtOpen": t.ld_at_open,
        "openedAt": t.opened_at,
        "sizingMode": t.sizing_mode,
        "riskPct": t.risk_pct,
        "notionalUsdt": t.notional_usdt.unwrap_or(0.0),
        "effectiveLeverage": t.effective_leverage.unwrap_or(0.0),
        "riskCapped": t.risk_capped,
        "tpTarget": t.tp_target.clone().unwrap_or_else(|| "tp1".into()),
        "tpFallbackFrom": t.tp_fallback_from,
    }))
    .ok()
}
