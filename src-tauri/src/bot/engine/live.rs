//! Live-execution glue between the bot engine and the exchange write paths:
//! the gate, the i18n keys, and the ENTRY (market order + exchange-side stop
//! and take-profit). Management lives in `live_manage`, closes and real-fill
//! settlement in `live_close`, offline exits in `reconcile`.
//!
//! The decision pipeline (prechecks, risk limits, management plan, kill
//! switch) is identical for simulated and live positions — these modules only
//! swap HOW a decision becomes reality.
//!
//! Gating, in order, all must hold:
//!   1. `LIVE_TRADING_ENABLED` — the binary has the order path: always in the
//!      desktop's one build, never in the bot farm.
//!   2. `cfg.live` — the bot is armed this session: set only by the typed
//!      LIVE on a Live bot (`bot_set_live`, which also checks the key, the
//!      venue and the money at risk), never restored from disk.
//!   3. Futures only, on a venue whose order path passed its test-network
//!      dry run (`exchange::DRY_RUN_PASSED`: Binance today), or any order
//!      venue while the app runs against the venues' test networks
//!      (`ALEPH_EDGE_VENUE_SANDBOX=1`).
//!   4. An unlocked vault holding a Trade-capable key for the bot's exchange.
//!   5. One-way position mode, and no existing exposure on the symbol.
//!
//! Both directions: Sentinel publishes shorts as well as longs, and a
//! direction the live path skipped would stay simulated without saying so.
//!
//! FAILURE HONESTY: a fill that never happened is never recorded (entry price
//! and quantity come from Binance's response), and a live position that fails
//! to close stays in the book and retries — the book never records a close
//! the exchange did not make.

use tauri::{AppHandle, Manager};

use crate::exchange::model::BinanceKeyCheckError as E;
use crate::exchange::providers::binance_parse::{OrderFill, OrderState};
use crate::exchange::providers::binance_requests::{entry_client_id, Protective};
use crate::exchange::providers::binance_rules::{quantize_price, quantize_qty};
use crate::exchange::ExchangeManager;
use crate::signal::model::{Direction, Signal};
use crate::vault::VaultManager;

use super::super::model::{BotConfig, BotKind, TakeProfitTarget, LIVE_TRADING_ENABLED};
use super::super::BotManager;
use super::take_profit::{resolve_tp, TpResolution};

// ---- i18n keys (no silent skips) ----
// Entry skips.
pub const SKIP_VAULT_LOCKED: &str = "liveVaultLocked";
pub const SKIP_QTY_BELOW_MIN: &str = "liveQtyBelowMinimum";
pub const SKIP_ORDER_FAILED: &str = "liveOrderFailed";
pub const SKIP_STOP_FAILED: &str = "liveProtectiveStopFailed";
pub const SKIP_HEDGE_MODE: &str = "hedgeModeUnsupported";
pub const SKIP_SYMBOL_HELD: &str = "liveSymbolHeld";
pub const SKIP_EXCHANGE_CHECK_FAILED: &str = "liveExchangeCheckFailed";
pub const SKIP_SYMBOL_HAS_ORDERS: &str = "liveSymbolHasOrders";
// The liquidation rule is shared with the paper book (sizing.rs).
pub use super::sizing::{exchange_leverage, stop_beyond_liquidation, SKIP_STOP_BEYOND_LIQUIDATION};
/// Permanent setup rejections (final: retrying every tick cannot succeed and
/// only re-sends signed requests until the signal expires).
pub const SKIP_LEVERAGE_REJECTED: &str = "liveLeverageRejected";
pub const SKIP_MARGIN_TYPE_REJECTED: &str = "liveMarginTypeRejected";
pub const SKIP_SYMBOL_NOT_TRADING: &str = "liveSymbolNotTrading";
/// The entry's outcome stayed unknown and the account showed an opposite or
/// unreadable position: nothing is recorded, reconcile reports the exposure.
pub const SKIP_ORDER_UNCONFIRMED: &str = "liveOrderUnconfirmed";
// Management / close notes.
pub const SKIP_CLOSE_FAILED: &str = "liveCloseFailed";
pub const NOTE_TP_FAILED: &str = "liveTakeProfitFailed";
pub const NOTE_BREAKEVEN_FAILED: &str = "liveBreakevenMoveFailed";
/// The venue has no exchange-side breakeven move in this process (Bybit /
/// OKX before their dry run passed): the book's breakeven exit runs only
/// while the app does.
pub const NOTE_BREAKEVEN_APP_ONLY: &str = "liveBreakevenAppOnly";
pub const NOTE_STALE_STOP: &str = "liveStaleStopCancelFailed";
pub const NOTE_PARTIAL_BELOW_MIN: &str = "livePartialBelowMinimum";
pub const NOTE_PARTIAL_FAILED: &str = "livePartialFailed";
pub const NOTE_FILLS_UNAVAILABLE: &str = "liveFillsUnavailable";
pub const NOTE_FEE_NOT_USDT: &str = "liveFeeNotUsdt";
// Reconciliation.
pub const NOTE_UNTRACKED: &str = "untrackedExchangePosition";
pub const NOTE_DIRECTION_MISMATCH: &str = "liveDirectionMismatch";
pub const NOTE_RECONCILE_FAILED: &str = "liveReconcileFailed";
/// A real position with no exchange stop; the book flattens it every tick.
pub const NOTE_UNPROTECTED: &str = "liveUnprotected";
/// Exit reason: flat on the exchange, neither protective order fired.
pub const EXIT_EXCHANGE_CLOSED: &str = "exchangeClosed";

