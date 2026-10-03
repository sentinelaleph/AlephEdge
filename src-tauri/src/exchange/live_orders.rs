//! The live-order surface of `ExchangeManager`: thin wrappers that pair a pure
//! query builder with the signed transport and a pure parser. The bot engine
//! calls these and never touches reqwest, signing or wire formats directly.
//!
//! Everything here can move real money. Every ENGINE call site is reachable
//! only through `bot::engine::live::live_trading_allowed` (compile-time
//! `LIVE_TRADING_ENABLED`, false today) or through a position that path
//! opened, read via `OpenPosition::is_live()`.
//!
//! The engine is not the only caller. `exchange_close_position` and
//! `exchange_close_all` (`exchange/commands.rs`) are user-invoked Tauri
//! commands that reach `reduce_market` in EVERY build, master switch or not —
//! a manual flatten is the one thing this app will do on the real venue
//! today. Do not read the constant as "nothing here sends orders".
//!
//! One-way position mode is required; `hedge_mode` lets the engine refuse
//! before sending an order with the wrong position side.

use reqwest::Method;

use super::model::BinanceKeyCheckError as E;
use super::providers::binance_fills::{parse_user_trades, UserTrade};
use super::providers::binance_http::{now_ms as now, public_get, signed};
use super::providers::binance_parse::{
    parse_algo_id, parse_algo_status, parse_dual_side, parse_open_count, parse_order_fill,
    parse_order_state, parse_recent_orders, OrderFill, OrderState, RecentOrder,
};
use super::providers::binance_requests as q;
use super::providers::binance_rules::{close_parts, parse_symbol_rules, SymbolRules};
use super::ExchangeManager;

impl ExchangeManager {
    /// LOT_SIZE + tick for one symbol (public). Unknown symbol = error, never
    /// a guessed rule.
    pub async fn symbol_rules(&self, symbol: &str) -> Result<SymbolRules, E> {
        let body = public_get(
            &self.client,
            &format!("/fapi/v1/exchangeInfo?symbol={symbol}"),
        )
        .await?;
        parse_symbol_rules(&body, symbol).ok_or(E::Rejected {
            code: 0,
            msg: format!("no trading rules for {symbol}"),
        })
    }

    /// Sets the symbol's initial leverage.
    pub async fn set_leverage(&self, key: &str, secret: &str, symbol: &str, leverage: u8) -> Result<(), E> {
        signed(
            &self.client,
            Method::POST,
            q::LEVERAGE_PATH,
            &q::leverage_query(symbol, leverage, now()),
            key,
            secret,
        )
        .await
        .map(|_| ())
    }

