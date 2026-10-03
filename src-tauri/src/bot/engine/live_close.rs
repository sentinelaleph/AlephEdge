//! Closing a LIVE position and settling it on REAL fills and fees.
//!
//! The upcoming real-money test exists to measure execution against the
//! Sentinel ledger, so the trade record must carry what Binance executed, not
//! the tick price the app saw when it decided: the exit is the volume-weighted
//! price over every closing fill (partial legs included) and the fee is the
//! commission Binance charged. When fills cannot be fetched the record falls
//! back to the order responses / trigger levels and says so with a note.

use tauri::{AppHandle, Manager};

use crate::exchange::providers::binance_fills::{summarize_fills, FillSummary};
use crate::exchange::ExchangeManager;
use crate::store::model::TradeRecord;
use crate::vault::VaultManager;

use super::super::model::{OpenPosition, FUTURES_FEE_RATE};
use super::super::BotManager;
use super::live::{
    NOTE_DIRECTION_MISMATCH, NOTE_FEE_NOT_USDT, NOTE_FILLS_UNAVAILABLE, SKIP_CLOSE_FAILED,
    SKIP_VAULT_LOCKED,
};
use super::now_ms;
use super::pnl::{LEDGER_FUNDING_PCT, LEDGER_ROUND_TRIP_PCT};
use super::reconcile::{exchange_state, infer_exit_reason, reference_exit, ExchangeSide};

/// Slack before `opened_at` for the fill query: the app's clock stamps the
/// position before the order round trip, and local clocks drift from
/// Binance's. The entry order id then trims the window exactly.
const FILL_WINDOW_SLACK_MS: u64 = 60_000;

/// What the round trip really was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settlement {
    pub entry: f64,
    pub exit: f64,
    pub qty: f64,
    pub commission_usdt: Option<f64>,
    /// The exit is an executed price (fills or our own order's response),
    /// not a trigger level standing in for one.
    pub exit_is_fill: bool,
}

pub fn settlement_from_summary(s: &FillSummary) -> Settlement {
    Settlement {
        entry: s.entry_price,
        exit: s.exit_price,
        qty: s.exit_qty,
        commission_usdt: s.commission_usdt,
        exit_is_fill: true,
    }
}

/// Without fills: the entry order's own average, the exchange-reduced partial
/// leg blended in by quantity, and `final_exit` for the remainder.
pub fn fallback_settlement(pos: &OpenPosition, final_exit: f64, exit_is_fill: bool) -> Settlement {
    let partial = pos
        .partial_price
        .filter(|_| pos.partial_qty > 0.0 && pos.partial_qty < pos.qty);
    let exit = match partial {
        Some(p) => (p * pos.partial_qty + final_exit * (pos.qty - pos.partial_qty)) / pos.qty,
        None => final_exit,
    };
    Settlement {
        entry: pos.entry,
        exit,
        qty: pos.qty,
        commission_usdt: None,
        exit_is_fill,
    }
}

/// The persisted record of a live trade. `unlevered_net_pct` stays on the
/// ledger's scale: real move − real fees as % of entry notional − the
/// ledger's funding assumption (not fetched). Unknown fees fall back to the
/// ledger's 0.10 round trip there, and to the desk's taker model in quote.
pub fn live_trade_record(
    pos: &OpenPosition,
    s: &Settlement,
    reason: &str,
    closed_at: u64,
) -> TradeRecord {
    let long = pos.direction == "long";
    let sign = if long { 1.0 } else { -1.0 };
    let notional = s.entry * s.qty;
    let move_pct = if s.entry > 0.0 {
        sign * (s.exit - s.entry) / s.entry * 100.0
    } else {
        0.0
    };
    let fee_pct = match s.commission_usdt {
        Some(c) if notional > 0.0 => c / notional * 100.0,
        _ => LEDGER_ROUND_TRIP_PCT,
    };
    let funding = if long {
        LEDGER_FUNDING_PCT
    } else {
        -LEDGER_FUNDING_PCT
    };
    let fees_quote = s
        .commission_usdt
        .unwrap_or(notional * 2.0 * FUTURES_FEE_RATE);
    let pnl_quote = sign * (s.exit - s.entry) * s.qty - fees_quote;
    TradeRecord {
        id: 0,
        signal_id: pos.signal_id.clone(),
        bot_kind: pos.bot_kind.as_str().to_string(),
        exchange_id: pos.exchange_id.clone(),
        symbol: pos.symbol.clone(),
        direction: pos.direction.clone(),
        entry: s.entry,
        exit: s.exit,
        leverage: pos.leverage,
        capital: pos.capital,
        pnl_pct: if pos.capital > 0.0 {
            pnl_quote / pos.capital * 100.0
        } else {
            0.0
        },
        pnl_quote,
        exit_reason: reason.to_string(),
        fr_at_open: pos.fr_at_open,
        ld_at_open: pos.ld_at_open,
        opened_at: pos.opened_at,
        closed_at,
        unlevered_net_pct: Some(move_pct - fee_pct - funding),
        live: true,
        fill_entry: Some(s.entry),
        fill_exit: s.exit_is_fill.then_some(s.exit),
        commission_usdt: s.commission_usdt,
        sizing_mode: pos.sizing_mode.as_str().to_string(),
        risk_pct: pos.risk_pct,
        // The REAL notional at the fill, not the planned one.
        notional_usdt: Some(notional),
        effective_leverage: (pos.capital > 0.0).then(|| notional / pos.capital),
        risk_capped: pos.risk_capped,
        pnl_usdt: pnl_quote,
        pnl_pct_of_capital: if pos.capital > 0.0 {
            pnl_quote / pos.capital * 100.0
        } else {
            0.0
        },
        // Stamped by `book::close_one_on_veto` after this record comes back
        // when `reason == "veto"`; every other live close leaves it None.
        veto_reason_code: None,
        veto_reason_text: None,
        tp_target: Some(pos.tp_target.clone()),
        tp_fallback_from: pos.tp_fallback_from.clone(),
    }
}