/// Whether this bot's fills should be real orders. See the module doc for the
/// full gate chain; this is the single place it is encoded.
pub fn live_trading_allowed(cfg: &BotConfig, kind: BotKind) -> bool {
    live_trading_allowed_in(cfg, kind, crate::exchange::venue::ccxt::sandbox_from_env())
}

/// `live_trading_allowed` with the sandbox switch passed in (pure, tested).
pub fn live_trading_allowed_in(cfg: &BotConfig, kind: BotKind, sandbox: bool) -> bool {
    LIVE_TRADING_ENABLED
        && cfg.live
        && kind.live_capable()
        && crate::exchange::live_venue_allowed_in(&cfg.exchange_id, sandbox)
}

/// The skip a failed pre-order setup call (trading rules, margin type,
/// leverage) earns. A Binance code that cannot change on a resend is a final
/// verdict on the signal; everything else (network, rate limit, an unknown
/// or unreadable answer) stays the transient `liveExchangeCheckFailed`.
pub fn setup_failure_skip(err: &E) -> &'static str {
    match err {
        E::Rejected { code, .. } => match code {
            // -4028 leverage not valid for the symbol; -4161 leverage cannot
            // be reduced on isolated with a position; -2027 the position
            // exceeds the maximum at this leverage.
            -4028 | -4161 | -2027 => SKIP_LEVERAGE_REJECTED,
            // Margin type cannot be changed: -4047 open orders, -4048 an
            // open position (-4046 "already isolated" is success upstream).
            -4047 | -4048 => SKIP_MARGIN_TYPE_REJECTED,
            // -1121 invalid symbol, -1122 invalid symbol status, -4140
            // symbol status refuses opening, -4141 symbol closed.
            -1121 | -1122 | -4140 | -4141 => SKIP_SYMBOL_NOT_TRADING,
            _ => SKIP_EXCHANGE_CHECK_FAILED,
        },
        _ => SKIP_EXCHANGE_CHECK_FAILED,
    }
}

/// Outcome of a real entry attempt.
pub struct LiveOpen {
    pub entry: f64,
    pub qty: f64,
    pub entry_order_id: i64,
    /// None = the stop could not be placed AND the emergency flatten did not
    /// confirm: the position is real and bare, and the book must flatten it
    /// (`OpenPosition::unprotected`).
    pub stop_algo_id: Option<i64>,
    pub tp_algo_id: Option<i64>,
    /// The take-profit resolved at the REAL fill; the exchange TP order
    /// sits at this level.
    pub tp: TpResolution,
    /// The stop the exchange order sits at (the signal's, or the alert's
    /// put at the fill).
    pub sl: f64,
}

/// Attempts for the stop and the emergency flatten, and lookups of an entry
/// whose outcome is unknown.
const ATTEMPTS: u32 = 3;
const RETRY_PAUSE: std::time::Duration = std::time::Duration::from_millis(1_000);

/// Places the real MARKET entry for `notional` USDT (from engine/sizing.rs,
/// the same size a paper position gets), its exchange-side stop, then its
/// take-profit. `leverage_cap` is the risk level's clamp.
/// Err(skip_key) = no position was left open.
#[allow(clippy::too_many_arguments)]
pub async fn open_live(
    app: &AppHandle,
    cfg: &BotConfig,
    sig: &Signal,
    live_price: f64,
    notional: f64,
    leverage_cap: u8,
    target: TakeProfitTarget,
) -> Result<LiveOpen, &'static str> {
    open_live_with(app, cfg, sig, live_price, notional, leverage_cap, target, None)
        .await
        .map_err(|r| r.key)
}

