//! Reconnection policy for the relay connection.
//!
//! # Why this is a policy and not a `sleep`
//!
//! When the desk loses its connection to the relay the user **cannot see**
//! it: the phone says "your desk is off", but there is no such sign at the
//! computer. Reconnecting happens where the user does not notice it, which is
//! exactly why it must be a written rule.
//!
//! There are two failure classes, and **treating them the same** is the
//! mistake to avoid:
//!
//! - **Transient** (no network, relay restarting): retried forever. A desk
//!   that gives up is a desk that died silently.
//! - **Permanent** (membership invalid, desk id conflict, protocol version
//!   mismatch): retrying is useless. Retrying forever only burns the relay and
//!   hides the real problem: the user looks at "connecting" and waits for the
//!   problem to clear by itself.

use std::time::Duration;

/// First retry delay.
pub const BACKOFF_START: Duration = Duration::from_secs(1);
/// Upper bound. Waiting longer would keep the desk off needlessly long once
/// the network returns.
pub const BACKOFF_MAX: Duration = Duration::from_secs(60);

/// Why the connection dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disconnect {
    /// No network, DNS did not resolve, relay down, socket dropped.
    Transient(String),
    /// Membership could not be verified.
    Unauthorized,
    /// This desk id is already connected (another machine or an old process).
    DeskConflict,
    /// The relay speaks a protocol version we do not.
    VersionMismatch,
    /// The pairing QR died before any phone used it: the key is invalid
    /// (`DeskLink::key_live`). Reconnecting is useless; a new QR is needed.
    PairingExpired,
}

impl Disconnect {
    /// Whether retrying makes sense.
    fn retryable(&self) -> bool {
        matches!(self, Disconnect::Transient(_))
    }
}

/// The policy's decision on what to do after a disconnect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// Wait this long, then retry.
    RetryIn(Duration),
    /// Stop trying and show the user THIS message.
    ///
    /// The message is shown in the UI, not a spinner saying "connecting…".
    Stop { reason: Disconnect, message: String },
}

/// Reconnection state.
#[derive(Debug, Default)]
pub struct Reconnect {
    attempt: u32,
}

impl Reconnect {
    pub fn new() -> Self {
        Self::default()
    }

    /// Called once a connection is established: resets the counter.
    ///
    /// Without the reset, a long-running desk that dropped once would wait
    /// the full 60 seconds straight away.
    pub fn connected(&mut self) {
        self.attempt = 0;
    }

    /// Decides what to do after a disconnect.
    pub fn on_disconnect(&mut self, why: &Disconnect) -> Next {
        if !why.retryable() {
            return Next::Stop {
                reason: why.clone(),
                // A stable code; the UI shows it under `errors.*`, in the
                // user's language, saying what to do.
                message: match why {
                    Disconnect::Unauthorized => "linkUnauthorized".into(),
                    Disconnect::DeskConflict => "linkDeskConflict".into(),
                    Disconnect::VersionMismatch => "linkVersionMismatch".into(),
                    Disconnect::PairingExpired => "pairingExpired".into(),
                    Disconnect::Transient(_) => unreachable!(),
                },
            };
        }

        // Exponential backoff, held at the cap. NO jitter: a single desk has
        // no thundering herd, and being deterministic keeps the behaviour
        // testable.
        let secs = BACKOFF_START
            .as_secs()
            .saturating_mul(1u64 << self.attempt.min(6));
        self.attempt = self.attempt.saturating_add(1);
        Next::RetryIn(Duration::from_secs(secs.min(BACKOFF_MAX.as_secs())))
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transient() -> Disconnect {
        Disconnect::Transient("no network".into())
    }

    /// A desk that gives up is a desk that died silently: a transient error
    /// is retried forever.
    #[test]
    fn transient_error_is_retried_forever() {
        let mut r = Reconnect::new();
        for _ in 0..200 {
            match r.on_disconnect(&transient()) {
                Next::RetryIn(_) => {}
                Next::Stop { .. } => panic!("gave up on a transient error"),
            }
        }
    }

    #[test]
    fn backoff_is_exponential_and_capped() {
        let mut r = Reconnect::new();
        let mut seen = Vec::new();
        for _ in 0..10 {
            if let Next::RetryIn(d) = r.on_disconnect(&transient()) {
                seen.push(d.as_secs());
            }
        }
        assert_eq!(&seen[..7], &[1, 2, 4, 8, 16, 32, 60]);
        // Held at the cap; it never grows past it.
        assert!(seen.iter().all(|&s| s <= BACKOFF_MAX.as_secs()));
    }

    /// See `Reconnect::connected`.
    #[test]
    fn counter_resets_once_connected() {
        let mut r = Reconnect::new();
        for _ in 0..5 {
            r.on_disconnect(&transient());
        }
        r.connected();
        assert_eq!(r.attempt(), 0);
        assert_eq!(r.on_disconnect(&transient()), Next::RetryIn(BACKOFF_START));
    }

    /// Retrying a permanent error forever hides the REAL problem behind a
    /// "connecting" spinner (see the module doc).
    #[test]
    fn permanent_errors_are_not_retried_and_say_why() {
        for why in [
            Disconnect::Unauthorized,
            Disconnect::DeskConflict,
            Disconnect::VersionMismatch,
            Disconnect::PairingExpired,
        ] {
            let mut r = Reconnect::new();
            match r.on_disconnect(&why) {
                Next::Stop { reason, message } => {
                    assert_eq!(reason, why);
                    assert!(!message.is_empty(), "reason came back with an empty message: {why:?}");
                    // Every reason has its own stable code; the text saying
                    // what to do lives in the UI translations (errors.link*).
                    let expected = match why {
                        Disconnect::Unauthorized => "linkUnauthorized",
                        Disconnect::DeskConflict => "linkDeskConflict",
                        Disconnect::PairingExpired => "pairingExpired",
                        _ => "linkVersionMismatch",
                    };
                    assert_eq!(message, expected);
                }
                Next::RetryIn(_) => panic!("permanent error was retried: {why:?}"),
            }
        }
    }

    /// A desk conflict in particular must not be retried: if two processes
    /// take turns connecting, neither works and the user sees neither.
    #[test]
    fn desk_conflict_is_not_retried() {
        let mut r = Reconnect::new();
        assert!(matches!(
            r.on_disconnect(&Disconnect::DeskConflict),
            Next::Stop { .. }
        ));
    }
}
