//! Pure builders for every signed live-order query. No network, no clock:
//! the timestamp is a parameter, so each string is tested byte for byte.
//!
//! Direction-generic on purpose. 39 of the 42 Sentinel signals pending on
//! 2026-09-16 were SHORT; the old LONG-only path (side=BUY entry, side=SELL
//! stop hardcoded) silently kept every one of them simulated.
//!
//! Conditional orders (stop, take-profit) go to `/fapi/v1/algoOrder` with
//! `algoType=CONDITIONAL` and `triggerPrice`. Binance migrated them off
//! `/fapi/v1/order` on 2025-12-09; the old endpoint now rejects STOP_MARKET /
//! TAKE_PROFIT_MARKET with -4120, which would have made every live entry fail
//! its stop and flatten immediately.

pub const ORDER_PATH: &str = "/fapi/v1/order";
pub const ALGO_ORDER_PATH: &str = "/fapi/v1/algoOrder";
pub const ALL_OPEN_ORDERS_PATH: &str = "/fapi/v1/allOpenOrders";
pub const ALGO_OPEN_ORDERS_PATH: &str = "/fapi/v1/algoOpenOrders";
pub const USER_TRADES_PATH: &str = "/fapi/v1/userTrades";
pub const DUAL_SIDE_PATH: &str = "/fapi/v1/positionSide/dual";
pub const LEVERAGE_PATH: &str = "/fapi/v1/leverage";
pub const MARGIN_TYPE_PATH: &str = "/fapi/v1/marginType";
pub const OPEN_ORDERS_PATH: &str = "/fapi/v1/openOrders";
pub const OPEN_ALGO_ORDERS_PATH: &str = "/fapi/v1/openAlgoOrders";
pub const ALL_ORDERS_PATH: &str = "/fapi/v1/allOrders";
/// How many of a symbol's latest orders reconcile reads to attribute an
/// untracked position (one entry plus its stop/TP and a close are a handful).
pub const RECENT_ORDERS_LIMIT: u32 = 50;
/// Every entry this app sends carries this client-id prefix (see
/// `entry_client_id`); reconcile attributes positions by it.
pub const ENTRY_CLIENT_PREFIX: &str = "ae";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Buy => "BUY",
            Side::Sell => "SELL",
        }
    }

    /// The side that OPENS a position: BUY a long, SELL a short.
    pub fn opening(long: bool) -> Side {
        if long {
            Side::Buy
        } else {
            Side::Sell
        }
    }

    /// The side that REDUCES it — every stop, take-profit and close.
    pub fn closing(long: bool) -> Side {
        Self::opening(!long)
    }
}

/// Which exchange-side protective order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protective {
    Stop,
    TakeProfit,
}

impl Protective {
    fn order_type(self) -> &'static str {
        match self {
            Protective::Stop => "STOP_MARKET",
            Protective::TakeProfit => "TAKE_PROFIT_MARKET",
        }
    }
}

/// Opening MARKET order. `RESULT` so the response carries the real average
/// fill and executed quantity — the honest entry, not the pre-order ticker.
/// `client_id` (see `entry_client_id`) makes an ambiguous send checkable:
/// the order can be looked up by it, and Binance refuses a duplicate.
pub fn market_open_query(symbol: &str, long: bool, qty: f64, client_id: &str, ts: u64) -> String {
    format!(
        "symbol={symbol}&side={}&type=MARKET&quantity={}&newClientOrderId={client_id}&newOrderRespType=RESULT&timestamp={ts}",
        Side::opening(long).as_str(),
        format_decimal(qty)
    )
}

/// The entry order's client id for one signal: "ae" + the signal id's
/// allowed characters, cut to Binance's 36 (`^[\.A-Z\:/a-z0-9_-]{1,36}$`).
/// Deterministic, so a lookup after a timeout finds the order the timed-out
/// request placed.
pub fn entry_client_id(signal_id: &str) -> String {
    let body: String = signal_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(34)
        .collect();
    format!("{ENTRY_CLIENT_PREFIX}{body}")
}

/// The symbol's latest orders, newest included (`GET /fapi/v1/allOrders`).
pub fn recent_orders_query(symbol: &str, limit: u32, ts: u64) -> String {
    format!("symbol={symbol}&limit={limit}&timestamp={ts}")
}

/// Look up one order by the client id it was sent with.
pub fn order_by_client_id_query(symbol: &str, client_id: &str, ts: u64) -> String {
    format!("symbol={symbol}&origClientOrderId={client_id}&timestamp={ts}")
}

pub fn order_by_id_query(symbol: &str, order_id: i64, ts: u64) -> String {
    format!("symbol={symbol}&orderId={order_id}&timestamp={ts}")
}

/// Initial leverage for the symbol (1–125).
pub fn leverage_query(symbol: &str, leverage: u8, ts: u64) -> String {
    format!("symbol={symbol}&leverage={leverage}&timestamp={ts}")
}

