//! The stream supervisor's connection state, kept apart from error text.
//!
//! Pure (clock passed in) so every transition is unit-tested without a
//! network or a Tauri runtime.

use super::model::StreamPhase;

/// Stable code stored in `last_error` when no access token reaches the
/// stream client; the UI maps it to a localized "sign in again".
pub const NO_ACCESS_TOKEN: &str = "noAccessToken";
/// Stable code stored when the server closed the stream without an error.
pub const STREAM_CLOSED: &str = "streamClosed";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamState {
    phase: StreamPhase,
    /// Start of the current not-live episode (connect request or drop), ms.
    since_ms: u64,
    last_error: Option<String>,
}

impl Default for StreamState {
    fn default() -> Self {
        Self {
            phase: StreamPhase::Idle,
            since_ms: 0,
            last_error: None,
        }
    }
}

impl StreamState {
    pub fn phase(&self) -> StreamPhase {
        self.phase
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn is_live(&self) -> bool {
        self.phase == StreamPhase::Live
    }

    /// Seconds out of `Live`; None while live or idle.
    pub fn not_live_secs(&self, now_ms: u64) -> Option<u32> {
        match self.phase {
            StreamPhase::Idle | StreamPhase::Live => None,
            _ => Some((now_ms.saturating_sub(self.since_ms) / 1000) as u32),
        }
    }

    /// No connection wanted: clears any error.
    pub fn idle(&mut self) {
        *self = Self::default();
    }

    /// An attempt starts. From idle or live this opens a new episode in
    /// `Connecting`; a retry keeps its phase, its error and its clock, so a
    /// failing stream does not flip back to a harmless-looking "connecting".
    pub fn attempt(&mut self, now_ms: u64) {
        match self.phase {
            StreamPhase::Idle | StreamPhase::Live => {
                self.phase = StreamPhase::Connecting;
                self.since_ms = now_ms;
                self.last_error = None;
            }
            StreamPhase::Down => {
                // The blocker (token) is gone if we got here; keep the clock.
                self.phase = StreamPhase::Retrying;
            }
            StreamPhase::Connecting | StreamPhase::Retrying => {}
        }
    }

    /// The ticket was issued and the stream is open.
    pub fn live(&mut self, now_ms: u64) {
        self.phase = StreamPhase::Live;
        self.since_ms = now_ms;
        self.last_error = None;
    }

    /// A real failure; the supervisor will retry.
    pub fn failed(&mut self, error: String, now_ms: u64) {
        self.open_episode(now_ms);
        self.phase = StreamPhase::Retrying;
        self.last_error = Some(error);
    }

    /// The session ended. A live stream that closed without reporting an
    /// error is still a drop; a failure already recorded is kept.
    pub fn ended(&mut self, now_ms: u64) {
        if self.phase == StreamPhase::Live {
            self.failed(STREAM_CLOSED.to_string(), now_ms);
        }
    }

    /// Needs the user (no access token).
    pub fn blocked(&mut self, code: &str, now_ms: u64) {
        self.open_episode(now_ms);
        self.phase = StreamPhase::Down;
        self.last_error = Some(code.to_string());
    }

    fn open_episode(&mut self, now_ms: u64) {
        if matches!(self.phase, StreamPhase::Idle | StreamPhase::Live) {
            self.since_ms = now_ms;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_normal_connect_is_never_an_error() {
        let mut s = StreamState::default();
        assert_eq!(s.phase(), StreamPhase::Idle);
        s.attempt(1_000);
        assert_eq!(s.phase(), StreamPhase::Connecting);
        assert_eq!(s.last_error(), None, "progress must not be reported as an error");
        assert_eq!(s.not_live_secs(6_000), Some(5));
        s.live(7_000);
        assert!(s.is_live());
        assert_eq!(s.last_error(), None);
        assert_eq!(s.not_live_secs(9_000), None);
    }

    #[test]
    fn a_failure_retries_and_keeps_its_clock_and_error() {
        let mut s = StreamState::default();
        s.attempt(0);
        s.failed("signalTicketRefused|503".into(), 2_000);
        assert_eq!(s.phase(), StreamPhase::Retrying);
        // Next attempt: still retrying, error kept, clock from the first attempt.
        s.attempt(5_000);
        assert_eq!(s.phase(), StreamPhase::Retrying);
        assert_eq!(s.last_error(), Some("signalTicketRefused|503"));
        assert_eq!(s.not_live_secs(65_000), Some(65));
        s.live(66_000);
        assert_eq!(s.phase(), StreamPhase::Live);
        assert_eq!(s.last_error(), None);
    }

    #[test]
    fn a_drop_from_live_opens_a_new_episode() {
        let mut s = StreamState::default();
        s.attempt(0);
        s.live(1_000);
        s.failed("signalStreamIdle".into(), 100_000);
        assert_eq!(s.phase(), StreamPhase::Retrying);
        assert_eq!(s.not_live_secs(103_000), Some(3), "clock restarts at the drop");
    }

    #[test]
    fn a_clean_close_while_live_is_a_drop_but_a_failure_is_not_overwritten() {
        let mut s = StreamState::default();
        s.attempt(0);
        s.live(0);
        s.ended(10_000);
        assert_eq!(s.phase(), StreamPhase::Retrying);
        assert_eq!(s.last_error(), Some(STREAM_CLOSED));

        let mut s = StreamState::default();
        s.attempt(0);
        s.failed("signalTicketRefused|401".into(), 0);
        s.ended(1_000);
        assert_eq!(s.last_error(), Some("signalTicketRefused|401"));
    }

    #[test]
    fn no_token_is_down_until_an_attempt_can_start() {
        let mut s = StreamState::default();
        s.blocked(NO_ACCESS_TOKEN, 0);
        assert_eq!(s.phase(), StreamPhase::Down);
        assert_eq!(s.last_error(), Some(NO_ACCESS_TOKEN));
        s.attempt(3_000);
        assert_eq!(s.phase(), StreamPhase::Retrying);
        assert_eq!(s.not_live_secs(3_000), Some(3));
    }

    #[test]
    fn idle_clears_everything() {
        let mut s = StreamState::default();
        s.attempt(0);
        s.failed("x".into(), 0);
        s.idle();
        assert_eq!(s, StreamState::default());
        assert_eq!(s.not_live_secs(99_000), None);
    }
}
