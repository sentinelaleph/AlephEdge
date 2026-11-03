//! Paper catch-up, pure: what a PAPER position missed while the app was
//! closed, asleep or off the feed, and the close Sentinel itself graded.
//!
//! Two sources, both Sentinel's (never an exchange):
//!   1. the signal's graded outcome (`/api/public/signals/{id}`): a paper
//!      position follows Sentinel's invalidation close always, and its
//!      stop / target / breakeven exit when the position runs Sentinel's own
//!      plan (same levels, TP1, no user loss cap); booked at Sentinel's
//!      exit price and time when the markets match;
//!   2. 1m candles of the gap (`/api/v1/market/candles/history`), replayed
//!      adverse-first through the same management step the live tick uses,
//!      so a stop touched while the app was closed is booked at its level and
//!      at the minute it happened, not at the price seen on restart.
//!
//! Live positions never come here: their stop and target rest on the
//! exchange and their exits are read from real fills.

use serde::Deserialize;

use crate::bot::strategy::backtest::Candle;
use crate::signal::model::Direction;

use super::super::model::OpenPosition;
use super::management::{self, Exit, Fill};

pub const MINUTE_MS: u64 = 60_000;

/// The fields of `/api/public/signals/{id}` the catch-up reads.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SentinelOutcome {
    /// "tp_hit" | "sl_hit" | "be_scratch" | "invalidated" | "expired_unresolved".
    #[serde(default)]
    pub exit_type: String,
    #[serde(default)]
    pub exit_at: String,
    #[serde(default)]
    pub exit_price: Option<f64>,
    /// "spot" | "futures": the series Sentinel graded on.
    #[serde(default)]
    pub market_type: String,
    #[serde(default)]
    pub stop_loss: f64,
    #[serde(default)]
    pub take_profit: Vec<f64>,
    #[serde(default)]
    pub management_plan: Option<serde_json::Value>,
    #[serde(default)]
    pub invalidation_reason: String,
    #[serde(default)]
    pub execution: Option<Execution>,
}

/// The graded trade spelled out; only whether its partial could fire.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Execution {
    #[serde(default)]
    pub partial_reachable: bool,
}

/// `{"data": {...}}` of the permalink endpoint.
pub fn parse_outcome(body: &str) -> Option<SentinelOutcome> {
    #[derive(Deserialize)]
    struct Wrap {
        data: SentinelOutcome,
    }
    serde_json::from_str::<Wrap>(body).ok().map(|w| w.data)
}

/// A close Sentinel decided for the signal this position follows.
#[derive(Debug, Clone, PartialEq)]
pub struct Forced {
    pub reason: &'static str,
    pub at_ms: u64,
    /// Sentinel's exit price; `None` when Sentinel graded another market than
    /// the position's, so the price comes from the position's own candles.
    pub price: Option<f64>,
    /// Sentinel's invalidation reason code, kept on the trade record.
    pub code: Option<String>,
}

/// The close to follow, if Sentinel has graded one this position shares.
pub fn forced_close(
    pos: &OpenPosition,
    o: &SentinelOutcome,
    max_loss_pct: Option<f64>,
    position_market: &str,
) -> Option<Forced> {
    let reason = followed_reason(&o.exit_type)?;
    if reason != "invalidated" && !runs_sentinel_plan(pos, o, max_loss_pct) {
        return None;
    }
    let at_ms = crate::signal::time::parse_rfc3339_ms(&o.exit_at)?;
    if at_ms < pos.opened_at {
        return None;
    }
    let price = o
        .exit_price
        .filter(|p| p.is_finite() && *p > 0.0)
        .filter(|_| o.market_type == position_market);
    let code = (reason == "invalidated" && !o.invalidation_reason.trim().is_empty())
        .then(|| o.invalidation_reason.clone());
    Some(Forced { reason, at_ms, price, code })
}

/// The book's exit reason for Sentinel's graded exit type. Expired positions
/// carry no exit price; the position's own horizon closes those.
pub fn followed_reason(exit_type: &str) -> Option<&'static str> {
    match exit_type {
        "invalidated" => Some("invalidated"),
        "tp_hit" => Some("tp"),
        "sl_hit" => Some("sl"),
        "be_scratch" => Some("breakeven"),
        _ => None,
    }
}

/// Whether the position's exit rules are Sentinel's graded ones: the same
/// stop and TP1, a plan exactly when Sentinel has one, and no user loss cap.
/// A TP2/TP3/custom target or a cap exits on its own rules (the replay).
fn runs_sentinel_plan(pos: &OpenPosition, o: &SentinelOutcome, max_loss_pct: Option<f64>) -> bool {
    let same = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1e-12);
    pos.tp_target == "tp1"
        && pos.tp_fallback_from.is_none()
        && max_loss_pct.is_none()
        && o.take_profit.first().is_some_and(|&tp| same(pos.tp, tp))
        && same(pos.sl, o.stop_loss)
        && pos.plan.is_some() == o.management_plan.as_ref().is_some_and(|p| !p.is_null())
}

/// Replays closed 1m candles strictly before `until_ms` through the
/// management step: each candle's adverse extreme first, then its
/// favourable one, levels booked at their price (`Fill::Levels` watched).
/// A position past its horizon closes at the open of the first candle at or
/// after it. Returns the exit and the minute it happened; the position's
/// management state (breakeven, partial) advances either way.
pub fn replay(
    pos: &mut OpenPosition,
    candles: &[Candle],
    max_loss_pct: Option<f64>,
    until_ms: u64,
) -> Option<(Exit, u64)> {
    let long = pos.direction == Direction::Long;
    for k in candles.iter().filter(|k| k.open_ms < until_ms) {
        if pos.horizon_ms > 0 && k.open_ms >= pos.horizon_ms {
            return Some((Exit { reason: "horizon", price: k.o }, k.open_ms));
        }
        let (adverse, favourable) = if long { (k.l, k.h) } else { (k.h, k.l) };
        for price in [adverse, favourable] {
            let step = management::close_step_with(
                pos,
                price,
                k.open_ms,
                false,
                max_loss_pct,
                Fill::Levels { watched: true },
            );
            if let Some(exit) = step.exit {
                return Some((exit, k.open_ms));
            }
        }
    }
    None
}

/// The close of the minute candle holding `at_ms` (or the first after it).
pub fn price_at(candles: &[Candle], at_ms: u64) -> Option<f64> {
    candles
        .iter()
        .find(|k| k.open_ms + MINUTE_MS > at_ms)
        .map(|k| k.c)
}

/// Sorted, deduplicated minute candles that have closed by `now_ms`.
pub fn closed_minutes(mut candles: Vec<Candle>, now_ms: u64) -> Vec<Candle> {
    candles.retain(|k| k.open_ms + MINUTE_MS <= now_ms && k.l > 0.0 && k.h >= k.l);
    candles.sort_by_key(|k| k.open_ms);
    candles.dedup_by_key(|k| k.open_ms);
    candles
}

/// A Sentinel signal id (a UUID): TradingView and user trades have their own
/// ids and no permalink.
pub fn is_sentinel_id(id: &str) -> bool {
    let b = id.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
}