/// Why a real entry left no position, and whether the entry order's
/// outcome stayed unknown (sent, no verdict): such an entry is never sent
/// again; its client id (`entry_client_id(sig.id)`) is looked up later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveRefusal {
    pub key: &'static str,
    pub outcome_unknown: bool,
}

impl From<&'static str> for LiveRefusal {
    fn from(key: &'static str) -> Self {
        Self { key, outcome_unknown: false }
    }
}

/// The take-profit and stop after a real fill whose levels were given as a
/// percent from the price (TradingView alerts): percents are measured from
/// the FILL, then both levels are checked against it (side, both fees, the
/// liquidation price). Err = the fill made the levels unusable.
pub fn levels_after_fill(
    direction: Direction,
    fill: f64,
    levels: (super::user_trade::Level, super::user_trade::Level),
    leverage: u8,
    fee_frac: f64,
) -> Result<(f64, f64), String> {
    let (tp, sl) = super::user_trade::resolve_levels(direction, fill, levels.0, levels.1);
    super::user_trade::level_refusal(direction, fill, tp, sl, fee_frac)?;
    if stop_beyond_liquidation((fill - sl).abs() / fill, leverage) {
        return Err(SKIP_STOP_BEYOND_LIQUIDATION.to_string());
    }
    Ok((tp, sl))
}

/// The pilot cap on one real entry (pure): while `pilot_left` > 0 the
/// notional is at most `PILOT_NOTIONAL_USDT`, or just above the symbol's
/// minimum when that is higher. Per bot kind: the TradingView bot has its
/// own pilot, apart from the Futures bot's.
pub fn pilot_notional(notional: f64, pilot_left: u32, min_notional: f64) -> f64 {
    if pilot_left > 0 {
        notional.min(super::super::model::PILOT_NOTIONAL_USDT.max(min_notional * 1.1))
    } else {
        notional
    }
}

/// The fill made the alert's levels unusable: flattened (confirmed flat).
pub const SKIP_LEVELS_AFTER_FILL: &str = "liveLevelsAfterFill";

