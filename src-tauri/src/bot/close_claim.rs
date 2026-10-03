//! Advisory close claim (2026-09-18): makes the tick loop's own close and an
//! event-triggered veto close mutually exclusive on one position, so the two
//! can never both record a close for it — Sentinel's "veto" event closes a
//! position the instant it arrives, off the tick's own 4s schedule, and the
//! ordinary close phase is still evaluating positions at the same time.

use std::collections::HashSet;
use std::sync::Mutex;

use super::model::BotKind;

#[derive(Default)]
pub struct CloseClaims(Mutex<HashSet<(String, BotKind)>>);

impl CloseClaims {
    /// Claims the exclusive right to close this position now. `false` means
    /// another close already owns it — the caller must back off and leave the
    /// position for that other close to finish.
    pub fn claim(&self, signal_id: &str, kind: BotKind) -> bool {
        self.0
            .lock()
            .expect("bot mutex")
            .insert((signal_id.to_string(), kind))
    }

    /// Releases a claim, win or lose, so a position left open (a failed close
    /// retries) is claimable again next tick.
    pub fn release(&self, signal_id: &str, kind: BotKind) {
        self.0
            .lock()
            .expect("bot mutex")
            .remove(&(signal_id.to_string(), kind));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_claim_is_refused_until_released() {
        let c = CloseClaims::default();
        assert!(c.claim("sig-1", BotKind::Futures));
        assert!(!c.claim("sig-1", BotKind::Futures), "already claimed");
        c.release("sig-1", BotKind::Futures);
        assert!(
            c.claim("sig-1", BotKind::Futures),
            "claimable again after release"
        );
    }

    #[test]
    fn different_bot_kinds_on_the_same_signal_are_independent() {
        let c = CloseClaims::default();
        assert!(c.claim("sig-1", BotKind::Futures));
        assert!(
            c.claim("sig-1", BotKind::Spot),
            "a different bot kind is a different claim"
        );
    }

    #[test]
    fn different_signals_are_independent() {
        let c = CloseClaims::default();
        assert!(c.claim("sig-1", BotKind::Futures));
        assert!(c.claim("sig-2", BotKind::Futures));
    }
}
