//! The engine tick's pure decisions, moved out of `engine.rs` (which does the
//! I/O and holds the Tauri handle) so the headless bot farm
//! (`crates/aleph-bot-farm`) runs the very same rules: where a feed resumes
//! reading, when a fetch is pointless, when a feed is stale, which global
//! reason holds a bot's new cycles, how funding events land on bars, the bar
//! walk itself and the venue sync before a write. No Tauri, no I/O.

use std::collections::HashSet;

use super::cycle::CycleState;
use super::driver::{self, EquityPoint};
use super::limits::{leverage_ceiling, PRICE_STALE_MS};
use super::model::{Bar, Fill, FillKind, MarketKind, SimOrder, StrategyBot};
use super::venue::PaperVenue;

/// Every funding schedule (8h, 4h, 1h) settles at 00/08/16 UTC.
pub const FUNDING_EVERY_MS: u64 = 8 * 3_600_000;

/// Price-feed state of one (market, symbol).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Feed {
    /// Close time of the last processed closed bar (ms).
    pub last_close_ms: u64,
    pub last_close: f64,
    /// Last successful fetch (ms).
    pub last_ok_ms: u64,
}

/// Where a feed group reads from this tick, or None when nothing new can
/// have closed yet. `resume` = the oldest bar an open cycle of the group has
/// walked (close ms, > 0); `any_open` = the group holds an open cycle.
///
/// The cursor is written after the bots' own transactions; when a bot write
/// failed (or the cursor row is missing) the cycle is behind it and must
/// replay from its own bar, never jump the gap. A cursor is replayed only
/// when a cycle depends on it; otherwise a fresh start reads from the current
/// minute (no needless backlog).
pub fn read_from(feed: &Feed, resume: Option<u64>, any_open: bool, now: u64) -> Option<u64> {
    let start_ms = match resume {
        Some(ms) if feed.last_close_ms == 0 || ms < feed.last_close_ms => ms + 1,
        _ if feed.last_close_ms > 0 && (any_open || now.saturating_sub(feed.last_close_ms) <= 10 * 60_000) => {
            feed.last_close_ms + 1
        }
        _ => {
            let min = now - now % 60_000;
            min.saturating_sub(60_000)
        }
    };
    // Nothing new can have closed yet.
    if feed.last_close_ms > 0 && now < feed.last_close_ms + 1 + 60_000 + 1_500 && start_ms == feed.last_close_ms + 1 {
        return None;
    }
    Some(start_ms)
}

/// A fetch that returned no bars pauses the group's bots ("priceStale") when
/// the feed has no fresh bar and has not answered for `PRICE_STALE_MS`.
/// `feed.last_ok_ms` must already carry this tick's successful fetch.
pub fn feed_stale(feed: &Feed, now: u64) -> bool {
    let stale = feed.last_close_ms == 0 || now.saturating_sub(feed.last_close_ms) > PRICE_STALE_MS;
    stale && now.saturating_sub(feed.last_ok_ms.max(feed.last_close_ms)) > PRICE_STALE_MS
}

/// Global reasons new cycles are held this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gates {
    /// The portfolio breaker is tripped (holds only the bots inside it).
    pub portfolio: bool,
    pub membership_inactive: bool,
    pub btc_break: bool,
    /// The risk level's leverage ceiling now: a bot above it (the level was
    /// lowered after Start) opens no new cycle; its open cycle runs on.
    pub max_leverage: u8,
}

impl Gates {
    pub fn for_bot(&self, bot: &StrategyBot) -> Option<&'static str> {
        if self.portfolio && bot.cfg.portfolio_breaker {
            Some("portfolioDdTripped")
        } else if self.membership_inactive {
            Some("membershipInactive")
        } else if bot.cfg.leverage > leverage_ceiling(self.max_leverage, bot.cfg.market == MarketKind::Spot) {
            Some("leverageAboveCeiling")
        } else if self.btc_break && bot.cfg.pause_on_btc_break {
            Some("btcBreak")
        } else {
            None
        }
    }
}

/// Whether funding must be read for these (futures) bars at all: only a
/// batch containing an hour start can hold a settlement.
pub fn needs_funding(bars: &[Bar]) -> bool {
    bars.iter().any(|b| b.open_ms % 3_600_000 == 0)
}

