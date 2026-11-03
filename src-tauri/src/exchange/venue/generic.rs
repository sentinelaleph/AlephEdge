//! The pure half of the order path for the venues that run through ccxt's
//! UNIFIED methods (every ccxt venue except Bybit and OKX, which keep their
//! hand-written, endpoint-level path in `ccxt.rs`).
//!
//! The protective stop is ccxt's `stopLossPrice` (take-profit:
//! `takeProfitPrice`) on a reduce-only market order: ccxt routes it to the
//! venue's own stop-loss mechanism and infers the trigger direction from the
//! side. Where the venue can close the whole position (`StopKind::
//! ClosesPosition`) ccxt is asked for that (Bitget `pos_loss`, BingX
//! `closePosition`). Elsewhere
//! it is a reduce-only stop for the full size, which `resync_stops` keeps at
//! the position's size after every add or reduce the app makes.
//!
//! Our protective orders are found again by their client id, computed from
//! the protective id (`placed_ms * 10 + kind`, as on Bybit / OKX) in the
//! venue's own client-id form. A size re-sync places a replacement under the
//! next generation of that id. Venues that do not echo client ids on stop
//! orders (Gate price orders) fall back to
//! the order's own stop-loss / take-profit field, then to the trigger's side
//! of the position's entry price.

use ccxt::types::Order;
use ccxt::Params;

use super::caps::{client_id_for, StopKind, VenueCaps};
use crate::exchange::providers::binance_requests::Protective;

/// How many size re-syncs one protective order may go through. Each gets
/// its own client id (`ap<id>`, `ap<id>r1`, …); lookups try all of them.
pub const MAX_GENERATIONS: u32 = 16;

/// The client id generation `gen` of protective `id` carries on the venue.
pub fn protective_client_id(caps: &VenueCaps, id: i64, gen: u32) -> String {
    let raw = if gen == 0 { format!("ap{id}") } else { format!("ap{id}r{gen}") };
    client_id_for(caps.client_id, &raw)
}

/// The generation of protective `id` an order's client id is, if any.
pub fn generation_of(caps: &VenueCaps, id: i64, client_order_id: Option<&str>) -> Option<u32> {
    let cid = client_order_id.filter(|c| !c.is_empty())?;
    (0..MAX_GENERATIONS).find(|g| protective_client_id(caps, id, *g) == cid)
}

/// The params of a protective order of `kind` at trigger `px`.
pub fn protective_params(caps: &VenueCaps, kind: Protective, px: f64, client_id: &str) -> Params {
    let mut p = match kind {
        Protective::Stop => Params::new().with_float("stopLossPrice", px),
        Protective::TakeProfit => Params::new().with_float("takeProfitPrice", px),
    };
    p = p.with_str("clientOrderId", client_id);
    // ccxt: "true to close the entire position with a TP/SL order, in which
    // case the quantity is not sent". BingX refuses it together with
    // reduceOnly. (Bitget's pos_loss needs no extra param.)
    if caps.stop == StopKind::ClosesPosition && caps.id == "bingx" {
        return p.with_bool("closePosition", true);
    }
    p.with_bool("reduceOnly", true)
}

/// A position as the generic path needs it: signed size in BASE coin and
/// the entry price (0 when unknown).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Held {
    pub amt: f64,
    pub entry: f64,
}

/// One resting order read as a protective order of ours or not.
#[derive(Debug, Clone, PartialEq)]
pub struct Resting {
    pub venue_id: String,
    pub kind: Protective,
    pub trigger: f64,
    /// In the venue's own unit (contracts where the venue trades contracts).
    pub amount: f64,
    /// Our protective id's generation, when the venue echoed the client id.
    pub generation: Option<u32>,
    /// It carries a client id that is not ours: someone else's order.
    pub foreign: bool,
}

fn trigger_of(o: &Order) -> Option<f64> {
    o.stop_loss_price.or(o.take_profit_price).or(o.trigger_price).or(o.stop_price).filter(|p| *p > 0.0)
}

