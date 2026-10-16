//! Pure builders and parsers for the foreign-trade check
//! (`exchange::foreign_fill`). No network, no clock.
//!
//! Sources (Binance USDT-M futures REST docs, read 16 Oct 2026):
//! - `GET /fapi/v1/income` (Get Income History): `incomeType=COMMISSION`,
//!   `startTime` / `endTime` inclusive, `limit` max 1000; rows carry
//!   `symbol`, `incomeType`, `income`, `asset`, `time`, `tranId` and
//!   `tradeId` (a STRING, empty when the row has no trade). Only the last
//!   three months are kept.
//! - `GET /fapi/v1/userTrades` with `fromId` (cannot be combined with a time
//!   range): `id` is the trade id, `orderId` the order it filled.
//! - `GET /fapi/v1/order` by `orderId`: `clientOrderId`.
//! - `GET /fapi/v1/algoOrder` by `algoId`: `actualOrderId` is a STRING,
//!   empty until the conditional order triggers, then the regular order id.

use serde::Deserialize;

pub const INCOME_PATH: &str = "/fapi/v1/income";
/// Binance's page maximum for income history.
pub const INCOME_PAGE: usize = 1000;

/// Commission rows (one per fill of the account) in [start, end].
pub fn commission_query(start_ms: u64, end_ms: u64, ts: u64) -> String {
    format!("incomeType=COMMISSION&startTime={start_ms}&endTime={end_ms}&limit={INCOME_PAGE}&timestamp={ts}")
}

/// The one trade `trade_id` of `symbol` (userTrades from that id, one row).
pub fn trade_by_id_query(symbol: &str, trade_id: i64, ts: u64) -> String {
    format!("symbol={symbol}&fromId={trade_id}&limit=1&timestamp={ts}")
}

/// One regular order by its exchange id.
pub fn order_by_id_query(symbol: &str, order_id: i64, ts: u64) -> String {
    format!("symbol={symbol}&orderId={order_id}&timestamp={ts}")
}

/// One fill of the account, as the commission row reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommissionRow {
    pub symbol: String,
    pub trade_id: i64,
    pub time: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawIncome {
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    income_type: String,
    time: u64,
    #[serde(default)]
    trade_id: serde_json::Value,
}

/// Commission rows with a trade id, oldest first. None when the body is not
/// an income list (an error body must never read as "no trades").
pub fn parse_commissions(body: &str) -> Option<Vec<CommissionRow>> {
    let raw: Vec<RawIncome> = serde_json::from_str(body).ok()?;
    let mut rows: Vec<CommissionRow> = raw
        .into_iter()
        .filter(|r| r.income_type == "COMMISSION" && !r.symbol.is_empty())
        .filter_map(|r| {
            Some(CommissionRow {
                trade_id: id_value(&r.trade_id)?,
                symbol: r.symbol,
                time: r.time,
            })
        })
        .collect();
    rows.sort_by_key(|r| (r.time, r.trade_id));
    Some(rows)
}

