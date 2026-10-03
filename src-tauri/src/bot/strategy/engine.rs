//! Strategy tick phase (I/O side): runs inside `bot::engine::tick` after the
//! kill-switch phase and before the signal open phase. Per active (market,
//! symbol) it fetches CLOSED 1m bars since the stored cursor, walks every
//! bot through them with the pure driver, persists each bot's bar results in
//! one SQLite transaction, and applies the portfolio breaker.
//!
//! Paper only: the one venue is `PaperVenue`. Nothing in this subtree calls
//! a signed endpoint or the real-order module.

use std::collections::{BTreeMap, HashMap, HashSet};

use tauri::{AppHandle, Manager};

use crate::bot::engine::btc_break::BtcRegime;
use crate::bot::BotManager;
use crate::exchange::providers::klines::{self, Kline};
use crate::exchange::ExchangeManager;
use crate::membership::MembershipManager;
use crate::store::strategy as db;
use crate::store::{utc_day_start_ms, StoreManager};

use super::cycle::CycleState;
use super::driver::{self, EquityPoint};
use super::limits::PRICE_STALE_MS;
use super::model::{Bar, BotId, ExitReason, Fill, MarketKind, StrategyBot};
use super::venue::{OrderVenue, PaperVenue};
use super::{Feed, StrategyManager, StrategyRisk};

const META_RISK: &str = "strategy_risk";
const META_PEAK: &str = "strategy_portfolio_peak";
const META_TRIPPED: &str = "strategy_portfolio_tripped_day";
const EQUITY_KEEP_MINUTE_MS: u64 = 7 * 86_400_000;
const FUNDING_EVERY_MS: u64 = 8 * 3_600_000;

pub fn now_ms() -> u64 {
    crate::bot::engine::now_ms()
}

fn dir(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok()
}

/// yyyymmdd of a UTC day start.
fn yyyymmdd(day_ms: u64) -> String {
    // civil-from-days (H. Hinnant)
    let z = (day_ms / 86_400_000) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}{m:02}{d:02}")
}

fn day_key(day: u64) -> String {
    format!("strategy_equity_day.{}", yyyymmdd(day))
}

fn note_all(mgr: &StrategyManager, bots: &[StrategyBot], key: &str, now: u64) {
    for b in bots {
        mgr.note(&b.id, &b.cfg.symbol, key, None, now);
    }
}

/// Reloads bots, open cycles, venues, cursors and the risk state. Every bot
/// comes back Stopped unless it holds an open cycle (managed, not re-armed).
pub fn restore(app: &AppHandle) {
    let mgr = app.state::<StrategyManager>();
    let Some(dir) = dir(app) else {
        return;
    };
    let store = app.state::<StoreManager>();
    let loaded = store.strategy(&dir, |c| {
        let bots = db::load_bots(c)?;
        let cycles = db::load_open_cycles(c)?;
        let mut venues = Vec::new();
        for b in &bots {
            venues.push((b.id.clone(), PaperVenue::from_open(db::load_open_orders(c, &b.id)?)));
        }
        let mut cursors = Vec::new();
        for b in &bots {
            if let Some((ms, px)) = db::get_cursor(c, &b.cfg.exchange_id, b.cfg.market.as_str(), &b.cfg.symbol)? {
                cursors.push((
                    (b.cfg.market, b.cfg.symbol.clone()),
                    Feed {
                        last_close_ms: ms,
                        last_close: px,
                        last_ok_ms: 0,
                    },
                ));
            }
        }
        Ok((bots, cycles, venues, cursors))
    });
    match loaded {
        Ok((bots, cycles, venues, cursors)) => {
            // Persist the forced Stopped state right away.
            let forced: Vec<StrategyBot> = bots
                .iter()
                .filter(|b| driver::is_running(b.state))
                .cloned()
                .collect();
            mgr.load(bots, cycles, venues, cursors);
            for b in forced {
                if let Some(cur) = mgr.bot(&b.id) {
                    save_bot(app, &cur);
                }
            }
        }
        Err(e) => mgr.note("", "—", "storeReadFailed", Some(e), now_ms()),
    }
    let meta = |k: &str| store.get_meta(&dir, k).ok().flatten();
    let risk = meta(META_RISK)
        .and_then(|v| serde_json::from_str::<StrategyRisk>(&v).ok())
        .unwrap_or_default();
    let tripped = meta(META_TRIPPED).and_then(|v| v.parse().ok()).unwrap_or(0);
    let peak = meta(META_PEAK).and_then(|v| v.parse().ok()).unwrap_or(0.0);
    mgr.set_risk(risk, tripped, peak);
    let day = utc_day_start_ms();
    let stored = meta(&day_key(day)).and_then(|v| serde_json::from_str(&v).ok());
    if stored.is_some() {
        mgr.roll_day(day, stored);
    }
    if mgr.open_cycle_count() > 0 {
        app.state::<BotManager>().ensure_loop(app.clone());
    }
}