/// Whether `o` closes a position at a trigger (a stop or take-profit), and
/// which kind. `held` decides when the order does not say: for a long, a
/// trigger under the entry is a stop; for a short, above it.
pub fn classify(o: &Order, held: Option<Held>) -> Option<(Protective, f64)> {
    let trigger = trigger_of(o)?;
    if o.stop_loss_price.is_some_and(|p| p > 0.0) {
        return Some((Protective::Stop, trigger));
    }
    if o.take_profit_price.is_some_and(|p| p > 0.0) {
        return Some((Protective::TakeProfit, trigger));
    }
    // A trigger order that opens is not protective.
    if o.reduce_only == Some(false) {
        return None;
    }
    let h = held.filter(|h| h.amt != 0.0 && h.entry > 0.0)?;
    let long = h.amt > 0.0;
    // The closing side only: a buy trigger cannot protect a long.
    if o.side.as_deref() == Some(if long { "buy" } else { "sell" }) {
        return None;
    }
    let below = trigger < h.entry;
    Some((if below == long { Protective::Stop } else { Protective::TakeProfit }, trigger))
}

/// The resting protective orders among `orders` (deduplicated by id).
pub fn resting(caps: &VenueCaps, orders: &[Order], held: Option<Held>, id_hint: Option<i64>) -> Vec<Resting> {
    let mut out: Vec<Resting> = Vec::new();
    for o in orders {
        if matches!(o.status.as_deref(), Some("closed" | "canceled" | "expired" | "rejected")) {
            continue;
        }
        let Some(venue_id) = o.id.clone().filter(|i| !i.is_empty()) else { continue };
        if out.iter().any(|r| r.venue_id == venue_id) {
            continue;
        }
        let Some((kind, trigger)) = classify(o, held) else { continue };
        let generation = id_hint.and_then(|id| generation_of(caps, id, o.client_order_id.as_deref()));
        let foreign = generation.is_none() && o.client_order_id.as_deref().is_some_and(|c| !c.is_empty() && !is_protective_cid(c));
        out.push(Resting { venue_id, kind, trigger, amount: o.amount.unwrap_or(0.0), generation, foreign });
    }
    out
}

/// A client id one of our protective orders could carry without knowing
/// which id (an `ap` id in the Alnum / Gate forms). Numeric and hex forms
/// cannot be told apart from anyone else's, so they count as unknown here.
fn is_protective_cid(c: &str) -> bool {
    let c = c.strip_prefix("t-").unwrap_or(c);
    c.starts_with("ap") || c.starts_with("0x") || c.chars().all(|ch| ch.is_ascii_digit())
}

/// The resting order that IS protective `id` of `kind`: by client id when
/// the venue echoed it, else a resting order of that kind that carries no
/// foreign client id (venues that echo none).
pub fn find_ours(list: &[Resting], kind: Protective) -> Option<&Resting> {
    let same: Vec<&Resting> = list.iter().filter(|r| r.kind == kind && !r.foreign).collect();
    same.iter().find(|r| r.generation.is_some()).or_else(|| same.first()).copied()
}

/// Whether a full-size stop of `amount` (venue unit) must be re-synced to a
/// position of `size` (venue unit): more than half a lot apart.
pub fn needs_resync(amount: f64, size: f64, lot: f64) -> bool {
    let tol = if lot > 0.0 { lot / 2.0 } else { size.abs() * 1e-9 };
    (amount - size).abs() > tol
}