/// ISOLATED margin: a position can lose its own margin, never the wallet.
pub fn isolated_margin_query(symbol: &str, ts: u64) -> String {
    format!("symbol={symbol}&marginType=ISOLATED&timestamp={ts}")
}

/// Reduce-only MARKET close of `qty` (a partial, or the final close).
/// `reduceOnly=true` means a stale quantity can shrink the position but never
/// flip it into the opposite direction.
pub fn reduce_market_query(symbol: &str, long: bool, qty: f64, ts: u64) -> String {
    format!(
        "symbol={symbol}&side={}&type=MARKET&quantity={}&reduceOnly=true&newOrderRespType=RESULT&timestamp={ts}",
        Side::closing(long).as_str(),
        format_decimal(qty)
    )
}

/// Exchange-side stop or take-profit with `closePosition=true`: no quantity,
/// so it covers whatever is left after a partial, and it lives ON BINANCE —
/// it still fires if the app is closed. CONTRACT_PRICE (last trade) is the
/// price series Sentinel's ledger evaluates against, not the mark price.
pub fn protective_query(
    symbol: &str,
    long: bool,
    kind: Protective,
    trigger: f64,
    ts: u64,
) -> String {
    format!(
        "algoType=CONDITIONAL&symbol={symbol}&side={}&type={}&triggerPrice={}&closePosition=true&workingType=CONTRACT_PRICE&timestamp={ts}",
        Side::closing(long).as_str(),
        kind.order_type(),
        format_decimal(trigger)
    )
}

/// A quantity stop (`reduceOnly`, one-way mode only): the breakeven stop.
/// Placed BESIDE the original closePosition stop before that one is
/// cancelled; a second closePosition stop on the same side is not documented
/// as allowed, a reduce-only quantity stop is. reduceOnly caps it at whatever
/// remains, so it can never open or flip a position.
pub fn reduce_stop_query(symbol: &str, long: bool, qty: f64, trigger: f64, ts: u64) -> String {
    format!(
        "algoType=CONDITIONAL&symbol={symbol}&side={}&type=STOP_MARKET&triggerPrice={}&quantity={}&reduceOnly=true&workingType=CONTRACT_PRICE&timestamp={ts}",
        Side::closing(long).as_str(),
        format_decimal(trigger),
        format_decimal(qty)
    )
}

/// Cancel or query one algo order by id (same parameters for both).
pub fn algo_id_query(algo_id: i64, ts: u64) -> String {
    format!("algoId={algo_id}&timestamp={ts}")
}

/// Cancel-all for one symbol; sent to BOTH the regular and the algo open-order
/// endpoints, because the migration split orders across two books.
pub fn symbol_query(symbol: &str, ts: u64) -> String {
    format!("symbol={symbol}&timestamp={ts}")
}

/// The account's fills on `symbol` in [start, end]. Binance allows at most 7
/// days between the two (see `USER_TRADES_MAX_SPAN_MS`); 1000 is its page
/// max, and one position is a handful of fills.
pub fn user_trades_query(symbol: &str, start_ms: u64, end_ms: u64, ts: u64) -> String {
    format!("symbol={symbol}&startTime={start_ms}&endTime={end_ms}&limit=1000&timestamp={ts}")
}

/// userTrades' documented maximum span between startTime and endTime.
pub const USER_TRADES_MAX_SPAN_MS: u64 = 7 * 24 * 3_600_000;

pub fn timestamp_query(ts: u64) -> String {
    format!("timestamp={ts}")
}