pub fn save_bot(app: &AppHandle, bot: &StrategyBot) {
    let mgr = app.state::<StrategyManager>();
    let res = dir(app)
        .ok_or_else(|| "app data dir unavailable".to_string())
        .and_then(|d| app.state::<StoreManager>().strategy(&d, |c| db::save_bot(c, bot, now_ms())));
    if let Err(e) = res {
        mgr.note(&bot.id, &bot.cfg.symbol, "storeWriteFailed", Some(e), now_ms());
    }
}

pub fn set_meta(app: &AppHandle, key: &str, value: &str) {
    if let Some(d) = dir(app) {
        let _ = app.state::<StoreManager>().set_meta(&d, key, value);
    }
}

pub fn save_risk(app: &AppHandle, risk: &StrategyRisk) {
    if let Ok(v) = serde_json::to_string(risk) {
        set_meta(app, META_RISK, &v);
    }
}

pub fn save_breaker(app: &AppHandle, mgr: &StrategyManager) {
    set_meta(app, META_PEAK, &mgr.portfolio_peak().to_string());
    set_meta(app, META_TRIPPED, &mgr.tripped_day().to_string());
}

/// Client ids carry the cycle seq: "ae{bot8}-{seq}-{role}".
fn seq_of(client_id: &str) -> u32 {
    client_id.split('-').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// Everything a bot produced, written in one transaction.
struct Batch {
    /// Market orders already filled (manual / kill closes).
    market_orders: Vec<super::model::SimOrder>,
    cycles: Vec<CycleState>,
    fills: Vec<(u32, Fill)>,
    equity: Vec<EquityPoint>,
}

fn persist(app: &AppHandle, bot: &StrategyBot, venue: &mut PaperVenue, open: Option<&CycleState>, batch: Batch) {
    let filled: HashSet<String> = batch
        .fills
        .iter()
        .filter(|(_, f)| f.kind == super::model::FillKind::Fill)
        .map(|(_, f)| f.client_id.clone())
        .collect();
    let desired = open.map(|c| c.open_orders()).unwrap_or_default();
    let orders: Vec<(u32, super::model::SimOrder)> = venue
        .sync(desired, &filled)
        .into_iter()
        .chain(batch.market_orders)
        .map(|o| (seq_of(&o.client_id), o))
        .collect();
    let mut cycles: Vec<&CycleState> = batch.cycles.iter().collect();
    if let Some(c) = open {
        cycles.push(c);
    }
    let w = db::TickWrite {
        bot,
        cycles,
        orders,
        fills: batch.fills,
        equity: batch.equity,
    };
    let res = dir(app)
        .ok_or_else(|| "app data dir unavailable".to_string())
        .and_then(|d| app.state::<StoreManager>().strategy(&d, |c| db::persist_tick(c, &w, now_ms())));
    if let Err(e) = res {
        app.state::<StrategyManager>()
            .note(&bot.id, &bot.cfg.symbol, "storeWriteFailed", Some(e), now_ms());
    }
}

/// Global reasons new cycles are held this tick.
struct Gates {
    /// The portfolio breaker is tripped (holds only the bots inside it).
    portfolio: bool,
    membership_inactive: bool,
    btc_break: bool,
}

impl Gates {
    fn read(app: &AppHandle, mgr: &StrategyManager) -> Self {
        let bots = app.state::<BotManager>();
        Self {
            portfolio: mgr.portfolio_tripped(),
            membership_inactive: !app.state::<MembershipManager>().view().active,
            btc_break: bots.btc_regime() == BtcRegime::Break,
        }
    }

    fn for_bot(&self, bot: &StrategyBot) -> Option<&'static str> {
        if self.portfolio && bot.cfg.portfolio_breaker {
            Some("portfolioDdTripped")
        } else if self.membership_inactive {
            Some("membershipInactive")
        } else if self.btc_break && bot.cfg.pause_on_btc_break {
            Some("btcBreak")
        } else {
            None
        }
    }
}