/// A filled protective order of ours in the closed-order history: the
/// stop (or take-profit) fired.
pub fn fired_in(caps: &VenueCaps, closed: &[Order], id: i64, kind: Protective, placed_ms: u64) -> bool {
    closed.iter().any(|o| {
        let filled = o.status.as_deref() == Some("closed") && o.filled.unwrap_or(0.0) > 0.0;
        let recent = o.timestamp.or(o.last_update_timestamp).map_or(true, |t| t >= placed_ms as i64);
        let ours = match generation_of(caps, id, o.client_order_id.as_deref()) {
            Some(_) => true,
            None if o.client_order_id.as_deref().is_some_and(|c| !c.is_empty()) => false,
            None => classify(o, None).is_some_and(|(k, _)| k == kind),
        };
        filled && recent && ours
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::venue::caps::caps;
    use ccxt::Value;

    fn order(j: serde_json::Value) -> Order {
        Order::from_value(Value::from(j))
    }

    #[test]
    fn protective_client_ids_are_distinct_per_generation_and_found_again() {
        for venue in ["bitget", "gate", "woo", "deribit"] {
            let c = caps(venue).unwrap();
            let id = 17_597_000_001_230;
            let ids: Vec<String> = (0..MAX_GENERATIONS).map(|g| protective_client_id(c, id, g)).collect();
            let mut uniq = ids.clone();
            uniq.sort();
            uniq.dedup();
            assert_eq!(uniq.len(), ids.len(), "{venue}");
            assert_eq!(generation_of(c, id, Some(&ids[3])), Some(3), "{venue}");
            assert_eq!(generation_of(c, id + 10, Some(&ids[3])), None, "{venue}: another stop");
            assert_eq!(generation_of(c, id, None), None);
        }
    }

    #[test]
    fn stop_params_ask_for_a_position_close_where_the_venue_has_one() {
        let get = |p: Params, k: &str| match p.into_value_object() {
            Value::Dict(d) => d.get(k).cloned().unwrap_or(Value::Null),
            _ => Value::Null,
        };
        let bingx = caps("bingx").unwrap();
        let p = || protective_params(bingx, Protective::Stop, 100.5, "c1");
        assert_eq!(get(p(), "closePosition"), Value::Bool(true));
        assert_eq!(get(p(), "reduceOnly"), Value::Null, "BingX refuses closePosition with reduceOnly");
        assert_eq!(get(p(), "stopLossPrice"), Value::Float(100.5));
        let bitget = caps("bitget").unwrap();
        assert_eq!(get(protective_params(bitget, Protective::Stop, 1.0, "c"), "reduceOnly"), Value::Bool(true));
        let gate = caps("gate").unwrap();
        let g = || protective_params(gate, Protective::TakeProfit, 2.0, "t-c");
        assert_eq!(get(g(), "reduceOnly"), Value::Bool(true));
        assert_eq!(get(g(), "takeProfitPrice"), Value::Float(2.0));
        assert_eq!(get(g(), "closeOrder"), Value::Null);
        assert_eq!(get(g(), "clientOrderId"), Value::from("t-c"));
    }

    #[test]
    fn orders_are_classified_by_their_own_fields_then_by_the_entry() {
        let long = Some(Held { amt: 0.5, entry: 100.0 });
        let sl = order(serde_json::json!({"id": "1", "symbol": "BTC/USDT:USDT", "status": "open", "side": "sell", "stopLossPrice": 95.0, "amount": 0.5}));
        assert_eq!(classify(&sl, None), Some((Protective::Stop, 95.0)));
        let tp = order(serde_json::json!({"id": "2", "symbol": "BTC/USDT:USDT", "status": "open", "side": "sell", "takeProfitPrice": 110.0}));
        assert_eq!(classify(&tp, None), Some((Protective::TakeProfit, 110.0)));
        let plain = |px: f64, side: &str| order(serde_json::json!({"id": "3", "symbol": "X", "status": "open", "side": side, "triggerPrice": px, "reduceOnly": true}));
        assert_eq!(classify(&plain(95.0, "sell"), long), Some((Protective::Stop, 95.0)));
        assert_eq!(classify(&plain(105.0, "sell"), long), Some((Protective::TakeProfit, 105.0)));
        assert_eq!(classify(&plain(95.0, "buy"), long), None, "a buy trigger cannot protect a long");
        let short = Some(Held { amt: -0.5, entry: 100.0 });
        assert_eq!(classify(&plain(105.0, "buy"), short), Some((Protective::Stop, 105.0)));
        assert_eq!(classify(&plain(95.0, "sell"), None), None, "unknown without a position");
        let opening = order(serde_json::json!({"id": "4", "symbol": "X", "status": "open", "side": "sell", "triggerPrice": 95.0, "reduceOnly": false}));
        assert_eq!(classify(&opening, long), None);
        let limit = order(serde_json::json!({"id": "5", "symbol": "X", "status": "open", "side": "sell", "price": 120.0}));
        assert_eq!(classify(&limit, long), None, "no trigger");
    }

    #[test]
    fn our_stop_is_found_by_client_id_before_any_other() {
        let c = caps("bitget").unwrap();
        let id = 17_597_000_001_230;
        let ours = protective_client_id(c, id, 2);
        let list = vec![
            order(serde_json::json!({"id": "a", "symbol": "X", "status": "open", "side": "sell", "stopLossPrice": 90.0, "clientOrderId": "user1", "amount": 1.0})),
            order(serde_json::json!({"id": "b", "symbol": "X", "status": "open", "side": "sell", "stopLossPrice": 95.0, "clientOrderId": ours, "amount": 1.0})),
            order(serde_json::json!({"id": "b", "symbol": "X", "status": "open", "side": "sell", "stopLossPrice": 95.0, "clientOrderId": ours})),
            order(serde_json::json!({"id": "c", "symbol": "X", "status": "canceled", "side": "sell", "stopLossPrice": 97.0})),
        ];
        let r = resting(c, &list, None, Some(id));
        assert_eq!(r.len(), 2, "duplicates and finished orders dropped");
        let found = find_ours(&r, Protective::Stop).unwrap();
        assert_eq!((found.venue_id.as_str(), found.generation), ("b", Some(2)));
        assert!(find_ours(&r, Protective::TakeProfit).is_none());
        // Only someone else's stop rests: not ours.
        let other = resting(c, &list[..1], None, Some(id));
        assert!(other[0].foreign);
        assert!(find_ours(&other, Protective::Stop).is_none());
        // A venue without client-id echo: the one resting stop is ours.
        let gate = caps("gate").unwrap();
        let bare = vec![order(serde_json::json!({"id": "g1", "symbol": "X", "status": "open", "side": "sell", "stopLossPrice": 95.0}))];
        let r = resting(gate, &bare, None, Some(id));
        assert_eq!(find_ours(&r, Protective::Stop).map(|r| r.venue_id.as_str()), Some("g1"));
    }

    #[test]
    fn a_full_size_stop_is_resynced_only_when_it_no_longer_covers_the_position() {
        assert!(!needs_resync(3.0, 3.0, 1.0));
        assert!(!needs_resync(0.3000000001, 0.3, 0.001), "float noise");
        assert!(needs_resync(3.0, 4.0, 1.0), "an add");
        assert!(needs_resync(3.0, 1.0, 1.0), "a partial close");
        assert!(needs_resync(3.0, 0.0, 1.0), "flat");
    }

    #[test]
    fn a_fired_stop_is_read_from_the_history() {
        let c = caps("deribit").unwrap();
        let id = 17_597_000_001_230;
        let placed = 1_759_700_000_123u64;
        let cid = protective_client_id(c, id, 0);
        let filled = |cid: &str, ts: i64| {
            order(serde_json::json!({"id": "9", "symbol": "X", "status": "closed", "filled": 1.0, "side": "sell", "stopLossPrice": 95.0, "clientOrderId": cid, "timestamp": ts}))
        };
        assert!(fired_in(c, &[filled(&cid, placed as i64 + 5)], id, Protective::Stop, placed));
        assert!(!fired_in(c, &[filled("someone", placed as i64 + 5)], id, Protective::Stop, placed));
        assert!(!fired_in(c, &[filled(&cid, placed as i64 - 5)], id, Protective::Stop, placed), "older than the stop");
        let bare = order(serde_json::json!({"id": "9", "symbol": "X", "status": "closed", "filled": 1.0, "side": "sell", "stopLossPrice": 95.0}));
        assert!(fired_in(c, &[bare.clone()], id, Protective::Stop, placed));
        assert!(!fired_in(c, &[bare], id, Protective::TakeProfit, placed));
    }
}
