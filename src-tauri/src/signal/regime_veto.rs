//! Bounded store of signals Sentinel vetoed via the NEW "veto" event
//! (2026-09-18 — BTC's regime turned against them). Split out of
//! `SignalManager` (mirrors `bot::close_claim`'s `CloseClaims`) so the dedup
//! and FIFO-bounding logic is pure and unit-testable without a live manager.

use std::collections::VecDeque;
use std::sync::Mutex;

use super::model::RegimeVetoEvent;

/// FIFO-bounded the same as `SignalManager`'s legacy `invalidated` store — a
/// void still applying to an open position is never wiped en masse.
const CAP: usize = 500;

#[derive(Default)]
pub struct RegimeVetoStore(Mutex<VecDeque<RegimeVetoEvent>>);

impl RegimeVetoStore {
    /// Records `event` unless its signal id is already stored. Returns `true`
    /// when newly recorded — the caller then queues the visible note and
    /// spawns the close. A duplicate delivery returns `false` and is
    /// otherwise a no-op here; the close side's own idempotency (never
    /// closing the same position twice) lives in `bot::engine::book`'s close
    /// claim, not in this store.
    pub fn record(&self, event: RegimeVetoEvent) -> bool {
        let mut q = self.0.lock().expect("signal mutex");
        if q.iter().any(|r| r.signal_id == event.signal_id) {
            return false;
        }
        if q.len() >= CAP {
            q.pop_front();
        }
        q.push_back(event);
        true
    }

    /// True when Sentinel vetoed this signal: never ENTER it.
    pub fn contains(&self, signal_id: &str) -> bool {
        self.0
            .lock()
            .expect("signal mutex")
            .iter()
            .any(|r| r.signal_id == signal_id)
    }

    /// The stored veto for `signal_id`, if any: the tick loop's backstop
    /// closes a still-open position on it (see `book::close_phase`).
    pub fn get(&self, signal_id: &str) -> Option<RegimeVetoEvent> {
        self.0
            .lock()
            .expect("signal mutex")
            .iter()
            .find(|r| r.signal_id == signal_id)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::model::Direction;

    fn event(id: &str) -> RegimeVetoEvent {
        RegimeVetoEvent {
            signal_id: id.into(),
            symbol: "ETHUSDT".into(),
            direction: Some(Direction::Short),
            reason_code: "btc_regime_turn_bullish".into(),
            reason_text: "BTC turned bullish for 60 min".into(),
            trigger: "regime_turn".into(),
            btc_price: 77_541.9,
            btc_trend: "bullish".into(),
            btc_strength: 0.21,
            btc_change_1h_pct: 1.4,
            vetoed_at: "2026-09-18T11:02:03Z".into(),
        }
    }

    #[test]
    fn records_once_and_reports_which_delivery_was_new() {
        let store = RegimeVetoStore::default();
        assert!(!store.contains("sig-1"));
        assert!(store.record(event("sig-1")), "first delivery is new");
        assert!(store.contains("sig-1"));
        assert!(
            !store.record(event("sig-1")),
            "duplicate delivery is not new"
        );
        assert!(
            store.contains("sig-1"),
            "still recorded after the duplicate"
        );
    }

    #[test]
    fn distinct_signals_are_independent() {
        let store = RegimeVetoStore::default();
        store.record(event("sig-1"));
        assert!(!store.contains("sig-2"));
        assert!(store.record(event("sig-2")), "a different signal is new");
    }

    #[test]
    fn bounded_fifo_evicts_the_oldest_first() {
        let store = RegimeVetoStore::default();
        for i in 0..(CAP + 5) {
            store.record(event(&format!("sig-{i}")));
        }
        assert!(!store.contains("sig-0"), "oldest evicted");
        assert!(!store.contains("sig-4"), "still within the evicted range");
        assert!(store.contains(&format!("sig-{}", CAP + 4)), "newest kept");
    }
}