fn to_bar(k: &Kline) -> Bar {
    Bar {
        open_ms: k.open_ms,
        close_ms: k.close_ms,
        o: k.o,
        h: k.h,
        l: k.l,
        c: k.c,
        funding_rate: None,
        funding_unknown: false,
    }
}

/// Attaches funding events to the bars containing them. A failed read marks
/// the standard 8h funding bars unknown (charged 0, flagged).
async fn attach_funding(mgr: &StrategyManager, symbol: &str, bars: &mut [Bar]) {
    let (Some(first), Some(last)) = (bars.first(), bars.last()) else {
        return;
    };
    if !bars.iter().any(|b| b.open_ms % 3_600_000 == 0) {
        return;
    }
    let (start, end) = (first.open_ms, last.close_ms);
    match klines::fetch_funding(&mgr.client, symbol, start, end).await {
        Ok(events) => {
            for (ts, rate) in events {
                if let Some(b) = bars.iter_mut().find(|b| b.open_ms <= ts && ts <= b.close_ms) {
                    b.funding_rate = Some(b.funding_rate.unwrap_or(0.0) + rate);
                }
            }
            // Every funding schedule (8h, 4h, 1h) settles at 00/08/16 UTC. A
            // read that came back without that event (history not yet
            // published) is not "no funding": flag it, never charge 0 silently.
            for b in bars.iter_mut().filter(|b| b.open_ms % FUNDING_EVERY_MS == 0 && b.funding_rate.is_none()) {
                b.funding_unknown = true;
            }
        }
        Err(_) => {
            for b in bars.iter_mut().filter(|b| b.open_ms % FUNDING_EVERY_MS == 0) {
                b.funding_unknown = true;
            }
        }
    }
}

