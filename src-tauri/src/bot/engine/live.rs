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
//!   1. `LIVE_TRADING_ENABLED` — compile-time master switch, false today.
//!      Flipping it is a deliberate release act, not a setting.
//!   2. `cfg.live` — per-bot opt-in the user sets, default false.
//!   3. Futures only, on a venue whose order path passed its test-network
//!      dry run (`exchange::DRY_RUN_PASSED`: Binance today), or any order
//!      venue while the app runs against the venues' test networks
//!      (`ALEPH_EDGE_VENUE_SANDBOX=1`).
//!   4. An unlocked vault holding a Trade-capable key for the bot's exchange.
//!   5. One-way position mode, and no existing exposure on the symbol.
//!
//! Both directions. The old LONG-only rule dated from Sentinel publishing
//! longs only; 39 of the 42 signals pending on 2026-09-16 were SHORT, so it
//! kept nearly the whole live book simulated without saying so.
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

// ---- i18n keys (PRD §5.4: no silent skips) ----
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
        && kind == BotKind::Futures
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
    let exchange = app.state::<ExchangeManager>();
    let bots = app.state::<BotManager>();
    let Some(cred) = app.state::<VaultManager>().credential(&cfg.exchange_id) else {
        return Err(SKIP_VAULT_LOCKED);
    };
    let long = sig.direction == Direction::Long;
    let symbol = sig.symbol.as_str();

    // One-way mode only: in hedge mode every order needs positionSide, and a
    // closePosition stop without it is rejected — or worse, closes the wrong
    // leg. Refuse before any order exists.
    match exchange.hedge_mode(&cred, symbol).await {
        Ok(false) => {}
        Ok(true) => return Err(SKIP_HEDGE_MODE),
        Err(_) => return Err(SKIP_EXCHANGE_CHECK_FAILED),
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
        return Err(SKIP_SYMBOL_HELD);
    }
    // A leftover order on a flat symbol (an old closePosition stop, the
    // user's own limit) would act on the position we are about to open.
    match exchange.open_order_count(&cred, symbol).await {
        Ok(0) => {}
        Ok(_) => return Err(SKIP_SYMBOL_HAS_ORDERS),
        Err(_) => return Err(SKIP_EXCHANGE_CHECK_FAILED),
    }

    let rules = exchange
        .symbol_rules(&cred, symbol)
        .await
        .map_err(|e| setup_failure_skip(&e))?;
    if !rules.trading {
        return Err(SKIP_SYMBOL_NOT_TRADING);
    }
    let leverage = exchange_leverage(notional, cfg.capital, leverage_cap);
    let stop_frac = (live_price - sig.sl).abs() / live_price;
    if stop_beyond_liquidation(stop_frac, leverage) {
        return Err(SKIP_STOP_BEYOND_LIQUIDATION);
    }
    // Isolated margin at the leverage the sizing assumed: on CROSS (Binance's
    // default) the whole wallet backs the position, and a symbol left at 20x
    // from an earlier manual trade could liquidate inside the stop.
    // A permanent rejection here is judged once (`setup_failure_skip`);
    // it used to retry every tick until the signal expired.
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
    let notional = if bots.pilot_left(cfg.kind) > 0 {
        notional.min(super::super::model::PILOT_NOTIONAL_USDT.max(rules.min_notional * 1.1))
    } else {
        notional
    };
    let mut qty = quantize_qty(notional / live_price, rules.lot);
    if let Some(max) = rules.market_max_qty {
        qty = qty.min(quantize_qty(max, rules.lot));
    }
    if qty <= 0.0 || qty * live_price < rules.min_notional {
        return Err(SKIP_QTY_BELOW_MIN);
    }
    let client_id = entry_client_id(&sig.id);
    let fill = match exchange
        .open_market(&cred, symbol, long, qty, &client_id)
        .await
    {
        Ok(fill) => fill,
        // Never sent (no connection, rate-limit hold): nothing happened.
        Err(E::NetworkUnavailable | E::RateLimited) => return Err(SKIP_EXCHANGE_CHECK_FAILED),
        Err(E::Unknown) => {
            match resolve_unknown_entry(&exchange, &cred, symbol, long, &client_id).await {
                Ok(fill) => fill,
                Err(skip) => {
                    bots.push_skip(symbol, skip, None);
                    return Err(skip);
                }
            }
        }
        // Final either way; a classified rejection says why.
        Err(e) => {
            return Err(match setup_failure_skip(&e) {
                SKIP_EXCHANGE_CHECK_FAILED => SKIP_ORDER_FAILED,
                skip => skip,
            })
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
            Err(E::Rejected { .. } | E::InvalidCredentials) => return None,
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

    fn cfg(live: bool, exchange: &str) -> BotConfig {
        BotConfig {
            kind: BotKind::Futures,
            exchange_id: exchange.into(),
            max_positions: 3,
            capital: 100.0,
            leverage: 3,
            min_confidence: None,
            direction: None,
            symbols: vec![],
            combos: vec![],
            engines: vec![],
            live,
            max_loss_pct: None,
            sizing: crate::bot::model::SizingMode::Fixed,
            risk_per_trade_pct: 1.0,
            take_profit: Default::default(),
            take_profit_overrides: Default::default(),
            // Fixtures carry fixed dates; the age gate has its own tests.
            max_signal_age_min: 0,
        }
    }

    // The master switch is false in this build, so NOTHING may be considered
    // live — not even a fully opted-in Binance futures bot, in any direction
    // or configuration. This test is the contract that shipping the live path
    // changes no behaviour.
    #[cfg(not(feature = "live"))]
    #[test]
    fn master_switch_off_means_never_live() {
        // Checked at compile time: this build cannot even compile with it on.
        const _: () = assert!(!LIVE_TRADING_ENABLED);
        for exchange in ["binance", "okx", "bybit"] {
            for live in [true, false] {
                for kind in [BotKind::Futures, BotKind::Spot, BotKind::Pump] {
                    let mut c = cfg(live, exchange);
                    c.kind = kind;
                    for direction in [None, Some("long".to_string()), Some("short".to_string())] {
                        c.direction = direction;
                        assert!(
                            !live_trading_allowed(&c, kind),
                            "{exchange} {live} {kind:?}"
                        );
                    }
                }
            }
        }
    }

    // The rest of the gate chain, so a future master-switch flip cannot
    // silently widen the surface: spot, pump and non-Binance stay excluded.
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
            E::InvalidCredentials,
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
        // The dry-run venue follows the build switch.
        assert_eq!(live_trading_allowed_in(&cfg(true, "binance"), BotKind::Futures, false), LIVE_TRADING_ENABLED);
        // No dry run passed: never real money, even with the build switch on
        // and a config that says live (e.g. one kept from an older version)...
        for venue in ["bybit", "okx"] {
            assert!(!live_trading_allowed_in(&cfg(true, venue), BotKind::Futures, false), "{venue}");
            // ...except against the venues' test networks, for the dry run.
            assert_eq!(live_trading_allowed_in(&cfg(true, venue), BotKind::Futures, true), LIVE_TRADING_ENABLED, "{venue}");
        }
        // No order path at all (Bitget since 2026-10-09): never, sandbox or not.
        for sandbox in [false, true] {
            assert!(!live_trading_allowed_in(&cfg(true, "bitget"), BotKind::Futures, sandbox));
        }
    }
}