/// Flattens a live position the book decided to close (`reason` at
/// `book_price`). Re-reads the exchange first: if it is ALREADY flat, a
/// protective order fired and that — not the book's reason — is recorded.
/// Err = nothing recorded; the position stays in the book and retries.
pub async fn close_live(
    app: &AppHandle,
    pos: &OpenPosition,
    reason: &str,
    book_price: f64,
) -> Result<TradeRecord, &'static str> {
    let exchange = app.state::<ExchangeManager>();
    let Some(cred) = app.state::<VaultManager>().credential(&pos.exchange_id) else {
        return Err(SKIP_VAULT_LOCKED);
    };
    let (key, secret) = (cred.api_key.as_str(), cred.api_secret.as_str());
    let account = exchange
        .futures_account(key, secret)
        .await
        .map_err(|_| SKIP_CLOSE_FAILED)?;
    let amt = account
        .positions
        .iter()
        .find(|p| p.symbol == pos.symbol)
        .map(|p| p.position_amt);
    let long = pos.direction == "long";
    match exchange_state(long, amt) {
        ExchangeSide::Mismatch => Err(NOTE_DIRECTION_MISMATCH),
        ExchangeSide::Flat => Ok(settle_closed(app, key, secret, pos, book_price).await),
        ExchangeSide::Open => {
            // Size from Binance itself: step-compliant, and it is what is
            // actually held after any partial.
            let size = amt.unwrap_or_default().abs();
            let fill = exchange
                .reduce_market_all(key, secret, &pos.symbol, long, size)
                .await
                .map_err(|_| SKIP_CLOSE_FAILED)?;
            let _ = exchange
                .cancel_symbol_orders(key, secret, &pos.symbol)
                .await;
            Ok(settle(app, key, secret, pos, reason, fill.avg_price, true).await)
        }
    }
}

/// Settles a position the exchange already closed: which protective order
/// fired decides the reason; leftovers on the symbol are cancelled.
pub async fn settle_closed(
    app: &AppHandle,
    key: &str,
    secret: &str,
    pos: &OpenPosition,
    book_price: f64,
) -> TradeRecord {
    let exchange = app.state::<ExchangeManager>();
    let fired = |id: Option<i64>| {
        let exchange = &exchange;
        async move {
            match id {
                Some(id) => exchange
                    .algo_status(key, secret, id)
                    .await
                    .is_ok_and(|s| crate::exchange::providers::binance_parse::algo_fired(&s)),
                None => false,
            }
        }
    };
    let stop_fired = fired(pos.stop_algo_id).await;
    let tp_fired = fired(pos.tp_algo_id).await;
    let reason = infer_exit_reason(stop_fired, tp_fired, pos.stop_at_breakeven);
    let _ = exchange
        .cancel_symbol_orders(key, secret, &pos.symbol)
        .await;
    let exit = reference_exit(pos, reason).unwrap_or(book_price);
    settle(app, key, secret, pos, reason, exit, false).await
}

async fn settle(
    app: &AppHandle,
    key: &str,
    secret: &str,
    pos: &OpenPosition,
    reason: &str,
    final_exit: f64,
    exit_is_fill: bool,
) -> TradeRecord {
    let bots = app.state::<BotManager>();
    let start = pos.opened_at.saturating_sub(FILL_WINDOW_SLACK_MS);
    let summary = app
        .state::<ExchangeManager>()
        .user_trades(key, secret, &pos.symbol, start)
        .await
        .ok()
        .and_then(|trades| summarize_fills(&trades, pos.direction == "long", pos.entry_order_id));
    let settlement = match summary {
        Some(s) => {
            if s.commission_usdt.is_none() {
                bots.push_skip(&pos.symbol, NOTE_FEE_NOT_USDT, None);
            }
            settlement_from_summary(&s)
        }
        None => {
            bots.push_skip(&pos.symbol, NOTE_FILLS_UNAVAILABLE, None);
            fallback_settlement(pos, final_exit, exit_is_fill)
        }
    };
    live_trade_record(pos, &settlement, reason, now_ms())
}
