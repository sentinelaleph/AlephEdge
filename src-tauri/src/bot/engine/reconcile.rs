//! Reconciliation: the exchange is the truth for LIVE positions.
//!
//! A stop or take-profit that fires on Binance while the app is closed used
//! to be invisible: the book kept the position until its OWN price condition
//! triggered, then recorded an exit that never happened at that price (or
//! never happened at all, if price came back). At startup and every 60s this
//! reads the account; a live book position whose symbol is flat on the
//! exchange is settled on real fills with the reason taken from which
//! protective order fired.
//!
//! Exposure the book does not know is NEVER touched when it may be the
//! user's own manual trade: it is only surfaced. The one exception is
//! exposure provably opened by this app's own entry (its client id, see
//! `attribute_position`): that is adopted with its stop, or flattened.

use std::sync::atomic::{AtomicU64, Ordering};

use tauri::{AppHandle, Manager};

use crate::exchange::providers::binance_parse::{OrderFill, RecentOrder};
use crate::exchange::providers::binance_requests::{entry_client_id, ENTRY_CLIENT_PREFIX};
use crate::exchange::{venue_order_client_id, ExchangeManager};
use crate::risk::RiskManager;
use crate::signal::model::{Direction, Signal};
use crate::signal::SignalManager;
use crate::vault::VaultManager;

use super::super::model::{BotConfig, BotKind, OpenPosition, SizingMode};
use super::super::BotManager;
use super::entry_rules::new_position;
use super::live::{
    self, exchange_leverage, live_trading_allowed, stop_beyond_liquidation, Protection,
    EXIT_EXCHANGE_CLOSED, NOTE_DIRECTION_MISMATCH, NOTE_RECONCILE_FAILED, NOTE_UNPROTECTED,
    NOTE_UNTRACKED, SKIP_VAULT_LOCKED,
};
use super::sizing::{self, Size};
use super::{book, geometry, live_close, live_price, now_ms, precheck};

pub const RECONCILE_INTERVAL_MS: u64 = 60_000;
/// An entry of ours that filled after it was finalized as failed: now in
/// the book with its exchange stop.
pub const NOTE_OWN_ENTRY_ADOPTED: &str = "liveOwnEntryAdopted";
/// An entry of ours that filled after it was finalized as failed and could
/// not be protected as its signal planned: flattened.
pub const NOTE_OWN_ENTRY_FLATTENED: &str = "liveOwnEntryFlattened";
/// 0 = never ran, so the first tick after startup reconciles immediately.
static LAST_RECONCILE_MS: AtomicU64 = AtomicU64::new(0);

/// The exchange's side of one book position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeSide {
    /// Holds exposure in the book's direction.
    Open,
    /// Nothing on the symbol: the position closed on the exchange.
    Flat,
    /// Exposure in the OPPOSITE direction — not ours to touch.
    Mismatch,
}

/// `amt` is Binance's signed positionAmt (long > 0, short < 0; None = no row).
pub fn exchange_state(long: bool, amt: Option<f64>) -> ExchangeSide {
    match amt {
        None => ExchangeSide::Flat,
        Some(0.0) => ExchangeSide::Flat,
        Some(a) if (a > 0.0) == long => ExchangeSide::Open,
        Some(_) => ExchangeSide::Mismatch,
    }
}

/// Which exit the exchange made. A fired TP is "tp"; a fired stop is
/// "breakeven" once it had been moved to entry, "sl" otherwise; flat with
/// neither fired (manual close, liquidation, a cancelled order) is
/// "exchangeClosed" — never guessed as a ledger exit.
pub fn infer_exit_reason(
    stop_fired: bool,
    tp_fired: bool,
    stop_at_breakeven: bool,
) -> &'static str {
    match (tp_fired, stop_fired, stop_at_breakeven) {
        (true, _, _) => "tp",
        (false, true, true) => "breakeven",
        (false, true, false) => "sl",
        _ => EXIT_EXCHANGE_CLOSED,
    }
}

/// The trigger level standing in for the exit when fills are unavailable.
pub fn reference_exit(pos: &OpenPosition, reason: &str) -> Option<f64> {
    match reason {
        "tp" => Some(pos.tp),
        "sl" => Some(pos.sl),
        "breakeven" => Some(pos.entry),
        _ => None,
    }
}