/// `open_live`, with the levels of `fill_levels` (take-profit, stop) put at
/// the real fill instead of the signal's (see `levels_after_fill`).
#[allow(clippy::too_many_arguments)]
pub async fn open_live_with(
    app: &AppHandle,
    cfg: &BotConfig,
    sig: &Signal,
    live_price: f64,
    notional: f64,
    leverage_cap: u8,
    target: TakeProfitTarget,
    fill_levels: Option<(super::user_trade::Level, super::user_trade::Level)>,
) -> Result<LiveOpen, LiveRefusal> {
    let exchange = app.state::<ExchangeManager>();
    let bots = app.state::<BotManager>();
    let Some(cred) = app.state::<VaultManager>().credential(&cfg.exchange_id) else {
        return Err(SKIP_VAULT_LOCKED.into());
    };
    let long = sig.direction == Direction::Long;
    let symbol = sig.symbol.as_str();

    // One-way mode only: in hedge mode every order needs positionSide, and a
    // closePosition stop without it is rejected — or worse, closes the wrong
    // leg. Refuse before any order exists.
    match exchange.hedge_mode(&cred, symbol).await {
        Ok(false) => {}
        Ok(true) => return Err(SKIP_HEDGE_MODE.into()),
        Err(_) => return Err(SKIP_EXCHANGE_CHECK_FAILED.into()),
    }
    // One exchange position per symbol. Two book positions on one symbol
    // (e.g. 1h and 4h signals) would net into ONE exchange position whose
    // closePosition stop closes both — the book could never reconcile it.
    // Any existing exposure (ours or the user's manual trade) refuses too.
    let held_in_book = bots
        .positions_snapshot()
        .iter()
        .any(|p| p.live && p.symbol == symbol);
    let account = exchange
        .futures_account(&cred)
        .await
        .map_err(|_| SKIP_EXCHANGE_CHECK_FAILED)?;
    // A live DCA / Grid bot owns its symbol even while it waits for its first
    // order: a signal position there would net into its position.
    let held_by_strategy = crate::bot::strategy_live::owns_symbol(app, symbol);
    if held_in_book || held_by_strategy || account.positions.iter().any(|p| p.symbol == symbol) {
        return Err(SKIP_SYMBOL_HELD.into());
    }
    // A leftover order on a flat symbol (an old closePosition stop, the
    // user's own limit) would act on the position we are about to open.
    match exchange.open_order_count(&cred, symbol).await {
        Ok(0) => {}
        Ok(_) => return Err(SKIP_SYMBOL_HAS_ORDERS.into()),
        Err(_) => return Err(SKIP_EXCHANGE_CHECK_FAILED.into()),
    }

    let rules = exchange
        .symbol_rules(&cred, symbol)
        .await
        .map_err(|e| setup_failure_skip(&e))?;
    if !rules.trading {
        return Err(SKIP_SYMBOL_NOT_TRADING.into());
    }
    let leverage = exchange_leverage(notional, cfg.capital, leverage_cap);
    let stop_frac = (live_price - sig.sl).abs() / live_price;
    if stop_beyond_liquidation(stop_frac, leverage) {
        return Err(SKIP_STOP_BEYOND_LIQUIDATION.into());
    }
    // Isolated margin at the leverage the sizing assumed: on CROSS (Binance's
    // default) the whole wallet backs the position, and a symbol left at 20x
    // from an earlier manual trade could liquidate inside the stop.
    // A permanent rejection here is judged once (`setup_failure_skip`),
    // not retried every tick until the signal expires.
    exchange
        .set_isolated(&cred, symbol)
        .await
        .map_err(|e| setup_failure_skip(&e))?;
    exchange
        .set_leverage(&cred, symbol, leverage)
        .await
        .map_err(|e| setup_failure_skip(&e))?;

    // Quantity from the sized notional at the live price, floored to
    // LOT_SIZE: flooring can only risk LESS than sized; below the minimum
    // aborts.
    // Capped to the largest single MARKET order: only ever less than sized.
    // Pilot: the first real entries after LIVE is switched on are capped
    // (just above the symbol's minimum when that is higher).
    let notional = pilot_notional(notional, bots.pilot_left(cfg.kind), rules.min_notional);
    let mut qty = quantize_qty(notional / live_price, rules.lot);
    if let Some(max) = rules.market_max_qty {
        qty = qty.min(quantize_qty(max, rules.lot));
    }
    if qty <= 0.0 || qty * live_price < rules.min_notional {
        return Err(SKIP_QTY_BELOW_MIN.into());
    }
    let client_id = entry_client_id(&sig.id);
    let fill = match exchange
        .open_market(&cred, symbol, long, qty, &client_id)
        .await
    {
        Ok(fill) => fill,
        // Never sent (no connection, rate-limit hold): nothing happened.
        Err(E::NetworkUnavailable | E::RateLimited) => return Err(SKIP_EXCHANGE_CHECK_FAILED.into()),
        Err(E::Unknown) => {
            match resolve_unknown_entry(&exchange, &cred, symbol, long, &client_id).await {
                Ok(fill) => fill,
                Err(skip) => {
                    bots.push_skip(symbol, skip, None);
                    // Sent and never confirmed either way: the caller must
                    // never send it again (the lookup can still find it).
                    return Err(LiveRefusal { key: skip, outcome_unknown: true });
                }
            }
        }
        // Final either way; a classified rejection says why.
        Err(e) => {
            return Err(match setup_failure_skip(&e) {
                SKIP_EXCHANGE_CHECK_FAILED => SKIP_ORDER_FAILED,
                skip => skip,
            }
            .into())
        }
    };

    // Levels given as a percent are put at the real fill, then re-checked
    // against it: a fill that makes them unusable is flattened, never left
    // with a stop on the wrong side.
    let at_fill;
    let sig = match fill_levels {
        None => sig,
        Some(levels) => {
            let fee = super::user_trade::round_trip_fee(cfg.kind);
            match levels_after_fill(sig.direction, fill.avg_price, levels, leverage, fee) {
                Ok((tp, sl)) => {
                    let mut s = sig.clone();
                    (s.tp, s.sl) = (vec![tp], sl);
                    at_fill = s;
                    &at_fill
                }
                Err(_) => {
                    if flatten_confirmed(&exchange, &cred, symbol, long, fill.executed_qty).await {
                        bots.push_skip(symbol, SKIP_LEVELS_AFTER_FILL, None);
                        return Err(SKIP_LEVELS_AFTER_FILL.into());
                    }
                    // Still exposed: the book flattens it every tick.
                    bots.push_skip(symbol, NOTE_UNPROTECTED, None);
                    return Ok(LiveOpen {
                        entry: fill.avg_price,
                        qty: fill.executed_qty,
                        entry_order_id: fill.order_id,
                        stop_algo_id: None,
                        tp_algo_id: None,
                        tp: TpResolution::signal_tp1(sig),
                        sl: sig.sl,
                    });
                }
            }
        }
    };

    let protection = protect_fill(
        &exchange, &bots, &cred, sig, target, &fill, live_price, rules.tick,
    )
    .await?;
    Ok(LiveOpen {
        entry: fill.avg_price,
        qty: fill.executed_qty,
        entry_order_id: fill.order_id,
        stop_algo_id: protection.stop_algo_id,
        tp_algo_id: protection.tp_algo_id,
        tp: protection.tp,
        sl: sig.sl,
    })
}

