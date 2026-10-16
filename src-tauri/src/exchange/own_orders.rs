//! Every Binance order this app placed, by exchange id: the regular orders
//! (entries, closes, mirror orders) and the conditional (algo) stops and
//! take-profits. The foreign-trade check (`foreign_fill`) asks it "did this
//! app make the order that filled?".
//!
//! Recorded where the order layer gets Binance's answer (`live_orders.rs`),
//! so no engine call site can forget it. Persisted write-through by a sink
//! the app installs (the store's meta table): a crash right after an order
//! must not turn that order's fill into a false alarm after the restart.
//! Entries older than Binance's income history (three months) are dropped.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// Binance keeps income history for three months; older ids cannot match.
pub const KEEP_MS: u64 = 95 * 24 * 3_600_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OwnKind {
    /// A regular order (`orderId`).
    Order,
    /// A conditional order (`algoId`); it fills as a new regular order.
    Algo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnOrder {
    pub kind: OwnKind,
    pub id: i64,
    pub symbol: String,
    pub at_ms: u64,
    /// Algo only: the regular order it became once it triggered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_order_id: Option<i64>,
    /// Algo only: settled (triggered, or ended without triggering); it is
    /// not queried again.
    #[serde(default)]
    pub done: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OwnBook {
    pub orders: Vec<OwnOrder>,
}

impl OwnBook {
    fn prune(&mut self, now_ms: u64) {
        let floor = now_ms.saturating_sub(KEEP_MS);
        self.orders.retain(|o| o.at_ms >= floor);
    }

    /// Whether `order_id` on `symbol` is a regular order of this app, or the
    /// order one of its conditional orders became.
    pub fn owns_order(&self, symbol: &str, order_id: i64) -> bool {
        self.orders.iter().any(|o| {
            o.symbol == symbol
                && match o.kind {
                    OwnKind::Order => o.id == order_id,
                    OwnKind::Algo => o.actual_order_id == Some(order_id),
                }
        })
    }

    /// Conditional orders on `symbol` not yet settled: the ones that may
    /// have become the order behind an unknown fill.
    pub fn open_algos(&self, symbol: &str) -> Vec<i64> {
        self.orders
            .iter()
            .filter(|o| o.kind == OwnKind::Algo && !o.done && o.symbol == symbol)
            .map(|o| o.id)
            .collect()
    }
}

type Sink = Arc<dyn Fn(String) + Send + Sync>;

/// The shared registry. Cheap to record into from any build; it is persisted
/// and read only where a sink is installed (the live build).
#[derive(Default)]
pub struct OwnOrders {
    book: Mutex<OwnBook>,
    sink: Mutex<Option<Sink>>,
}

impl OwnOrders {
    /// Loads what an earlier run persisted (an unreadable body starts empty)
    /// and installs the sink every later change is written through.
    pub fn install(&self, persisted: Option<&str>, sink: Sink) {
        let loaded: OwnBook = persisted.and_then(|b| serde_json::from_str(b).ok()).unwrap_or_default();
        {
            let mut g = self.book.lock().expect("own orders mutex");
            // Anything recorded before the install (none in practice) is kept.
            let mut merged = loaded;
            for o in g.orders.drain(..) {
                if !merged.orders.iter().any(|m| m.kind == o.kind && m.id == o.id && m.symbol == o.symbol) {
                    merged.orders.push(o);
                }
            }
            *g = merged;
        }
        *self.sink.lock().expect("own orders mutex") = Some(sink);
    }

    pub fn snapshot(&self) -> OwnBook {
        self.book.lock().expect("own orders mutex").clone()
    }

    pub fn record_order(&self, symbol: &str, order_id: i64, now_ms: u64) {
        if order_id > 0 {
            self.record(OwnKind::Order, symbol, order_id, now_ms);
        }
    }

    pub fn record_algo(&self, symbol: &str, algo_id: i64, now_ms: u64) {
        if algo_id > 0 {
            self.record(OwnKind::Algo, symbol, algo_id, now_ms);
        }
    }

    fn record(&self, kind: OwnKind, symbol: &str, id: i64, now_ms: u64) {
        self.change(now_ms, |b| {
            if b.orders.iter().any(|o| o.kind == kind && o.id == id && o.symbol == symbol) {
                return false;
            }
            b.orders.push(OwnOrder { kind, id, symbol: symbol.to_string(), at_ms: now_ms, actual_order_id: None, done: false });
            true
        });
    }

    /// Settles a conditional order: the order it became (Some), or ended
    /// without triggering (None).
    pub fn settle_algo(&self, symbol: &str, algo_id: i64, actual_order_id: Option<i64>, now_ms: u64) {
        self.change(now_ms, |b| {
            match b.orders.iter_mut().find(|o| o.kind == OwnKind::Algo && o.id == algo_id && o.symbol == symbol) {
                Some(o) => {
                    o.actual_order_id = actual_order_id;
                    o.done = true;
                }
                // An algo id known only from the book (placed by an older
                // version): kept from now on.
                None => b.orders.push(OwnOrder {
                    kind: OwnKind::Algo,
                    id: algo_id,
                    symbol: symbol.to_string(),
                    at_ms: now_ms,
                    actual_order_id,
                    done: true,
                }),
            }
            true
        });
    }

    fn change(&self, now_ms: u64, f: impl FnOnce(&mut OwnBook) -> bool) {
        let body = {
            let mut g = self.book.lock().expect("own orders mutex");
            if !f(&mut g) {
                return;
            }
            g.prune(now_ms);
            serde_json::to_string(&*g).ok()
        };
        let sink = self.sink.lock().expect("own orders mutex").clone();
        if let (Some(sink), Some(body)) = (sink, body) {
            sink(body);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_answer_ownership_and_survive_a_restart_through_the_sink() {
        let saved = Arc::new(Mutex::new(String::new()));
        let own = OwnOrders::default();
        let s = saved.clone();
        own.install(None, Arc::new(move |b| *s.lock().unwrap() = b));
        own.record_order("ETHUSDT", 11, 1_000);
        own.record_order("ETHUSDT", 11, 1_001); // once
        own.record_algo("ETHUSDT", 900, 1_002);
        own.record_order("ETHUSDT", 0, 1_003); // no id: nothing
        let b = own.snapshot();
        assert_eq!(b.orders.len(), 2);
        assert!(b.owns_order("ETHUSDT", 11));
        assert!(!b.owns_order("BTCUSDT", 11), "ids are per symbol");
        assert!(!b.owns_order("ETHUSDT", 12));
        assert_eq!(b.open_algos("ETHUSDT"), vec![900]);

        own.settle_algo("ETHUSDT", 900, Some(77), 2_000);
        assert!(own.snapshot().owns_order("ETHUSDT", 77), "the stop's fill is ours");
        assert!(own.snapshot().open_algos("ETHUSDT").is_empty());

        // The next run starts from what the sink wrote.
        let again = OwnOrders::default();
        again.install(Some(&saved.lock().unwrap()), Arc::new(|_| {}));
        assert_eq!(again.snapshot(), own.snapshot());
        // An unreadable body starts empty rather than failing.
        let empty = OwnOrders::default();
        empty.install(Some("{not json"), Arc::new(|_| {}));
        assert!(empty.snapshot().orders.is_empty());
    }

    #[test]
    fn ids_older_than_the_income_history_are_dropped() {
        let own = OwnOrders::default();
        own.record_order("ETHUSDT", 1, 0);
        own.record_order("ETHUSDT", 2, KEEP_MS + 5);
        let b = own.snapshot();
        assert!(!b.owns_order("ETHUSDT", 1));
        assert!(b.owns_order("ETHUSDT", 2));
    }
}