/// What one account snapshot means for the book.
#[derive(Debug, Default, PartialEq)]
pub struct ReconcilePlan {
    /// Signal ids of live positions that are flat on the exchange.
    pub closed: Vec<String>,
    /// Symbols held in the opposite direction.
    pub mismatched: Vec<String>,
    /// Exchange symbols no live book position accounts for.
    pub untracked: Vec<String>,
}

/// `exchange` = (symbol, signed positionAmt) for every non-zero position.
///
/// A plan is not an order: this reads `live` as the book records it, and the
/// master-switch gate (`is_live()`) sits on the paths that actually send —
/// `reconcile_phase` below, and the close/manage branches in book.rs.
pub fn plan_reconcile(book: &[OpenPosition], exchange: &[(String, f64)]) -> ReconcilePlan {
    let mut plan = ReconcilePlan::default();
    for pos in book.iter().filter(|p| p.live) {
        let amt = exchange
            .iter()
            .find(|(s, _)| *s == pos.symbol)
            .map(|(_, a)| *a);
        match exchange_state(pos.direction == "long", amt) {
            ExchangeSide::Open => {}
            ExchangeSide::Flat => plan.closed.push(pos.signal_id.clone()),
            ExchangeSide::Mismatch => plan.mismatched.push(pos.symbol.clone()),
        }
    }
    plan.untracked = exchange
        .iter()
        .filter(|(s, a)| *a != 0.0 && !book.iter().any(|p| p.live && p.symbol == *s))
        .map(|(s, _)| s.clone())
        .collect();
    plan
}

/// `plan_reconcile` for ONE venue's account snapshot: only the book
/// positions that sit on `venue` are judged. A position is never marked flat
/// from another venue's account (a Binance position is absent from every
/// Bybit snapshot, and was settled as closed while still open).
pub fn plan_reconcile_on(venue: &str, book: &[OpenPosition], exchange: &[(String, f64)]) -> ReconcilePlan {
    let mine: Vec<OpenPosition> = book.iter().filter(|p| p.exchange_id == venue).cloned().collect();
    plan_reconcile(&mine, exchange)
}

/// The venues one reconcile pass reads: every venue a live book position
/// sits on, plus the live bot's own venue (lost entries of its own may sit
/// there), in that order, each once.
pub fn reconcile_venues(bot_venue: Option<&str>, book: &[OpenPosition]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let held = book.iter().filter(|p| p.live).map(|p| p.exchange_id.as_str());
    for v in held.chain(bot_venue) {
        if !out.iter().any(|o| o == v) {
            out.push(v.to_string());
        }
    }
    out
}

/// The flat positions of `plan` this pass may settle, each under the close
/// claim (the caller releases it). One already claimed — a veto or a remote
/// kill closing it right now — is left to that close: settling it here too
/// recorded the same real round trip twice.
pub fn claim_flat(
    bots: &BotManager,
    positions: &[OpenPosition],
    plan: &ReconcilePlan,
) -> Vec<OpenPosition> {
    positions
        .iter()
        .filter(|p| plan.closed.contains(&p.signal_id))
        .filter(|p| bots.claim_open(&p.signal_id, p.bot_kind))
        .cloned()
        .collect()
}

// ---- Untracked exchange positions: ours or the user's? ----
//
// An entry whose outcome stayed unknown is finalized as failed when Binance
// cannot show the order yet; if it then fills, the position is real, carries
// no stop, and no book row. Every entry this app sends carries a
// deterministic client id (`entry_client_id`: "ae" + the signal id), so the
// symbol's order history tells our own entries from the user's trades.

/// This app's entry that opened an exchange position the book does not hold.
#[derive(Debug, Clone, PartialEq)]
pub struct OwnEntry {
    pub client_id: String,
    pub order_id: i64,
    pub avg_price: f64,
    pub qty: f64,
    /// The order's updateTime (ms); 0 when Binance did not send it.
    pub filled_at: u64,
}