/// The exchange-side protection a filled entry got.
pub struct Protection {
    /// None = the stop failed AND the emergency flatten did not confirm:
    /// the book must hold the position flagged unprotected.
    pub stop_algo_id: Option<i64>,
    pub tp_algo_id: Option<i64>,
    pub tp: TpResolution,
}

/// The protection path every real fill of `sig` takes — a normal entry and
/// an entry reconcile adopts after its outcome was lost: the stop first
/// (retried), an emergency flatten when it cannot be placed, then the
/// take-profit. Err(SKIP_STOP_FAILED) = flattened and confirmed flat.
#[allow(clippy::too_many_arguments)]
pub async fn protect_fill(
    exchange: &ExchangeManager,
    bots: &BotManager,
    cred: &crate::vault::model::ExchangeCredential,
    sig: &Signal,
    target: TakeProfitTarget,
    fill: &OrderFill,
    ref_price: f64,
    tick: f64,
) -> Result<Protection, &'static str> {
    let long = sig.direction == Direction::Long;
    let symbol = sig.symbol.as_str();
    // The stop must exist ON THE EXCHANGE before the position counts as open.
    let stop = quantize_price(sig.sl, tick);
    let stop_algo_id = place_stop(exchange, cred, symbol, long, stop).await;
    if stop_algo_id.is_none() {
        // An unprotected leveraged position because a follow-up request
        // failed is the one state this module must never leave behind:
        // flatten, and confirm it on the account.
        if flatten_confirmed(exchange, cred, symbol, long, fill.executed_qty).await {
            return Err(SKIP_STOP_FAILED);
        }
        // Still exposed: hand it to the book flagged unprotected, which
        // flattens it every tick until it holds. Said loudly.
        bots.push_skip(symbol, NOTE_UNPROTECTED, None);
    }

    // Take-profit is NOT a safety order: if it fails the stop still protects,
    // so keep the position, say so, and let the app-side TP exit handle it.
    // Resolved at the real fill; `ref_price` (the entry gate's live price)
    // stands in if slippage alone made it fail here.
    let tp_res = resolve_tp(sig, target, fill.avg_price)
        .or_else(|_| resolve_tp(sig, target, ref_price))
        .unwrap_or_else(|_| TpResolution::signal_tp1(sig));
    let tp_algo_id = if stop_algo_id.is_some() {
        let tp = quantize_price(tp_res.tp, tick);
        match exchange
            .place_protective(cred, symbol, long, Protective::TakeProfit, tp)
            .await
        {
            Ok(id) => Some(id),
            Err(_) => {
                bots.push_skip(symbol, NOTE_TP_FAILED, None);
                None
            }
        }
    } else {
        None
    };
    Ok(Protection {
        stop_algo_id,
        tp_algo_id,
        tp: tp_res,
    })
}