/// Attaches funding events (ts ms, rate) to the bars containing them. A read
/// that came back without the 00/08/16 UTC event (history not yet published)
/// is not "no funding": flagged, never charged 0 silently. A failed read
/// (`Err`) marks every such bar unknown.
pub fn apply_funding(bars: &mut [Bar], events: Result<&[(u64, f64)], ()>) {
    match events {
        Ok(events) => {
            for (ts, rate) in events {
                if let Some(b) = bars.iter_mut().find(|b| b.open_ms <= *ts && *ts <= b.close_ms) {
                    b.funding_rate = Some(b.funding_rate.unwrap_or(0.0) + rate);
                }
            }
            for b in bars.iter_mut().filter(|b| b.open_ms % FUNDING_EVERY_MS == 0 && b.funding_rate.is_none()) {
                b.funding_unknown = true;
            }
        }
        Err(()) => {
            for b in bars.iter_mut().filter(|b| b.open_ms % FUNDING_EVERY_MS == 0) {
                b.funding_unknown = true;
            }
        }
    }
}

/// The exchange's still-forming kline: a response shorter than the page
/// limit reached the present, and the exchange always returns the forming bar
/// last. `close_ms > now` alone drops it only while the local clock is not
/// ahead of the exchange's; when nothing was dropped (`raw_len == kept`) and
/// the last bar closed less than a minute ago, it may still be forming and
/// is held for the next tick (where it reappears before a newer bar if it
/// really closed). True = drop the last kept bar.
pub fn hold_forming_tail(raw_len: usize, kept: usize, limit: usize, last_close_ms: Option<u64>, now_ms: u64) -> bool {
    raw_len < limit && raw_len == kept && last_close_ms.is_some_and(|c| c + 60_000 > now_ms)
}

/// Everything a bot produced, written in one transaction.
#[derive(Debug, Default)]
pub struct Batch {
    /// Market orders already filled (manual / kill closes).
    pub market_orders: Vec<SimOrder>,
    pub cycles: Vec<CycleState>,
    pub fills: Vec<(u32, Fill)>,
    pub equity: Vec<EquityPoint>,
}

impl Batch {
    /// Whether the walk produced anything to write.
    pub fn wrote(&self) -> bool {
        !self.fills.is_empty() || !self.cycles.is_empty() || !self.equity.is_empty()
    }
}

pub type Notes = Vec<(&'static str, Option<String>)>;

/// Pure bar walk of one bot (no store, no Tauri). Every bar after the bot's
/// own position is offered exactly once: a bot with an open cycle resumes
/// after the cycle's last walked bar, an idle bot after `seen_until`. When
/// a lagging cycle closes partway through a replay, the bars after the close
/// (up to the old cursor) were never offered to this bot: they are walked
/// for a new start, utilisation and equity like any other bar.
/// Returns (batch, notes, whether a cycle opened).
pub fn walk_bars(
    bot: &mut StrategyBot,
    cycle: &mut Option<CycleState>,
    bars: &[Bar],
    gate: Option<&'static str>,
    seen_until: u64,
) -> (Batch, Notes, bool) {
    let mut batch = Batch::default();
    let mut notes = Vec::new();
    let mut opened = false;
    // The last bar this bot has already been offered.
    let mut walked_until = cycle.as_ref().map(|c| c.core.last_bar_close_ms).unwrap_or(seen_until);
    for bar in bars {
        if bar.close_ms <= walked_until {
            continue; // already walked (cursor shared across bots)
        }
        walked_until = bar.close_ms;
        let out = driver::on_bar(bot, cycle, bar, gate);
        opened |= out.opened.is_some();
        batch.fills.extend(out.events);
        if let Some(c) = out.closed {
            batch.cycles.push(c);
        }
        if let Some(e) = out.equity {
            batch.equity.push(e);
        }
        notes.extend(out.notes);
    }
    (batch, notes, opened)
}

/// Client ids carry the cycle seq: "ae{bot8}-{seq}-{role}".
pub fn seq_of(client_id: &str) -> u32 {
    client_id.split('-').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// Brings the paper venue's book in line with the open cycle before a write:
/// the orders to store, each with its cycle seq (the venue's changes, then
/// the already-filled market orders of the batch).
pub fn sync_orders(venue: &mut PaperVenue, open: Option<&CycleState>, batch_fills: &[(u32, Fill)], market_orders: Vec<SimOrder>) -> Vec<(u32, SimOrder)> {
    let filled: HashSet<String> = batch_fills
        .iter()
        .filter(|(_, f)| f.kind == FillKind::Fill)
        .map(|(_, f)| f.client_id.clone())
        .collect();
    let desired = open.map(|c| c.open_orders()).unwrap_or_default();
    venue
        .sync(desired, &filled)
        .into_iter()
        .chain(market_orders)
        .map(|o| (seq_of(&o.client_id), o))
        .collect()
}