/// One engine pass over every strategy bot.
pub async fn strategy_phase(app: &AppHandle) {
    let mgr = app.state::<StrategyManager>();
    if !mgr.any_active() {
        return;
    }
    // `op_lock` is taken per group around the walk (see `run_group`) and for
    // the breaker, never across the network reads: a start / stop / close
    // must not wait on a slow kline request.
    let now = now_ms();
    let today = utc_day_start_ms();
    roll_day(app, &mgr, today);

    // Active bots grouped by feed.
    let mut groups: BTreeMap<(MarketKind, String), Vec<StrategyBot>> = BTreeMap::new();
    for b in mgr.bots() {
        if driver::is_running(b.state) || mgr.has_open_cycle(&b.id) {
            groups
                .entry((b.cfg.market, b.cfg.symbol.clone()))
                .or_default()
                .push(b);
        }
    }
    for ((market, symbol), bots) in groups {
        run_group(app, &mgr, market, &symbol, &bots, now).await;
    }

    // Portfolio breaker on bar-close MTM, over the bots inside it only.
    let _guard = mgr.op_lock.lock().await;
    retry_pending_closes(app, &mgr).await;
    let peak_before = mgr.portfolio_peak();
    if !mgr.portfolio_tripped() && mgr.breaker_check(mgr.breaker_pnl(), mgr.breaker_budget()) {
        mgr.trip_breaker(today);
        close_breaker_bots_locked(app, &mgr).await;
        mgr.note("", "—", "portfolioDdTripped", None, now);
        save_breaker(app, &mgr);
    } else if mgr.portfolio_peak() != peak_before {
        set_meta(app, META_PEAK, &mgr.portfolio_peak().to_string());
    }

    if mgr.compact_due(now) {
        if let Some(d) = dir(app) {
            let _ = app
                .state::<StoreManager>()
                .strategy(&d, |c| db::compact_equity(c, now.saturating_sub(EQUITY_KEEP_MINUTE_MS)));
        }
    }
}

/// Installs the day's equity snapshot (kill-switch day P&L base).
fn roll_day(app: &AppHandle, mgr: &StrategyManager, today: u64) {
    if mgr.day_snapshot_day() == today {
        return;
    }
    let store = app.state::<StoreManager>();
    let stored = dir(app)
        .and_then(|d| store.get_meta(&d, &day_key(today)).ok().flatten())
        .and_then(|v| serde_json::from_str::<HashMap<BotId, f64>>(&v).ok());
    if let Some(snap) = mgr.roll_day(today, stored) {
        if let Ok(v) = serde_json::to_string(&snap) {
            set_meta(app, &day_key(today), &v);
        }
    }
}

async fn run_group(
    app: &AppHandle,
    mgr: &StrategyManager,
    market: MarketKind,
    symbol: &str,
    bots: &[StrategyBot],
    now: u64,
) {
    let mut feed = mgr.feed(market, symbol).unwrap_or_default();
    let seen_until = feed.last_close_ms;
    let any_open = bots.iter().any(|b| mgr.has_open_cycle(&b.id));
    // The oldest bar an open cycle has walked. The cursor is written after
    // the bots' own transactions; when a bot write failed (or the cursor row
    // is missing) the cycle is behind it and must replay from its own bar,
    // never jump the gap.
    let resume = bots
        .iter()
        .filter_map(|b| mgr.cycle_last_close_ms(&b.id))
        .filter(|ms| *ms > 0)
        .min();
    // A cursor is replayed only when a cycle depends on it; otherwise a
    // fresh start reads from the current minute (no needless backlog).
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
        return;
    }
    let futures = market == MarketKind::Futures;
    let fetch = async {
        let fetched = klines::fetch_closed_klines(&mgr.client, futures, symbol, Some(start_ms), now).await;
        let mut bars: Vec<Bar> = match fetched {
            Ok(k) => {
                feed.last_ok_ms = now;
                k.iter()
                    .filter(|k| k.open_ms >= start_ms)
                    .map(to_bar)
                    .collect()
            }
            Err(_) => Vec::new(),
        };
        if bars.is_empty() {
            let stale = feed.last_close_ms == 0 || now.saturating_sub(feed.last_close_ms) > PRICE_STALE_MS;
            if stale && now.saturating_sub(feed.last_ok_ms.max(feed.last_close_ms)) > PRICE_STALE_MS {
                for b in bots {
                    mgr.set_paused(&b.id, Some("priceStale"));
                }
                note_all(mgr, bots, "priceStale", now);
            }
            mgr.set_feed(market, symbol, feed);
            return None;
        }
        if futures {
            attach_funding(mgr, symbol, &mut bars).await;
        }
        Some(bars)
    };
    // Commands that move bots or cycles hold the same lock; the walk takes
    // each bot out and puts it back, which must not interleave with them.
    // The gates are read under it, after the fetch (decision time).
    let Some((bars, gates, _guard)) = fetch_then_gate(&mgr.op_lock, fetch, || Gates::read(app, mgr)).await else {
        return;
    };
    for b in bots {
        mgr.set_paused(&b.id, None);
        walk_bot(app, mgr, &b.id, (market, symbol), &bars, &gates, now, seen_until);
    }
    let last = *bars.last().expect("non-empty");
    feed.last_close_ms = last.close_ms;
    feed.last_close = last.c;
    mgr.set_feed(market, symbol, feed);
    if let Some(d) = dir(app) {
        let exchange = bots.first().map(|b| b.cfg.exchange_id.clone()).unwrap_or_else(|| "binance".into());
        let _ = app.state::<StoreManager>().strategy(&d, |c| {
            db::set_cursor(c, &exchange, market.as_str(), symbol, last.close_ms, last.c)
        });
    }
}