/// A Binance id sent as a number or as a numeric string; None when empty.
fn id_value(v: &serde_json::Value) -> Option<i64> {
    match v {
        serde_json::Value::Number(n) => n.as_i64(),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOrderClient {
    #[serde(default)]
    client_order_id: String,
}

/// The `clientOrderId` of a queried order.
pub fn parse_client_order_id(body: &str) -> Option<String> {
    serde_json::from_str::<RawOrderClient>(body).ok().map(|o| o.client_order_id)
}

/// Who made an order, read from its client id alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientIdOwner {
    /// This app's entry prefix.
    App,
    /// Binance itself: liquidation, auto-deleverage or settlement closes.
    Exchange,
    /// Anything else: not provably ours.
    Unknown,
}

/// Binance names its own forced closes `autoclose-…` (liquidation),
/// `adl_autoclose` (auto-deleveraging) and `settlement_autoclose-…`
/// (delisting settlement). This app's client ids have two shapes: signal and
/// New-trade entries `ae` + 16 to 34 letters/digits (`entry_client_id`), DCA /
/// Grid mirror orders `as` + 8 + `-<cycle>-<n>` (`strategy_live::client_id`).
pub fn client_id_owner(client_order_id: &str) -> ClientIdOwner {
    let c = client_order_id;
    if c.starts_with("autoclose-") || c.starts_with("adl_autoclose") || c.starts_with("settlement_autoclose") {
        return ClientIdOwner::Exchange;
    }
    let entry = c
        .strip_prefix("ae")
        .is_some_and(|b| (16..=34).contains(&b.len()) && b.chars().all(|ch| ch.is_ascii_alphanumeric()));
    let mirror = c.strip_prefix("as").is_some_and(|b| {
        let parts: Vec<&str> = b.split('-').collect();
        parts.len() == 3
            && !parts[0].is_empty()
            && parts[0].len() <= 8
            && [parts[1], parts[2]].iter().all(|p| !p.is_empty() && p.chars().all(|ch| ch.is_ascii_digit()))
    });
    if entry || mirror {
        ClientIdOwner::App
    } else {
        ClientIdOwner::Unknown
    }
}

/// What a queried algo order says about the regular order it became.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlgoOutcome {
    pub status: String,
    /// The triggered order's id; None while untriggered (empty string).
    pub actual_order_id: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAlgoOutcome {
    #[serde(default)]
    algo_status: String,
    #[serde(default)]
    actual_order_id: serde_json::Value,
}

pub fn parse_algo_outcome(body: &str) -> Option<AlgoOutcome> {
    let raw: RawAlgoOutcome = serde_json::from_str(body).ok()?;
    Some(AlgoOutcome {
        status: raw.algo_status,
        actual_order_id: id_value(&raw.actual_order_id).filter(|id| *id > 0),
    })
}

/// An algo order that ended without triggering can never own a fill.
pub fn algo_ended_untriggered(o: &AlgoOutcome) -> bool {
    o.actual_order_id.is_none() && matches!(o.status.as_str(), "CANCELED" | "EXPIRED" | "REJECTED")
}

#[cfg(test)]
mod tests {
    use super::*;

    const TS: u64 = 1_792_000_000_000;

    #[test]
    fn queries_are_byte_exact() {
        assert_eq!(
            commission_query(1, 2, TS),
            "incomeType=COMMISSION&startTime=1&endTime=2&limit=1000&timestamp=1792000000000"
        );
        assert_eq!(trade_by_id_query("XUSDT", 77, TS), "symbol=XUSDT&fromId=77&limit=1&timestamp=1792000000000");
        assert_eq!(order_by_id_query("XUSDT", 9, TS), "symbol=XUSDT&orderId=9&timestamp=1792000000000");
    }

    #[test]
    fn commission_rows_parse_string_trade_ids_and_skip_rows_without_one() {
        // Shaped like the documented response: tradeId is a string.
        let body = r#"[
          {"symbol":"ETHUSDT","incomeType":"COMMISSION","income":"-0.01","asset":"USDT","info":"","time":20,"tranId":5,"tradeId":"300"},
          {"symbol":"BTCUSDT","incomeType":"COMMISSION","income":"-0.02","asset":"BNB","info":"","time":10,"tranId":4,"tradeId":"299"},
          {"symbol":"","incomeType":"TRANSFER","income":"5","asset":"USDT","info":"TRANSFER","time":11,"tranId":3,"tradeId":""},
          {"symbol":"SOLUSDT","incomeType":"COMMISSION","income":"-0.01","asset":"USDT","info":"","time":12,"tranId":6,"tradeId":""},
          {"symbol":"SOLUSDT","incomeType":"COMMISSION","income":"-0.01","asset":"USDT","info":"","time":13,"tranId":7,"tradeId":41}
        ]"#;
        let rows = parse_commissions(body).unwrap();
        assert_eq!(
            rows,
            vec![
                CommissionRow { symbol: "BTCUSDT".into(), trade_id: 299, time: 10 },
                CommissionRow { symbol: "SOLUSDT".into(), trade_id: 41, time: 13 },
                CommissionRow { symbol: "ETHUSDT".into(), trade_id: 300, time: 20 },
            ]
        );
        assert_eq!(parse_commissions("[]"), Some(vec![]));
        assert_eq!(parse_commissions(r#"{"code":-2015,"msg":"x"}"#), None, "an error is not 'no trades'");
    }

    #[test]
    fn client_ids_name_the_app_binance_or_nobody() {
        use crate::exchange::providers::binance_requests::entry_client_id;
        let signal = entry_client_id("c3393031-8849-49a7-b006-a3d50e2d3566");
        let manual = entry_client_id("manual-1792000000000-0a1b2c3d");
        let mirror = crate::bot::strategy_live::client_id("sb_abcdefghijkl", 12, 345);
        for id in [signal.as_str(), manual.as_str(), mirror.as_str()] {
            assert_eq!(client_id_owner(id), ClientIdOwner::App, "{id}");
        }
        assert_eq!(client_id_owner("autoclose-1792000000"), ClientIdOwner::Exchange);
        assert_eq!(client_id_owner("adl_autoclose"), ClientIdOwner::Exchange);
        assert_eq!(client_id_owner("settlement_autoclose-ETHUSDT"), ClientIdOwner::Exchange);
        for other in ["web_Hk3x9", "x-3commas-1", "ae", "aeShort", "as-1-2", "asx-1-y", "ios_abc", ""] {
            assert_eq!(client_id_owner(other), ClientIdOwner::Unknown, "{other}");
        }
        assert_eq!(parse_client_order_id(r#"{"orderId":1,"clientOrderId":"web_1"}"#).as_deref(), Some("web_1"));
    }

    #[test]
    fn algo_outcomes_read_the_string_actual_order_id() {
        let untriggered = parse_algo_outcome(r#"{"algoId":1,"algoStatus":"NEW","actualOrderId":""}"#).unwrap();
        assert_eq!(untriggered.actual_order_id, None);
        assert!(!algo_ended_untriggered(&untriggered));
        let fired = parse_algo_outcome(r#"{"algoId":1,"algoStatus":"FINISHED","actualOrderId":"8389765"}"#).unwrap();
        assert_eq!(fired.actual_order_id, Some(8_389_765));
        let cancelled = parse_algo_outcome(r#"{"algoId":1,"algoStatus":"CANCELED","actualOrderId":""}"#).unwrap();
        assert!(algo_ended_untriggered(&cancelled));
        assert_eq!(parse_algo_outcome("nope"), None);
    }
}