    /// Puts the symbol on ISOLATED margin. Already isolated (-4046) is fine.
    pub async fn set_isolated(&self, key: &str, secret: &str, symbol: &str) -> Result<(), E> {
        match signed(
            &self.client,
            Method::POST,
            q::MARGIN_TYPE_PATH,
            &q::isolated_margin_query(symbol, now()),
            key,
            secret,
        )
        .await
        {
            Ok(_) | Err(E::Rejected { code: -4046, .. }) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Open orders on `symbol` across BOTH books (regular + algo). A leftover
    /// closePosition stop on a flat symbol would act on the next position.
    pub async fn open_order_count(&self, key: &str, secret: &str, symbol: &str) -> Result<usize, E> {
        let mut total = 0;
        for path in [q::OPEN_ORDERS_PATH, q::OPEN_ALGO_ORDERS_PATH] {
            let body = signed(
                &self.client,
                Method::GET,
                path,
                &q::symbol_query(symbol, now()),
                key,
                secret,
            )
            .await?;
            total += parse_open_count(&body).ok_or(E::Unknown)?;
        }
        Ok(total)
    }

    /// The order sent with `client_id`, or `Rejected{-2013}` if none exists.
    pub async fn order_by_client_id(
        &self,
        key: &str,
        secret: &str,
        symbol: &str,
        client_id: &str,
    ) -> Result<OrderState, E> {
        let body = signed(
            &self.client,
            Method::GET,
            q::ORDER_PATH,
            &q::order_by_client_id_query(symbol, client_id, now()),
            key,
            secret,
        )
        .await?;
        parse_order_state(&body).ok_or(E::Unknown)
    }

    /// The symbol's latest orders (read-only). Reconcile uses them to tell
    /// an entry of this app (client-id prefix) from the user's own trade.
    pub async fn recent_orders(&self, key: &str, secret: &str, symbol: &str) -> Result<Vec<RecentOrder>, E> {
        let body = signed(
            &self.client,
            Method::GET,
            q::ALL_ORDERS_PATH,
            &q::recent_orders_query(symbol, q::RECENT_ORDERS_LIMIT, now()),
            key,
            secret,
        )
        .await?;
        parse_recent_orders(&body).ok_or(E::Unknown)
    }

    /// True when the account runs hedge (dual-side) mode.
    pub async fn hedge_mode(&self, key: &str, secret: &str) -> Result<bool, E> {
        let body = signed(
            &self.client,
            Method::GET,
            q::DUAL_SIDE_PATH,
            &q::timestamp_query(now()),
            key,
            secret,
        )
        .await?;
        parse_dual_side(&body).ok_or(E::NetworkUnavailable)
    }

    /// Opening MARKET order (BUY a long, SELL a short); the real fill back.
    /// `Unknown` = it may have executed: look it up by `client_id`.
    pub async fn open_market(
        &self,
        key: &str,
        secret: &str,
        symbol: &str,
        long: bool,
        qty: f64,
        client_id: &str,
    ) -> Result<OrderFill, E> {
        let query = q::market_open_query(symbol, long, qty, client_id, now());
        let body = signed(
            &self.client,
            Method::POST,
            q::ORDER_PATH,
            &query,
            key,
            secret,
        )
        .await?;
        parse_order_fill(&body).ok_or(E::Unknown)
    }

    /// Reduce-only close of `qty`, in parts when it exceeds the symbol's
    /// MARKET_LOT_SIZE cap; one fill back with the volume-weighted price. A
    /// part that fails stops the loop: what closed so far is reported by the
    /// account on the next tick, and the caller retries the rest.
    pub async fn reduce_market_all(
        &self,
        key: &str,
        secret: &str,
        symbol: &str,
        long: bool,
        qty: f64,
    ) -> Result<OrderFill, E> {
        let parts = match self.symbol_rules(symbol).await {
            Ok(rules) => close_parts(qty, rules.market_max_qty, rules.lot),
            Err(_) => vec![qty],
        };
        let (mut filled, mut value, mut order_id) = (0.0, 0.0, 0);
        for part in parts {
            let fill = self.reduce_market(key, secret, symbol, long, part).await?;
            filled += fill.executed_qty;
            value += fill.executed_qty * fill.avg_price;
            order_id = fill.order_id;
        }
        if filled <= 0.0 {
            return Err(E::Unknown);
        }
        Ok(OrderFill { order_id, avg_price: value / filled, executed_qty: filled })
    }

    /// Reduce-only MARKET close of `qty` on the closing side; the real fill.
    pub async fn reduce_market(
        &self,
        key: &str,
        secret: &str,
        symbol: &str,
        long: bool,
        qty: f64,
    ) -> Result<OrderFill, E> {
        let query = q::reduce_market_query(symbol, long, qty, now());
        let body = signed(
            &self.client,
            Method::POST,
            q::ORDER_PATH,
            &query,
            key,
            secret,
        )
        .await?;
        parse_order_fill(&body).ok_or(E::Unknown)
    }

    /// Exchange-side stop / take-profit (closePosition algo order). Returns
    /// the algo id, which the position persists so it can be replaced,
    /// cancelled and queried later.
    pub async fn place_protective(
        &self,
        key: &str,
        secret: &str,
        symbol: &str,
        long: bool,
        kind: q::Protective,
        trigger: f64,
    ) -> Result<i64, E> {
        let query = q::protective_query(symbol, long, kind, trigger, now());
        let body = signed(
            &self.client,
            Method::POST,
            q::ALGO_ORDER_PATH,
            &query,
            key,
            secret,
        )
        .await?;
        parse_algo_id(&body).ok_or(E::Unknown)
    }

    /// Reduce-only quantity stop (the breakeven stop); its algo id.
    pub async fn place_reduce_stop(
        &self,
        key: &str,
        secret: &str,
        symbol: &str,
        long: bool,
        qty: f64,
        trigger: f64,
    ) -> Result<i64, E> {
        let body = signed(
            &self.client,
            Method::POST,
            q::ALGO_ORDER_PATH,
            &q::reduce_stop_query(symbol, long, qty, trigger, now()),
            key,
            secret,
        )
        .await?;
        parse_algo_id(&body).ok_or(E::Unknown)
    }

    pub async fn cancel_algo(&self, key: &str, secret: &str, algo_id: i64) -> Result<(), E> {
        let query = q::algo_id_query(algo_id, now());
        signed(
            &self.client,
            Method::DELETE,
            q::ALGO_ORDER_PATH,
            &query,
            key,
            secret,
        )
        .await
        .map(|_| ())
    }

    /// The algo order's `algoStatus` (NEW, TRIGGERED, FINISHED, CANCELED …).
    pub async fn algo_status(&self, key: &str, secret: &str, algo_id: i64) -> Result<String, E> {
        let query = q::algo_id_query(algo_id, now());
        let body = signed(
            &self.client,
            Method::GET,
            q::ALGO_ORDER_PATH,
            &query,
            key,
            secret,
        )
        .await?;
        parse_algo_status(&body).ok_or(E::NetworkUnavailable)
    }

    /// Cancels every open order on the symbol in BOTH books (regular and
    /// algo) so no protective order outlives its position and fires on a
    /// later one. Both are attempted; the first error is returned.
    pub async fn cancel_symbol_orders(
        &self,
        key: &str,
        secret: &str,
        symbol: &str,
    ) -> Result<(), E> {
        let regular = signed(
            &self.client,
            Method::DELETE,
            q::ALL_OPEN_ORDERS_PATH,
            &q::symbol_query(symbol, now()),
            key,
            secret,
        )
        .await;
        let algo = signed(
            &self.client,
            Method::DELETE,
            q::ALGO_OPEN_ORDERS_PATH,
            &q::symbol_query(symbol, now()),
            key,
            secret,
        )
        .await;
        regular.and(algo).map(|_| ())
    }

    /// The account's fills on `symbol` since `start_ms`, fetched in 7-day
    /// windows (Binance's limit): a position held longer used to settle on
    /// the fallback, missing its exit fills.
    pub async fn user_trades(
        &self,
        key: &str,
        secret: &str,
        symbol: &str,
        start_ms: u64,
    ) -> Result<Vec<UserTrade>, E> {
        let end = now();
        let mut trades = Vec::new();
        for (from, to) in trade_windows(start_ms, end) {
            let body = signed(
                &self.client,
                Method::GET,
                q::USER_TRADES_PATH,
                &q::user_trades_query(symbol, from, to, now()),
                key,
                secret,
            )
            .await?;
            trades.extend(parse_user_trades(&body).ok_or(E::Unknown)?);
        }
        Ok(trades)
    }
}

/// [start, end] cut into consecutive windows no longer than userTrades allows.
fn trade_windows(start: u64, end: u64) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    let mut from = start;
    while from < end {
        let to = (from + q::USER_TRADES_MAX_SPAN_MS - 1).min(end);
        out.push((from, to));
        from = to + 1;
    }
    if out.is_empty() {
        out.push((start, end.max(start)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_cover_the_span_without_gaps_or_overlap() {
        let day = 24 * 3_600_000;
        let w = trade_windows(0, 16 * day);
        assert_eq!(w.len(), 3);
        assert_eq!(w[0], (0, 7 * day - 1));
        assert_eq!(w[1].0, 7 * day);
        assert_eq!(w.last().unwrap().1, 16 * day);
        assert!(w.iter().all(|(a, b)| b - a < q::USER_TRADES_MAX_SPAN_MS));
        assert_eq!(trade_windows(5, 5), vec![(5, 5)]);
    }
}