/// Whether `bot` still trades the feed whose bars are being walked. The
/// group is built before the network read and `op_lock` is free during it:
/// a stop, symbol edit and start in that window would otherwise walk the bot
/// through another symbol's bars (a cycle opened at a foreign price).
fn on_feed(bot: &StrategyBot, feed: (MarketKind, &str)) -> bool {
    bot.cfg.market == feed.0 && bot.cfg.symbol == feed.1
}

/// `seen_until`: the group cursor before this tick. A bot without an open
/// cycle has already been offered every bar up to it (a replay for a lagging
/// cycle must not hand those bars to it again); see `walk_bars`.
#[allow(clippy::too_many_arguments)]
fn walk_bot(
    app: &AppHandle,
    mgr: &StrategyManager,
    id: &str,
    feed: (MarketKind, &str),
    bars: &[Bar],
    gates: &Gates,
    now: u64,
    seen_until: u64,
) {
    if !mgr.bot(id).is_some_and(|b| on_feed(&b, feed)) {
        return;
    }
    let Some((mut bot, mut cycle, mut venue)) = mgr.take_state(id) else {
        return;
    };
    let gate = gates.for_bot(&bot);
    let (batch, notes, opened) = walk_bars(&mut bot, &mut cycle, bars, gate, seen_until);
    if opened {
        mgr.clear_last_note(&bot.id);
    }
    let symbol = bot.cfg.symbol.clone();
    let wrote = !batch.fills.is_empty() || !batch.cycles.is_empty() || !batch.equity.is_empty();
    if wrote || cycle.is_some() {
        persist(app, &bot, &mut venue, cycle.as_ref(), batch);
    } else {
        save_bot(app, &bot);
    }
    mgr.put_state(bot, cycle, venue);
    for (key, detail) in notes {
        mgr.note(id, &symbol, key, detail, now);
    }
}