/// Absolute value, at most 8 decimals, trailing zeros trimmed. Callers
/// quantize to the symbol's step/tick first; rounding at 8 places only
/// absorbs f64 noise (2.2999999999 → "2.3").
pub fn format_decimal(v: f64) -> String {
    let s = format!("{:.8}", v.abs());
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TS: u64 = 1_758_100_000_000;

    #[test]
    fn sides_follow_direction() {
        assert_eq!(Side::opening(true), Side::Buy);
        assert_eq!(Side::opening(false), Side::Sell);
        assert_eq!(Side::closing(true), Side::Sell);
        assert_eq!(Side::closing(false), Side::Buy);
    }

    #[test]
    fn market_open_long_and_short() {
        assert_eq!(
            market_open_query("SOLUSDT", true, 2.3, "aeX", TS),
            "symbol=SOLUSDT&side=BUY&type=MARKET&quantity=2.3&newClientOrderId=aeX&newOrderRespType=RESULT&timestamp=1758100000000"
        );
        assert_eq!(
            market_open_query("SOLUSDT", false, 10.0, "aeX", TS),
            "symbol=SOLUSDT&side=SELL&type=MARKET&quantity=10&newClientOrderId=aeX&newOrderRespType=RESULT&timestamp=1758100000000"
        );
    }

    #[test]
    fn entry_client_id_fits_binance_rules_and_is_stable() {
        let id = entry_client_id("c3393031-8849-49a7-b006-a3d50e2d3566");
        assert_eq!(id, "aec3393031884949a7b006a3d50e2d3566");
        assert!(id.len() <= 36 && id.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_eq!(id, entry_client_id("c3393031-8849-49a7-b006-a3d50e2d3566"));
        assert_ne!(id, entry_client_id("d3393031-8849-49a7-b006-a3d50e2d3566"));
    }

    #[test]
    fn recent_orders_query_is_symbol_scoped() {
        assert_eq!(
            recent_orders_query("SOLUSDT", RECENT_ORDERS_LIMIT, TS),
            "symbol=SOLUSDT&limit=50&timestamp=1758100000000"
        );
        assert!(entry_client_id("x-1").starts_with(ENTRY_CLIENT_PREFIX));
    }

    #[test]
    fn leverage_margin_and_lookup_queries() {
        assert_eq!(leverage_query("SOLUSDT", 2, TS), "symbol=SOLUSDT&leverage=2&timestamp=1758100000000");
        assert_eq!(
            isolated_margin_query("SOLUSDT", TS),
            "symbol=SOLUSDT&marginType=ISOLATED&timestamp=1758100000000"
        );
        assert_eq!(
            order_by_client_id_query("SOLUSDT", "aeX", TS),
            "symbol=SOLUSDT&origClientOrderId=aeX&timestamp=1758100000000"
        );
    }

    #[test]
    fn reduce_market_is_reduce_only_on_the_closing_side() {
        assert_eq!(
            reduce_market_query("ETHUSDT", true, 0.015, TS),
            "symbol=ETHUSDT&side=SELL&type=MARKET&quantity=0.015&reduceOnly=true&newOrderRespType=RESULT&timestamp=1758100000000"
        );
        assert_eq!(
            reduce_market_query("ETHUSDT", false, 0.015, TS),
            "symbol=ETHUSDT&side=BUY&type=MARKET&quantity=0.015&reduceOnly=true&newOrderRespType=RESULT&timestamp=1758100000000"
        );
    }

    #[test]
    fn stop_and_tp_are_close_position_algo_orders_on_the_closing_side() {
        // Long: stop below, TP above, both SELL.
        assert_eq!(
            protective_query("SOLUSDT", true, Protective::Stop, 142.35, TS),
            "algoType=CONDITIONAL&symbol=SOLUSDT&side=SELL&type=STOP_MARKET&triggerPrice=142.35&closePosition=true&workingType=CONTRACT_PRICE&timestamp=1758100000000"
        );
        assert_eq!(
            protective_query("SOLUSDT", true, Protective::TakeProfit, 151.2, TS),
            "algoType=CONDITIONAL&symbol=SOLUSDT&side=SELL&type=TAKE_PROFIT_MARKET&triggerPrice=151.2&closePosition=true&workingType=CONTRACT_PRICE&timestamp=1758100000000"
        );
        // Short: both BUY. No quantity and no reduceOnly — closePosition
        // forbids a quantity and is inherently reduce-only.
        let stop = protective_query("DOGEUSDT", false, Protective::Stop, 0.24871, TS);
        assert!(stop.contains("side=BUY&type=STOP_MARKET&triggerPrice=0.24871&closePosition=true"));
        assert!(!stop.contains("quantity") && !stop.contains("reduceOnly"));
        let tp = protective_query("DOGEUSDT", false, Protective::TakeProfit, 0.2301, TS);
        assert!(tp.contains("side=BUY&type=TAKE_PROFIT_MARKET&triggerPrice=0.2301"));
    }

    #[test]
    fn breakeven_stop_is_a_reduce_only_quantity_order() {
        assert_eq!(
            reduce_stop_query("SOLUSDT", true, 1.5, 150.2, TS),
            "algoType=CONDITIONAL&symbol=SOLUSDT&side=SELL&type=STOP_MARKET&triggerPrice=150.2&quantity=1.5&reduceOnly=true&workingType=CONTRACT_PRICE&timestamp=1758100000000"
        );
        let short = reduce_stop_query("SOLUSDT", false, 1.5, 150.2, TS);
        assert!(short.contains("side=BUY") && !short.contains("closePosition"));
    }

    #[test]
    fn id_symbol_trades_and_timestamp_queries() {
        assert_eq!(
            algo_id_query(2146760, TS),
            "algoId=2146760&timestamp=1758100000000"
        );
        assert_eq!(
            symbol_query("BTCUSDT", TS),
            "symbol=BTCUSDT&timestamp=1758100000000"
        );
        assert_eq!(
            user_trades_query("BTCUSDT", 1_758_000_000_000, 1_758_050_000_000, TS),
            "symbol=BTCUSDT&startTime=1758000000000&endTime=1758050000000&limit=1000&timestamp=1758100000000"
        );
        assert_eq!(timestamp_query(TS), "timestamp=1758100000000");
    }

    #[test]
    fn decimals_trim_and_absorb_noise() {
        assert_eq!(format_decimal(2.3), "2.3");
        assert_eq!(format_decimal(-2.3), "2.3");
        assert_eq!(format_decimal(2.2999999999), "2.3");
        assert_eq!(format_decimal(10.0), "10");
        assert_eq!(format_decimal(0.00001234), "0.00001234");
    }
}