/// The entry request went out and no answer came back (timeout, 5xx, a 200
/// without a fill). Binance: "execution status is UNKNOWN". Look the order
/// up by its client id; if Binance cannot say, read the position itself.
async fn resolve_unknown_entry(
    exchange: &ExchangeManager,
    cred: &crate::vault::model::ExchangeCredential,
    symbol: &str,
    long: bool,
    client_id: &str,
) -> Result<OrderFill, &'static str> {
    for _ in 0..ATTEMPTS {
        tokio::time::sleep(RETRY_PAUSE).await;
        match unknown_entry_lookup(exchange.order_by_client_id(cred, symbol, client_id).await) {
            Lookup::Filled(fill) => return Ok(fill),
            Lookup::Failed => return Err(SKIP_ORDER_FAILED),
            Lookup::LookAgain => {}
        }
    }
    // Binance could not say. The position is the ground truth.
    let account = exchange
        .futures_account(cred)
        .await
        .map_err(|_| SKIP_ORDER_UNCONFIRMED)?;
    match account.positions.iter().find(|p| p.symbol == symbol) {
        Some(p) if (p.position_amt > 0.0) == long && p.entry_price > 0.0 => Ok(OrderFill {
            order_id: 0,
            avg_price: p.entry_price,
            executed_qty: p.position_amt.abs(),
        }),
        Some(_) => Err(SKIP_ORDER_UNCONFIRMED),
        None => Err(SKIP_ORDER_FAILED),
    }
}

/// One order lookup's verdict on an entry whose outcome was unknown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Lookup {
    Filled(OrderFill),
    /// Binance says it did not and will not execute.
    Failed,
    /// Not decided yet: look again, then read the position itself.
    LookAgain,
}

/// Reads one `order_by_client_id` answer. -2013 "order does not exist" is
/// NOT a verdict here: the request whose answer was lost may still be in
/// Binance's pipeline, and an order that lands after we called it failed is
/// a real position with no stop. Only the account read after the lookups may
/// conclude "nothing happened".
pub fn unknown_entry_lookup(res: Result<OrderState, E>) -> Lookup {
    match res {
        Ok(state) => match state.fill {
            Some(fill) => Lookup::Filled(fill),
            None if matches!(state.status.as_str(), "CANCELED" | "EXPIRED" | "REJECTED") => {
                Lookup::Failed
            }
            None => Lookup::LookAgain, // NEW: a market order about to fill
        },
        Err(_) => Lookup::LookAgain,
    }
}

/// Places the exchange stop, retrying. After an unknown outcome the symbol's
/// orders are cleared first, so a retry can never leave two stops behind.
async fn place_stop(
    exchange: &ExchangeManager,
    cred: &crate::vault::model::ExchangeCredential,
    symbol: &str,
    long: bool,
    trigger: f64,
) -> Option<i64> {
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            tokio::time::sleep(RETRY_PAUSE).await;
        }
        match exchange
            .place_protective(cred, symbol, long, Protective::Stop, trigger)
            .await
        {
            Ok(id) => return Some(id),
            Err(E::Unknown) => {
                let _ = exchange.cancel_symbol_orders(cred, symbol).await;
            }
            // A rejection (e.g. -2021 "would trigger immediately") will not
            // change on a resend.
            Err(E::Rejected { .. } | E::InvalidCredentials { .. }) => return None,
            Err(_) => {}
        }
    }
    None
}

