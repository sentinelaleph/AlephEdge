//! Pure parsers for live-order responses: order fills, algo order ids and
//! statuses, position mode. Binance sends amounts as strings and ids as
//! numbers; everything is parsed defensively and a malformed body is None,
//! never a zero that would read as a real fill.

use serde::Deserialize;

/// A MARKET order's real execution (`newOrderRespType=RESULT`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrderFill {
    pub order_id: i64,
    pub avg_price: f64,
    pub executed_qty: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOrder {
    order_id: i64,
    avg_price: String,
    executed_qty: String,
}

/// None when the body is malformed OR reports no execution: a 200 with no
/// fill is not success, and recording it would create a position the
/// exchange does not hold.
pub fn parse_order_fill(body: &str) -> Option<OrderFill> {
    let raw: RawOrder = serde_json::from_str(body).ok()?;
    let fill = OrderFill {
        order_id: raw.order_id,
        avg_price: raw.avg_price.parse().ok()?,
        executed_qty: raw.executed_qty.parse().ok()?,
    };
    (fill.avg_price > 0.0 && fill.executed_qty > 0.0).then_some(fill)
}

/// A queried order's state (`GET /fapi/v1/order`).
#[derive(Debug, Clone, PartialEq)]
pub struct OrderState {
    pub status: String,
    /// Some when anything executed (avg price and quantity both positive).
    pub fill: Option<OrderFill>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOrderState {
    order_id: i64,
    status: String,
    avg_price: String,
    executed_qty: String,
}

pub fn parse_order_state(body: &str) -> Option<OrderState> {
    let raw: RawOrderState = serde_json::from_str(body).ok()?;
    let avg: f64 = raw.avg_price.parse().ok()?;
    let qty: f64 = raw.executed_qty.parse().ok()?;
    Some(OrderState {
        status: raw.status,
        fill: (avg > 0.0 && qty > 0.0).then_some(OrderFill {
            order_id: raw.order_id,
            avg_price: avg,
            executed_qty: qty,
        }),
    })
}

/// One order from `GET /fapi/v1/allOrders`, as reconcile needs it to tell
/// this app's entries from the user's own trades.
#[derive(Debug, Clone, PartialEq)]
pub struct RecentOrder {
    pub order_id: i64,
    pub client_order_id: String,
    /// side == "BUY".
    pub buy: bool,
    pub status: String,
    pub avg_price: f64,
    pub executed_qty: f64,
    /// reduceOnly or closePosition: it can only shrink a position.
    pub reducing: bool,
    /// updateTime (ms), 0 when absent.
    pub update_time: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRecentOrder {
    order_id: i64,
    #[serde(default)]
    client_order_id: String,
    side: String,
    status: String,
    avg_price: String,
    executed_qty: String,
    #[serde(default)]
    reduce_only: bool,
    #[serde(default)]
    close_position: bool,
    #[serde(default)]
    update_time: u64,
}

/// None when the body is not an order array or any row is malformed: a
/// partly read history must not attribute (or fail to attribute) a position.
pub fn parse_recent_orders(body: &str) -> Option<Vec<RecentOrder>> {
    let raw: Vec<RawRecentOrder> = serde_json::from_str(body).ok()?;
    raw.into_iter()
        .map(|r| {
            Some(RecentOrder {
                order_id: r.order_id,
                client_order_id: r.client_order_id,
                buy: r.side == "BUY",
                status: r.status,
                avg_price: r.avg_price.parse().ok()?,
                executed_qty: r.executed_qty.parse().ok()?,
                reducing: r.reduce_only || r.close_position,
                update_time: r.update_time,
            })
        })
        .collect()
}

/// Number of entries in an open-orders array (regular or algo book).
pub fn parse_open_count(body: &str) -> Option<usize> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.as_array()
        .or_else(|| v.get("orders").and_then(|o| o.as_array()))
        .map(Vec::len)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAlgo {
    algo_id: i64,
    #[serde(default)]
    algo_status: Option<String>,
}

/// The id of a just-placed algo (conditional) order.
pub fn parse_algo_id(body: &str) -> Option<i64> {
    serde_json::from_str::<RawAlgo>(body)
        .ok()
        .map(|a| a.algo_id)
}

/// The `algoStatus` of a queried algo order.
pub fn parse_algo_status(body: &str) -> Option<String> {
    serde_json::from_str::<RawAlgo>(body).ok()?.algo_status
}

/// Whether an algo order FIRED. Binance's documented statuses are NEW,
/// TRIGGERING, TRIGGERED, FINISHED, CANCELED, EXPIRED, REJECTED; the first
/// three after NEW mean the trigger price was hit and the close order was
/// (being) sent.
pub fn algo_fired(status: &str) -> bool {
    matches!(status, "TRIGGERING" | "TRIGGERED" | "FINISHED")
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDualSide {
    dual_side_position: bool,
}

/// `GET /fapi/v1/positionSide/dual`: true = hedge mode.
pub fn parse_dual_side(body: &str) -> Option<bool> {
    serde_json::from_str::<RawDualSide>(body)
        .ok()
        .map(|d| d.dual_side_position)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_fill_needs_a_real_execution() {
        let ok = r#"{"orderId":22542179,"avgPrice":"150.2000","executedQty":"10","status":"FILLED","cumQuote":"1502"}"#;
        assert_eq!(
            parse_order_fill(ok),
            Some(OrderFill {
                order_id: 22542179,
                avg_price: 150.2,
                executed_qty: 10.0
            })
        );
        let unfilled = r#"{"orderId":1,"avgPrice":"0.00000","executedQty":"0","status":"NEW"}"#;
        assert_eq!(parse_order_fill(unfilled), None);
        assert_eq!(parse_order_fill("{}"), None);
    }

    #[test]
    fn algo_id_status_and_fired() {
        let placed =
            r#"{"algoId":2146760,"clientAlgoId":"x","algoType":"CONDITIONAL","algoStatus":"NEW"}"#;
        assert_eq!(parse_algo_id(placed), Some(2146760));
        assert_eq!(parse_algo_status(placed).as_deref(), Some("NEW"));
        for s in ["TRIGGERING", "TRIGGERED", "FINISHED"] {
            assert!(algo_fired(s), "{s}");
        }
        for s in ["NEW", "CANCELED", "EXPIRED", "REJECTED"] {
            assert!(!algo_fired(s), "{s}");
        }
    }

    #[test]
    fn order_state_reports_partial_and_empty_fills() {
        let filled = r#"{"orderId":7,"status":"FILLED","avgPrice":"2.5","executedQty":"4"}"#;
        assert_eq!(parse_order_state(filled).unwrap().fill.unwrap().executed_qty, 4.0);
        let new = r#"{"orderId":7,"status":"NEW","avgPrice":"0","executedQty":"0"}"#;
        let s = parse_order_state(new).unwrap();
        assert_eq!((s.status.as_str(), s.fill), ("NEW", None));
        assert_eq!(parse_order_state("{}"), None);
    }

    #[test]
    fn recent_orders_parse_side_fill_and_reducing_flags() {
        let body = r#"[
            {"orderId":1,"clientOrderId":"aeabc","side":"SELL","status":"FILLED","avgPrice":"150.2","executedQty":"10","reduceOnly":false,"closePosition":false,"updateTime":1758100000000,"type":"MARKET"},
            {"orderId":2,"clientOrderId":"web_x","side":"BUY","status":"NEW","avgPrice":"0","executedQty":"0","reduceOnly":false,"closePosition":true,"updateTime":5}
        ]"#;
        let orders = parse_recent_orders(body).unwrap();
        assert_eq!(orders.len(), 2);
        assert_eq!(
            orders[0],
            RecentOrder {
                order_id: 1,
                client_order_id: "aeabc".into(),
                buy: false,
                status: "FILLED".into(),
                avg_price: 150.2,
                executed_qty: 10.0,
                reducing: false,
                update_time: 1_758_100_000_000,
            }
        );
        assert!(orders[1].buy && orders[1].reducing, "closePosition reduces");
        assert_eq!(parse_recent_orders("[]"), Some(vec![]));
        assert_eq!(parse_recent_orders("{}"), None);
        let bad = r#"[{"orderId":1,"side":"BUY","status":"FILLED","avgPrice":"x","executedQty":"1"}]"#;
        assert_eq!(parse_recent_orders(bad), None, "a malformed row fails the whole read");
    }

    #[test]
    fn open_counts_accept_both_shapes() {
        assert_eq!(parse_open_count("[]"), Some(0));
        assert_eq!(parse_open_count(r#"[{"orderId":1},{"orderId":2}]"#), Some(2));
        assert_eq!(parse_open_count(r#"{"orders":[{"algoId":1}]}"#), Some(1));
        assert_eq!(parse_open_count("x"), None);
    }

    #[test]
    fn dual_side_mode() {
        assert_eq!(parse_dual_side(r#"{"dualSidePosition":true}"#), Some(true));
        assert_eq!(
            parse_dual_side(r#"{"dualSidePosition":false}"#),
            Some(false)
        );
        assert_eq!(parse_dual_side("[]"), None);
    }
}
