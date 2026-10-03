//! Server-side invalidation (veto) handling: the protection layer that voids a
//! signal after publication (PRD §1/§3.3 — btc_trend_flip etc.). Split from the
//! manager so the stream/lifecycle file stays within size limits; these methods
//! share `SignalManager`'s private state by design.

use super::model::{RegimeVetoEvent, StreamNoteKind, VetoEvent};
use super::SignalManager;

/// Un-surfaced veto notes waiting for the bot desk to drain them into the
/// visible skip feed. Bounded so an idle desk can't grow it without limit.
const PENDING_VETO_CAP: usize = 50;
/// FIFO-bounded generously so a void never ages out while its position is open.
const INVALIDATED_CAP: usize = 500;

impl SignalManager {
    /// Records a server-side invalidation. The payload is parsed leniently
    /// ({"signal_id"}|{"id"}, plus optional "symbol"/"reason") so a wire-format
    /// tweak upstream degrades gracefully rather than crashing. Beyond marking
    /// the id for the close brake, it queues a veto note (symbol + reason) for
    /// the visible skip feed — the symbol comes from the frame or, failing that,
    /// is resolved from the recent buffer.
    pub(super) fn mark_invalidated(&self, payload: &str) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) else {
            return;
        };
        let Some(id) = v
            .get("signal_id")
            .or_else(|| v.get("id"))
            .and_then(|s| s.as_str().map(String::from))
        else {
            return;
        };

        let mut q = self.invalidated.lock().expect("signal mutex");
        if q.iter().any(|existing| existing == &id) {
            return; // already voided — don't double-count or re-surface.
        }
        // FIFO-bounded — old ids age out one at a time, never a full wipe that
        // could drop a void still applying to an open position.
        if q.len() >= INVALIDATED_CAP {
            q.pop_front();
        }
        q.push_back(id.clone());
        drop(q);

        let reason = v
            .get("reason")
            .or_else(|| v.get("invalidation_reason"))
            .and_then(|s| s.as_str())
            .unwrap_or_default()
            .to_string();
        let symbol = v
            .get("symbol")
            .and_then(|s| s.as_str().map(String::from))
            .unwrap_or_else(|| self.symbol_for(&id));
        self.queue_veto(VetoEvent {
            signal_id: id,
            symbol,
            reason,
            kind: StreamNoteKind::Veto,
        });
        self.touch();
    }

    /// Resolves a signal id to its symbol from the recent buffer; falls back to
    /// "—" when the signal has already aged out (still surfaced, not dropped).
    fn symbol_for(&self, signal_id: &str) -> String {
        self.buffer
            .lock()
            .expect("signal mutex")
            .symbol_for(signal_id)
            .unwrap_or_else(|| "—".to_string())
    }

    pub(super) fn queue_veto(&self, veto: VetoEvent) {
        let mut q = self.pending_vetoes.lock().expect("signal mutex");
        if q.len() >= PENDING_VETO_CAP {
            q.pop_front();
        }
        q.push_back(veto);
    }

    /// Takes and clears the veto notes accumulated since the last call. The bot
    /// desk drains these each status poll and pushes them into the skip feed, so
    /// vetoes surface even when no bot loop is ticking.
    pub fn drain_pending_vetoes(&self) -> Vec<VetoEvent> {
        self.pending_vetoes
            .lock()
            .expect("signal mutex")
            .drain(..)
            .collect()
    }

    /// True when the server has voided this signal: never ENTER it. An open
    /// position is no longer closed on it — the server-side guard close was
    /// measured to destroy value (−163 pts) and is OFF upstream.
    pub fn is_invalidated(&self, signal_id: &str) -> bool {
        self.invalidated
            .lock()
            .expect("signal mutex")
            .iter()
            .any(|id| id == signal_id)
    }

    /// Records a NEW-contract "veto" frame (owner decision, 2026-09-18):
    /// unlike `mark_invalidated`'s legacy frame, this one ALSO closes any open
    /// position on the signal immediately — so beyond bookkeeping (which
    /// blocks entry, see `is_regime_vetoed`) it spawns the close right here,
    /// on the app handle `connect()` captured, rather than waiting for the
    /// next tick. A malformed payload is surfaced as a parse-error note and
    /// otherwise dropped — never a panic. A duplicate delivery for a signal
    /// already recorded still (re)spawns the close attempt — that side is
    /// idempotent on its own (the close claim in `bot::engine::book` refuses
    /// to close an already-closed or already-closing position) — but is not
    /// re-stored or re-noted.
    pub(super) fn mark_regime_veto(&self, payload: &str) {
        let event: RegimeVetoEvent = match serde_json::from_str(payload) {
            Ok(e) => e,
            Err(e) => {
                self.queue_note(
                    String::new(),
                    "—".into(),
                    format!("vetoPayloadMalformed|{e}"),
                    StreamNoteKind::ParseError,
                );
                return;
            }
        };

        // `record` itself is pure/testable (see regime_veto.rs); only the
        // note + close-spawn below are SignalManager-specific side effects.
        if self.regime_vetoes.record(event.clone()) {
            self.queue_veto(VetoEvent {
                signal_id: event.signal_id.clone(),
                symbol: event.symbol.clone(),
                reason: veto_note_reason(&event),
                kind: StreamNoteKind::RegimeVeto,
            });
        }
        self.touch();

        if let Some(app) = self.app.get() {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                crate::bot::engine::book::close_on_veto(&app, &event).await;
            });
        }
    }

    /// True when Sentinel's NEW veto event (2026-09-18) voided this signal:
    /// never ENTER it. Distinct from `is_invalidated` — a veto also closes any
    /// open position (handled at receipt, see `mark_regime_veto`), while the
    /// legacy invalidation only blocks entry.
    pub fn is_regime_vetoed(&self, signal_id: &str) -> bool {
        self.regime_vetoes.contains(signal_id)
    }

    /// The veto recorded for `signal_id`, if any (the tick's close backstop).
    pub fn regime_veto(&self, signal_id: &str) -> Option<RegimeVetoEvent> {
        self.regime_vetoes.get(signal_id)
    }

    /// Records vetoes learned from the public vetoed list on (re)connect —
    /// the ones issued while the stream was down. Only recorded (entry is
    /// blocked); the tick loop's backstop closes any position still open on
    /// them. Returns how many were new.
    pub(super) fn record_backfilled_vetoes(&self, events: Vec<RegimeVetoEvent>) -> usize {
        let mut new = 0;
        for event in events {
            let note = VetoEvent {
                signal_id: event.signal_id.clone(),
                symbol: event.symbol.clone(),
                reason: veto_note_reason(&event),
                kind: StreamNoteKind::RegimeVeto,
            };
            if self.regime_vetoes.record(event) {
                self.queue_veto(note);
                new += 1;
            }
        }
        new
    }
}

/// The note detail for a regime veto: Sentinel's stable code, which the UI
/// localises (`bots.vetoReason.<code>`); the server's English sentence only
/// when no code came with it.
pub(crate) fn veto_note_reason(event: &RegimeVetoEvent) -> String {
    if event.reason_code.trim().is_empty() {
        event.reason_text.clone()
    } else {
        event.reason_code.clone()
    }
}

#[cfg(test)]
mod note_reason_tests {
    use super::veto_note_reason;
    use crate::signal::model::RegimeVetoEvent;

    fn event(code: &str, text: &str) -> RegimeVetoEvent {
        serde_json::from_value(serde_json::json!({
            "signal_id": "s1", "symbol": "XUSDT", "reason_code": code, "reason_text": text
        }))
        .unwrap()
    }

    // The desk shows vetoes in the user's language: it gets the code, not
    // Sentinel's English sentence ("BTC turned bearish" on a Turkish screen).
    #[test]
    fn the_note_carries_the_code_and_falls_back_to_the_text() {
        assert_eq!(veto_note_reason(&event("btc_regime_turn_bearish", "BTC turned bearish")), "btc_regime_turn_bearish");
        assert_eq!(veto_note_reason(&event("", "BTC turned bearish")), "BTC turned bearish");
    }
}