type Notes = Vec<(&'static str, Option<String>)>;

/// Pure bar walk of one bot (no store, no Tauri). Every bar after the bot's
/// own position is offered exactly once: a bot with an open cycle resumes
/// after the cycle's last walked bar, an idle bot after `seen_until`. When
/// a lagging cycle closes partway through a replay, the bars after the close
/// (up to the old cursor) were never offered to this bot: they are walked
/// for a new start, utilisation and equity like any other bar.
/// Returns (batch, notes, whether a cycle opened).
fn walk_bars(
    bot: &mut StrategyBot,
    cycle: &mut Option<CycleState>,
    bars: &[Bar],
    gate: Option<&'static str>,
    seen_until: u64,
) -> (Batch, Notes, bool) {
    let mut batch = Batch {
        market_orders: Vec::new(),
        cycles: Vec::new(),
        fills: Vec::new(),
        equity: Vec::new(),
    };
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

/// Waits for the network read, then takes `op_lock` and only then reads the
/// gates: a re-arm, membership change or BTC regime flip that lands while
/// the bars are in flight takes effect on this tick, not the next one.
/// `None` from `fetch` (no bars) returns without taking the lock.
async fn fetch_then_gate<'a, T, G>(
    lock: &'a tokio::sync::Mutex<()>,
    fetch: impl std::future::Future<Output = Option<T>>,
    read_gates: impl FnOnce() -> G,
) -> Option<(T, G, tokio::sync::MutexGuard<'a, ()>)> {
    let fetched = fetch.await?;
    let guard = lock.lock().await;
    let gates = read_gates();
    Some((fetched, gates, guard))
}

/// Latest ticker price on the bot's market; falls back to the last closed
/// bar only while that bar is fresh. None = no honest price.
async fn close_price(app: &AppHandle, mgr: &StrategyManager, bot: &StrategyBot) -> Option<f64> {
    let status = app
        .state::<ExchangeManager>()
        .status(&bot.cfg.exchange_id, &bot.cfg.symbol)
        .await;
    let ticker = match bot.cfg.market {
        MarketKind::Futures => status.futures_price,
        MarketKind::Spot => status.spot_price,
    };
    ticker.or_else(|| {
        mgr.feed(bot.cfg.market, &bot.cfg.symbol)
            .filter(|f| now_ms().saturating_sub(f.last_close_ms) <= PRICE_STALE_MS && f.last_close > 0.0)
            .map(|f| f.last_close)
    })
}

/// Closes one bot's open cycle at market (caller holds `op_lock`). Ok(false)
/// when it had no open cycle; Err("priceUnavailable") when no price exists.
pub async fn close_one_locked(app: &AppHandle, mgr: &StrategyManager, id: &str, reason: ExitReason, at_mark: bool) -> Result<bool, String> {
    let Some(bot) = mgr.bot(id) else {
        return Err("botUnknown".into());
    };
    if !mgr.has_open_cycle(id) {
        mgr.clear_pending_close(id);
        return Ok(false);
    }
    let price = if at_mark {
        mgr.feed(bot.cfg.market, &bot.cfg.symbol).map(|f| f.last_close).filter(|p| *p > 0.0)
    } else {
        close_price(app, mgr, &bot).await
    };
    let Some(price) = price else {
        mgr.note(id, &bot.cfg.symbol, "priceUnavailable", None, now_ms());
        return Err("priceUnavailable".into());
    };
    let Some((mut bot, mut cycle, mut venue)) = mgr.take_state(id) else {
        return Err("botUnknown".into());
    };
    let now = now_ms();
    let Some((closed, fills)) = driver::force_close(&mut bot, &mut cycle, price, now, reason) else {
        mgr.put_state(bot, cycle, venue);
        return Ok(false);
    };
    let seq = closed.core.seq;
    let eq = super::accounting::equity(bot.cfg.budget, bot.realized_quote, 0.0);
    let market_orders = fills
        .iter()
        .filter_map(|f| {
            venue
                .place_market(super::model::SimOrder {
                    client_id: f.client_id.clone(),
                    role: super::model::OrderRole::Close,
                    side: if f.qty > 0.0 {
                        super::model::OrderSide::Buy
                    } else {
                        super::model::OrderSide::Sell
                    },
                    kind: super::model::OrderType::Market,
                    price: f.price,
                    qty: f.qty.abs(),
                    active_from_leg: f.leg,
                    state: super::model::OrderState::Planned,
                })
                .ok()
        })
        .collect();
    persist(
        app,
        &bot,
        &mut venue,
        None,
        Batch {
            market_orders,
            cycles: vec![closed],
            fills: fills.into_iter().map(|f| (seq, f)).collect(),
            equity: vec![EquityPoint {
                ts: now,
                equity: eq,
                margin: 0.0,
                reserved: 0.0,
            }],
        },
    );
    let symbol = bot.cfg.symbol.clone();
    mgr.put_state(bot, cycle, venue);
    mgr.clear_pending_close(id);
    mgr.note(id, &symbol, "cycleClosed", Some(reason.as_str().to_string()), now);
    Ok(true)
}

/// Stops every strategy bot and closes every open paper cycle (caller holds
/// `op_lock`). Kill switch / remote kill / "close all": the portfolio-breaker
/// flag does not exempt a bot. Returns (closed, unpriced).
async fn close_all_locked(app: &AppHandle, mgr: &StrategyManager, reason: ExitReason, at_mark: bool) -> (u32, u32) {
    for b in mgr.stop_all() {
        save_bot(app, &b);
    }
    let ids: Vec<BotId> = mgr.bots().into_iter().map(|b| b.id).collect();
    close_ids_locked(app, mgr, &ids, reason, at_mark).await
}

/// Portfolio-breaker trip: stops and closes (at the bar-close mark) only the
/// bots inside the breaker; a bot switched out of it runs on untouched.
async fn close_breaker_bots_locked(app: &AppHandle, mgr: &StrategyManager) -> (u32, u32) {
    for b in mgr.stop_breaker_bots() {
        save_bot(app, &b);
    }
    let ids = mgr.breaker_bot_ids();
    close_ids_locked(app, mgr, &ids, ExitReason::PortfolioDd, true).await
}

async fn close_ids_locked(app: &AppHandle, mgr: &StrategyManager, ids: &[BotId], reason: ExitReason, at_mark: bool) -> (u32, u32) {
    let (mut closed, mut unpriced) = (0, 0);
    for id in ids {
        match close_one_locked(app, mgr, id, reason, at_mark).await {
            Ok(true) => closed += 1,
            Ok(false) => {}
            Err(_) => {
                // Owed: retried every tick (`retry_pending_closes`) until a
                // price exists, instead of being managed on as if never asked.
                mgr.mark_pending_close(id, reason);
                unpriced += 1;
            }
        }
    }
    (closed, unpriced)
}

/// Retries forced closes that found no price (caller holds `op_lock`).
async fn retry_pending_closes(app: &AppHandle, mgr: &StrategyManager) {
    for (id, reason) in mgr.pending_closes() {
        let _ = close_one_locked(app, mgr, &id, reason, false).await;
    }
}

/// Kill switch / remote kill / UI "close all": never touches a real order.
pub async fn force_close_all(app: &AppHandle, reason: &str) -> (u32, u32) {
    let mgr = app.state::<StrategyManager>();
    let _guard = mgr.op_lock.lock().await;
    let reason = match reason {
        "dailyStop" => ExitReason::DailyStop,
        "remoteKill" => ExitReason::RemoteKill,
        "portfolioDd" => ExitReason::PortfolioDd,
        _ => ExitReason::Manual,
    };
    close_all_locked(app, &mgr, reason, false).await
}

/// Stops every strategy bot (new cycles only). Under `op_lock`, so a stop
/// arriving during a bar walk (remote stop, daily stop) is not overwritten
/// when the walk puts the bot back.
pub async fn stop_all(app: &AppHandle) {
    let mgr = app.state::<StrategyManager>();
    let _guard = mgr.op_lock.lock().await;
    for b in mgr.stop_all() {
        save_bot(app, &b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yyyymmdd_from_day_start() {
        assert_eq!(yyyymmdd(0), "19700101");
        assert_eq!(yyyymmdd(1_759_276_800_000), "20251001");
        assert_eq!(yyyymmdd(1_709_164_800_000), "20240229");
    }

    #[test]
    fn a_bot_moved_to_another_symbol_is_not_walked_on_the_old_feed() {
        let mut b = crate::bot::strategy::driver::tests::sample_bot();
        assert!(on_feed(&b, (MarketKind::Futures, "SYNUSDT")));
        b.cfg.symbol = "ETHUSDT".into();
        assert!(!on_feed(&b, (MarketKind::Futures, "SYNUSDT")));
        b.cfg.symbol = "SYNUSDT".into();
        b.cfg.market = MarketKind::Spot;
        assert!(!on_feed(&b, (MarketKind::Futures, "SYNUSDT")));
    }

    #[test]
    fn gates_are_read_after_the_fetch_under_the_lock() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let lock = tokio::sync::Mutex::new(());
        // e.g. the BTC regime leaves Break (or a breaker re-arm) while the
        // kline request is in flight
        let gate_open = AtomicBool::new(false);
        let got = tauri::async_runtime::block_on(async {
            let fetch = async {
                gate_open.store(true, Ordering::SeqCst);
                Some(vec![1u8])
            };
            let r = fetch_then_gate(&lock, fetch, || gate_open.load(Ordering::SeqCst)).await;
            let (bars, gates, guard) = r.expect("bars");
            assert!(lock.try_lock().is_err(), "op_lock held for the walk");
            drop(guard);
            (bars, gates)
        });
        assert_eq!(got.0, vec![1u8]);
        assert!(got.1, "the gate change during the fetch is seen on this tick");
        // no bars: the lock is not taken, the gates are not read
        let read = AtomicBool::new(false);
        let none = tauri::async_runtime::block_on(fetch_then_gate(&lock, async { None::<()> }, || {
            read.store(true, Ordering::SeqCst);
        }));
        assert!(none.is_none() && !read.load(Ordering::SeqCst));
        assert!(lock.try_lock().is_ok());
    }

    #[test]
    fn bars_after_a_lagging_cycle_closes_are_offered_once() {
        use crate::bot::strategy::driver::tests::{sample_bot, wave};
        let bars = wave();
        // Reference: one uninterrupted walk.
        let mut rb = sample_bot();
        let mut rc = None;
        let mut ref_eq = Vec::new();
        let mut ref_fills = Vec::new();
        let mut first_close = None;
        for (i, b) in bars.iter().enumerate() {
            let out = driver::on_bar(&mut rb, &mut rc, b, None);
            if i >= 1 {
                ref_eq.extend(out.equity);
                ref_fills.extend(out.events.into_iter().map(|(s, f)| (s, f.client_id)));
            }
            if out.closed.is_some() && first_close.is_none() {
                first_close = Some(i);
            }
        }
        let close_i = first_close.expect("wave closes a cycle");
        assert!(close_i + 60 < bars.len());
        // The bot's cycle walked bar 0 only (its write failed); the group
        // cursor already sits 50 bars past the cycle's close.
        let mut bot = sample_bot();
        let mut cyc = None;
        driver::on_bar(&mut bot, &mut cyc, &bars[0], None);
        assert!(cyc.is_some());
        let seen_until = bars[close_i + 50].close_ms;
        let (batch, _, _) = walk_bars(&mut bot, &mut cyc, &bars[1..], None, seen_until);
        let fills: Vec<(u32, String)> = batch.fills.into_iter().map(|(s, f)| (s, f.client_id)).collect();
        assert_eq!(fills, ref_fills, "a new cycle can start right after the close");
        assert_eq!(batch.equity, ref_eq, "every bar after the close has its equity point");
        assert_eq!(bot.util.bars, rb.util.bars, "every bar counts towards utilisation");
        assert_eq!(bot.cycles_done, rb.cycles_done);
        assert_eq!(bot.realized_quote, rb.realized_quote);
        // A bot without a cycle is not handed bars it was already offered.
        let mut idle = sample_bot();
        let mut none = None;
        let (b2, _, opened) = walk_bars(&mut idle, &mut none, &bars[..10], None, bars[9].close_ms);
        assert!(!opened && b2.fills.is_empty() && b2.equity.is_empty());
    }

    #[test]
    fn seq_is_parsed_from_client_ids() {
        assert_eq!(seq_of("aeabcdefgh-12-tp3"), 12);
        assert_eq!(seq_of("garbage"), 0);
    }
}