/// Whether `cid` is one of this app's entry client ids: the "ae" prefix and
/// either the id of a signal still in the buffer or the shape every Sentinel
/// id takes (a UUID: 32 lowercase hex characters once the dashes go). The
/// ccxt venues store the id cut to 32 characters (`venue_client_id`), which
/// leaves 30 of the UUID's hex characters.
pub fn is_own_client_id(cid: &str, known: &[String]) -> bool {
    let Some(body) = cid.strip_prefix(ENTRY_CLIENT_PREFIX) else {
        return false;
    };
    let uuid_shape = matches!(body.len(), 30 | 32)
        && body
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
    uuid_shape || known.iter().any(|k| k == cid)
}

/// The client id the entry for `signal_id` carries in `exchange_id`'s order
/// history: what `open_live` sent, as that venue stores it.
pub fn own_entry_id(exchange_id: &str, signal_id: &str) -> String {
    venue_order_client_id(exchange_id, &entry_client_id(signal_id))
}

/// Attributes an untracked position (`amt` = signed positionAmt) to our own
/// entry. The symbol's LATEST filled order must be an opening order of ours
/// on the position's side, for exactly the position's size: any later fill
/// (the user adding, reducing or flipping) makes the position not purely
/// ours, and it is then reported, never touched.
pub fn attribute_position(amt: f64, orders: &[RecentOrder], known: &[String]) -> Option<OwnEntry> {
    if !amt.is_finite() || amt == 0.0 {
        return None;
    }
    let long = amt > 0.0;
    let latest = orders
        .iter()
        .filter(|o| o.executed_qty > 0.0)
        .max_by_key(|o| (o.update_time, o.order_id))?;
    let opening = !latest.reducing && latest.buy == long;
    let size_matches = (latest.executed_qty - amt.abs()).abs() <= 1e-9 * amt.abs().max(1.0);
    let ours = is_own_client_id(&latest.client_order_id, known);
    (opening && size_matches && ours && latest.avg_price > 0.0).then(|| OwnEntry {
        client_id: latest.client_order_id.clone(),
        order_id: latest.order_id,
        avg_price: latest.avg_price,
        qty: latest.executed_qty,
        filled_at: latest.update_time,
    })
}

/// The buffered signal an own entry was sent for, on `exchange_id`.
pub fn signal_for<'a>(own: &OwnEntry, symbol: &str, signals: &'a [Signal], exchange_id: &str) -> Option<&'a Signal> {
    signals
        .iter()
        .find(|s| s.symbol == symbol && own_entry_id(exchange_id, &s.id) == own.client_id)
}

/// What reconcile does with one untracked exchange position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UntrackedAction {
    /// Not ours (or not provably ours): surfaced, never touched.
    Report,
    /// Ours, and its signal can still protect it: place the stop through
    /// the normal entry's protection path and put it in the book.
    Adopt,
    /// Ours, but it cannot be protected as its signal planned: flatten.
    Flatten,
}

/// The decision for one untracked position. `bot_can_hold` = a live-allowed
/// futures bot is running and the kill switch is not tripped; `price` the
/// current futures price (None = unknown); `leverage` the exchange leverage
/// the entry was sent at.
pub fn untracked_action(
    amt: f64,
    own: Option<&OwnEntry>,
    sig: Option<&Signal>,
    bot_can_hold: bool,
    price: Option<f64>,
    leverage: u8,
) -> UntrackedAction {
    let Some(own) = own else {
        return UntrackedAction::Report;
    };
    let Some(sig) = sig else {
        return UntrackedAction::Flatten;
    };
    let long = amt > 0.0;
    let same_side = (sig.direction == Direction::Long) == long;
    let stop_crossed = price.is_some_and(|p| if long { p <= sig.sl } else { p >= sig.sl });
    let stop_frac = (own.avg_price - sig.sl).abs() / own.avg_price;
    let protectable = bot_can_hold
        && same_side
        && geometry::geometry_coherent(sig)
        && !stop_crossed
        && !stop_beyond_liquidation(stop_frac, leverage);
    if protectable {
        UntrackedAction::Adopt
    } else {
        UntrackedAction::Flatten
    }
}