/// Flattens `qty` at market, retrying, then confirms on the account that the
/// symbol is flat. Clears the symbol's orders once it is.
pub async fn flatten_confirmed(
    exchange: &ExchangeManager,
    cred: &crate::vault::model::ExchangeCredential,
    symbol: &str,
    long: bool,
    qty: f64,
) -> bool {
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            tokio::time::sleep(RETRY_PAUSE).await;
        }
        let _ = exchange.reduce_market_all(cred, symbol, long, qty).await;
        match exchange.futures_account(cred).await {
            Ok(acc) if !acc.positions.iter().any(|p| p.symbol == symbol) => {
                let _ = exchange.cancel_symbol_orders(cred, symbol).await;
                return true;
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shared fixture config (`test_fixtures::cfg`) on `exchange`.
    fn cfg(live: bool, exchange: &str) -> BotConfig {
        BotConfig {
            exchange_id: exchange.into(),
            max_positions: 3,
            leverage: 3,
            live,
            ..crate::bot::engine::test_fixtures::cfg()
        }
    }

    // The one build has the order path, so the arm is what stands between a
    // bot and real orders: a bot not armed this session is never live, in
    // any mode, venue, kind or direction (a Live bot restored from disk
    // included: its arm is never restored).
    #[test]
    fn a_bot_not_armed_this_session_is_never_live() {
        const _: () = assert!(LIVE_TRADING_ENABLED);
        for exchange in ["binance", "okx", "bybit"] {
            for mode in [crate::bot::model::TradeMode::Paper, crate::bot::model::TradeMode::Live] {
                for kind in crate::bot::ALL_KINDS {
                    let mut c = cfg(false, exchange);
                    (c.kind, c.mode) = (kind, mode);
                    for direction in [None, Some("long".to_string()), Some("short".to_string())] {
                        c.direction = direction;
                        for sandbox in [false, true] {
                            assert!(!live_trading_allowed_in(&c, kind, sandbox), "{exchange} {mode:?} {kind:?} {sandbox}");
                        }
                    }
                }
            }
        }
    }

    // The rest of the gate chain, so a future master-switch flip cannot
    // silently widen the surface: spot, pump and non-Binance stay excluded.
    #[test]
    fn the_pilot_caps_the_first_real_entries_of_each_bot_on_its_own() {
        assert_eq!(pilot_notional(200.0, 3, 5.0), 50.0);
        assert_eq!(pilot_notional(30.0, 1, 5.0), 30.0, "below the cap: as sized");
        assert!((pilot_notional(200.0, 2, 100.0) - 110.0).abs() < 1e-9, "the symbol minimum is higher");
        assert_eq!(pilot_notional(200.0, 0, 5.0), 200.0, "pilot over");
        // The counter is per bot: TradingView entries never use up the
        // Futures bot's pilot, nor the other way round.
        let bots = BotManager::new();
        bots.set_pilot(BotKind::Futures, crate::bot::model::PILOT_TRADES);
        bots.set_pilot(BotKind::Tradingview, crate::bot::model::PILOT_TRADES);
        for _ in 0..3 {
            bots.pilot_used(BotKind::Tradingview);
        }
        assert_eq!(bots.pilot_left(BotKind::Tradingview), 0);
        assert_eq!(bots.pilot_left(BotKind::Futures), 3);
        bots.pilot_used(BotKind::Tradingview);
        assert_eq!(bots.pilot_left(BotKind::Tradingview), 0, "never below zero");
    }

    #[test]
    fn percent_levels_are_put_at_the_real_fill_and_rechecked() {
        use super::super::user_trade::Level;
        let fee = 0.001;
        // Planned at 100, filled at 101: 3% / 1.5% from the fill, not from 100.
        let (tp, sl) = levels_after_fill(Direction::Long, 101.0, (Level::Pct(3.0), Level::Pct(1.5)), 3, fee).unwrap();
        assert!((tp - 104.03).abs() < 1e-9 && (sl - 99.485).abs() < 1e-9, "{tp} {sl}");
        let (tp, sl) = levels_after_fill(Direction::Short, 99.0, (Level::Pct(3.0), Level::Pct(1.5)), 3, fee).unwrap();
        assert!((tp - 96.03).abs() < 1e-9 && (sl - 100.485).abs() < 1e-9, "{tp} {sl}");
        // A price stop the fill slipped through: the stop is now on the wrong side.
        assert_eq!(
            levels_after_fill(Direction::Long, 99.0, (Level::Price(110.0), Level::Price(99.5)), 3, fee),
            Err("tradeStopWrongSide".to_string())
        );
        // A stop past the liquidation price at the leverage the order ran.
        assert_eq!(
            levels_after_fill(Direction::Long, 100.0, (Level::Pct(20.0), Level::Pct(9.0)), 20, fee),
            Err(SKIP_STOP_BEYOND_LIQUIDATION.to_string())
        );
        // Inside both fees after the fill.
        assert!(levels_after_fill(Direction::Long, 100.0, (Level::Pct(5.0), Level::Pct(0.05)), 3, fee)
            .unwrap_err()
            .starts_with("tradeStopInsideFees|"));
    }

    #[test]
    fn exchange_leverage_puts_the_margin_at_the_capital() {
        assert_eq!(exchange_leverage(250.0, 100.0, 5), 3, "2.5x rounds up");
        assert_eq!(exchange_leverage(300.0, 100.0, 2), 2, "never above the cap");
        assert_eq!(exchange_leverage(50.0, 100.0, 5), 1);
        assert_eq!(exchange_leverage(50.0, 0.0, 3), 3);
    }

    #[test]
    fn a_stop_past_liquidation_is_refused() {
        assert!(!stop_beyond_liquidation(0.07, 2), "7% stop at 2x is fine");
        assert!(!stop_beyond_liquidation(0.07, 5), "liq ~17.5% away at 5x");
        assert!(stop_beyond_liquidation(0.07, 20), "liq ~2.5% away at 20x");
        assert!(stop_beyond_liquidation(0.03, 20));
    }

    #[test]
    fn an_order_not_found_yet_is_looked_up_again_not_called_failed() {
        let missing = Err(E::Rejected { code: -2013, msg: "Order does not exist.".into() });
        assert_eq!(unknown_entry_lookup(missing), Lookup::LookAgain);
        let state = |status: &str, fill| Ok(OrderState { status: status.into(), fill });
        assert_eq!(unknown_entry_lookup(state("NEW", None)), Lookup::LookAgain);
        assert_eq!(unknown_entry_lookup(state("EXPIRED", None)), Lookup::Failed);
        let fill = OrderFill { order_id: 7, avg_price: 2.5, executed_qty: 4.0 };
        assert_eq!(unknown_entry_lookup(state("FILLED", Some(fill))), Lookup::Filled(fill));
    }

    // Permanent leverage / margin-type / symbol rejections are judged once;
    // transport failures and unclassified rejections keep retrying.
    #[test]
    fn permanent_setup_rejections_are_final_and_transient_ones_are_not() {
        use super::super::precheck::is_final;
        let rej = |code| E::Rejected { code, msg: String::new() };
        for (code, key) in [
            (-4028, SKIP_LEVERAGE_REJECTED),
            (-4161, SKIP_LEVERAGE_REJECTED),
            (-2027, SKIP_LEVERAGE_REJECTED),
            (-4047, SKIP_MARGIN_TYPE_REJECTED),
            (-4048, SKIP_MARGIN_TYPE_REJECTED),
            (-1121, SKIP_SYMBOL_NOT_TRADING),
            (-1122, SKIP_SYMBOL_NOT_TRADING),
            (-4140, SKIP_SYMBOL_NOT_TRADING),
            (-4141, SKIP_SYMBOL_NOT_TRADING),
        ] {
            assert_eq!(setup_failure_skip(&rej(code)), key, "{code}");
            assert!(is_final(key), "{code}: {key} must be judged once");
        }
        for err in [
            E::NetworkUnavailable,
            E::RateLimited,
            E::Unknown,
            E::InvalidCredentials { code: -2015, msg: "Invalid API-key".into() },
            rej(0),     // an unreadable HTTP body, not a Binance verdict
            rej(-1021), // clock skew
            rej(-1003), // too many requests
        ] {
            let key = setup_failure_skip(&err);
            assert_eq!(key, SKIP_EXCHANGE_CHECK_FAILED, "{err:?}");
            assert!(!is_final(key), "{err:?} must retry");
        }
    }

    #[test]
    fn gate_chain_shape() {
        for sandbox in [false, true] {
            // An exchange without an order path never goes live.
            assert!(!live_trading_allowed_in(&cfg(true, "coindcx"), BotKind::Futures, sandbox));
            assert!(!live_trading_allowed_in(&cfg(true, "mexc"), BotKind::Futures, sandbox));
            assert!(!live_trading_allowed_in(&cfg(true, "binance"), BotKind::Spot, sandbox));
            assert!(!live_trading_allowed_in(&cfg(true, "binance"), BotKind::Pump, sandbox));
            for venue in ["binance", "bybit", "okx"] {
                assert!(!live_trading_allowed_in(&cfg(false, venue), BotKind::Futures, sandbox), "{venue}");
            }
        }
        // The dry-run venue follows the build switch, for the Futures bot and
        // the TradingView bot (both USDT-M futures) only.
        assert_eq!(live_trading_allowed_in(&cfg(true, "binance"), BotKind::Futures, false), LIVE_TRADING_ENABLED);
        assert_eq!(live_trading_allowed_in(&cfg(true, "binance"), BotKind::Tradingview, false), LIVE_TRADING_ENABLED);
        for venue in ["bybit", "okx", "mexc", "bitget"] {
            assert!(!live_trading_allowed_in(&cfg(true, venue), BotKind::Tradingview, false), "{venue}");
        }
        // No dry run passed: never real money, even with the build switch on
        // and a config that says live (e.g. one kept from an older version)...
        for venue in ["bybit", "okx"] {
            assert!(!live_trading_allowed_in(&cfg(true, venue), BotKind::Futures, false), "{venue}");
            // ...except against the venues' test networks, for the dry run.
            assert_eq!(live_trading_allowed_in(&cfg(true, venue), BotKind::Futures, true), LIVE_TRADING_ENABLED, "{venue}");
        }
        // No order path at all (Bitget): never, sandbox or not.
        for sandbox in [false, true] {
            assert!(!live_trading_allowed_in(&cfg(true, "bitget"), BotKind::Futures, sandbox));
        }
    }
}