/// The book row for an adopted entry: the signal's plan at the real fill,
/// sized by what actually filled, live, with the protection just placed.
pub fn adopted_position(
    cfg: &BotConfig,
    sig: &Signal,
    own: &OwnEntry,
    leverage_cap: u8,
    protection: &Protection,
    now_ms: u64,
) -> OpenPosition {
    let opened_at = if own.filled_at > 0 { own.filled_at } else { now_ms };
    let mut pos = new_position(cfg, sig, own.avg_price, leverage_cap, opened_at, &protection.tp);
    let notional = own.qty * own.avg_price;
    let risk = cfg.sizing == SizingMode::Risk;
    sizing::apply_size(
        &mut pos,
        &Size {
            mode: cfg.sizing,
            risk_pct: risk.then(|| cfg.risk_per_trade_pct.clamp(sizing::MIN_RISK_PCT, sizing::MAX_RISK_PCT)),
            notional,
            qty: own.qty,
            effective_leverage: if cfg.capital > 0.0 { notional / cfg.capital } else { 0.0 },
            risk_capped: false,
        },
    );
    pos.live = true;
    pos.entry_order_id = Some(own.order_id);
    pos.stop_algo_id = protection.stop_algo_id;
    pos.tp_algo_id = protection.tp_algo_id;
    pos.unprotected = protection.stop_algo_id.is_none();
    pos
}

pub fn is_due(last_ms: u64, now_ms: u64) -> bool {
    last_ms == 0 || now_ms.saturating_sub(last_ms) >= RECONCILE_INTERVAL_MS
}

/// Runs only when there is something live to reconcile: a live position, or
/// a bot for which `live_trading_allowed` holds. In this build neither can
/// exist, so the phase never touches the network.
pub async fn reconcile_phase(app: &AppHandle) {
    let bots = app.state::<BotManager>();
    let positions = bots.positions_snapshot();
    let live_bot = bots
        .config_for(BotKind::Futures)
        .is_some_and(|c| live_trading_allowed(&c, BotKind::Futures));
    if !live_bot && !positions.iter().any(|p| p.is_live()) {
        return;
    }
    let now = now_ms();
    if !is_due(LAST_RECONCILE_MS.load(Ordering::Relaxed), now) {
        return;
    }
    // Stamped before the network call: a failing account read retries on
    // the interval, not every 4s tick.
    LAST_RECONCILE_MS.store(now, Ordering::Relaxed);

    // Each live position is reconciled against the account of the venue it
    // sits on; the live bot's own venue is read too (lost entries).
    let bot_venue = bots
        .config_for(BotKind::Futures)
        .filter(|c| live_trading_allowed(c, BotKind::Futures))
        .map(|c| c.exchange_id);
    let live_positions: Vec<OpenPosition> = positions.iter().filter(|p| p.is_live()).cloned().collect();
    for venue in reconcile_venues(bot_venue.as_deref(), &live_positions) {
        reconcile_venue(app, &venue, &live_positions).await;
    }
}

/// One venue's pass: its account against the live book positions on it.
async fn reconcile_venue(app: &AppHandle, venue: &str, positions: &[OpenPosition]) {
    let bots = app.state::<BotManager>();
    let Some(cred) = app.state::<VaultManager>().credential(venue) else {
        for p in positions.iter().filter(|p| p.exchange_id == venue) {
            bots.skip_once(p.bot_kind, &p.signal_id, &p.symbol, SKIP_VAULT_LOCKED, None);
        }
        return;
    };
    let exchange = app.state::<ExchangeManager>();
    let Ok(account) = exchange.futures_account(&cred).await else {
        bots.push_skip("", NOTE_RECONCILE_FAILED, None);
        return;
    };
    let held: Vec<(String, f64)> = account
        .positions
        .iter()
        .map(|p| (p.symbol.clone(), p.position_amt))
        .collect();
    let plan = plan_reconcile_on(venue, positions, &held);

    for pos in claim_flat(&bots, positions, &plan) {
        let price = live_price(&exchange, &pos.exchange_id, &pos.symbol, pos.bot_kind)
            .await
            .unwrap_or(pos.entry);
        let record = live_close::settle_closed(app, &cred, &pos, price).await;
        book::close_with_record(app, &pos, &record);
        bots.closing.release(&pos.signal_id, pos.bot_kind);
    }
    // Deduplicated per symbol: an untouched foreign position must stay
    // visible without flooding the feed every minute.
    for symbol in &plan.mismatched {
        bots.skip_once(
            BotKind::Futures,
            &format!("exchange:{symbol}"),
            symbol,
            NOTE_DIRECTION_MISMATCH,
            None,
        );
    }
    // A live DCA / Grid bot's position is reconciled by its own mirror
    // (bot/strategy_live.rs), not reported here as a stranger's.
    for symbol in plan.untracked.iter().filter(|s| !crate::bot::strategy_live::owns_symbol(app, s)) {
        let amt = held
            .iter()
            .find(|(s, _)| s == symbol)
            .map_or(0.0, |(_, a)| *a);
        handle_untracked(app, &cred, symbol, amt).await;
    }
}

/// Reports an untracked position once per symbol (deduplicated: an untouched
/// foreign position must stay visible without flooding the feed).
fn report_untracked(bots: &BotManager, symbol: &str) {
    bots.skip_once(
        BotKind::Futures,
        &format!("exchange:{symbol}"),
        symbol,
        NOTE_UNTRACKED,
        None,
    );
}

/// One untracked exchange position: attributed by the symbol's order
/// history, then reported (not ours), adopted with its stop, or flattened.
/// An unreadable history is reported only: "cannot tell" is never "ours".
async fn handle_untracked(app: &AppHandle, cred: &crate::vault::model::ExchangeCredential, symbol: &str, amt: f64) {
    let bots = app.state::<BotManager>();
    let exchange = app.state::<ExchangeManager>();
    let signals = app.state::<SignalManager>().recent();
    let known: Vec<String> = signals.iter().map(|s| own_entry_id(&cred.exchange_id, &s.id)).collect();
    let own = match exchange.recent_orders(cred, symbol).await {
        Ok(orders) => attribute_position(amt, &orders, &known),
        Err(_) => None,
    };
    let Some(own) = own else {
        return report_untracked(&bots, symbol);
    };
    // Only the live bot trading THIS venue may hold the adopted position.
    let cfg = bots
        .running_config(BotKind::Futures)
        .filter(|c| live_trading_allowed(c, BotKind::Futures) && c.exchange_id == cred.exchange_id);
    let bot_can_hold = cfg.is_some() && !bots.kill_switch_tripped();
    let cap = cfg.as_ref().map_or(1, |c| {
        precheck::effective_leverage(c, &app.state::<RiskManager>().limits())
    });
    let leverage = cfg
        .as_ref()
        .map_or(cap, |c| exchange_leverage(own.qty * own.avg_price, c.capital, cap));
    let sig = signal_for(&own, symbol, &signals, &cred.exchange_id);
    let price = live_price(&exchange, &cred.exchange_id, symbol, BotKind::Futures).await;
    let action = untracked_action(amt, Some(&own), sig, bot_can_hold, price, leverage);

    if let (UntrackedAction::Adopt, Some(cfg), Some(sig)) = (action, cfg.as_ref(), sig) {
        // The tick is needed to place the stop; without it the position
        // cannot be protected, so it is flattened below.
        if let Ok(rules) = exchange.symbol_rules(cred, symbol).await {
            let fill = OrderFill {
                order_id: own.order_id,
                avg_price: own.avg_price,
                executed_qty: own.qty,
            };
            let target = cfg.take_profit_for(&sig.symbol);
            let reference = price.unwrap_or(own.avg_price);
            match live::protect_fill(
                &exchange, &bots, cred, sig, target, &fill, reference, rules.tick,
            )
            .await
            {
                Ok(protection) => {
                    let pos = adopted_position(cfg, sig, &own, cap, &protection, now_ms());
                    book::open_position(app, pos);
                    bots.judge(BotKind::Futures, &sig.id);
                    bots.push_skip(symbol, NOTE_OWN_ENTRY_ADOPTED, None);
                }
                // The stop failed and the emergency flatten confirmed flat.
                Err(_) => bots.push_skip(symbol, NOTE_OWN_ENTRY_FLATTENED, None),
            }
            return;
        }
    }
    // Ours, but not protectable as its signal planned (or the rules were
    // unreadable): flatten. A flatten that does not confirm is said loudly
    // and retried on the next reconcile pass.
    if live::flatten_confirmed(&exchange, cred, symbol, amt > 0.0, amt.abs()).await {
        bots.push_skip(symbol, NOTE_OWN_ENTRY_FLATTENED, None);
    } else {
        bots.push_skip(symbol, NOTE_UNPROTECTED, None);
    }
}
