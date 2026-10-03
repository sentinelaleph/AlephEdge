//! The vault's idle budget: how long the decrypted keys are allowed to stay in
//! memory after the last time the vault was actually USED.
//!
//! Why this file exists at all (2026-09-20 pre-publication review, S4). Every
//! other property of the vault is strong — Argon2id at 64 MiB, AES-256-GCM, the
//! salt in the OS keychain, an owner-only 0600 file, `zeroize` on the buffers we
//! own. None of that matters for very long if one password entry leaves the keys
//! decrypted until the process exits, and this application is designed to sit
//! running unattended on a desk for days. `vault_lock` existed as a command from
//! the beginning; nothing ever called it.
//!
//! Two decisions are worth defending here:
//!
//! **The clock is injected.** A 30-minute timer that can only be tested by
//! waiting 30 minutes is a timer that is never tested. `Clock` has exactly one
//! method, so the test double is three lines and the production implementation
//! is three lines.
//!
//! **The clock is monotonic, not wall-clock.** `SystemTime` moves backwards
//! whenever NTP corrects a drifting machine or a user changes the timezone by
//! hand, and a backwards jump on a lock timer extends the budget rather than
//! shortening it — the failure lands on the insecure side. `Instant` cannot go
//! backwards, which is the only property this measurement needs.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A monotonic millisecond source. One method, so injecting it is cheap.
///
/// `Send + Sync` because the timer lives inside Tauri-managed state, which is
/// shared across the command threads and the watchdog task.
pub trait Clock: Send + Sync {
    /// Milliseconds since some fixed origin. Must never decrease.
    fn now_ms(&self) -> u64;
}

/// Production clock: `Instant` deltas from process start.
pub struct MonotonicClock {
    origin: Instant,
}

impl MonotonicClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for MonotonicClock {
    fn now_ms(&self) -> u64 {
        // Saturates rather than wraps; a u64 of milliseconds is ~584 million
        // years, so the cast is a formality either way.
        self.origin.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
}

/// Tracks when the vault was last used and whether the budget is spent.
///
/// It deliberately knows nothing about sessions, credentials or Tauri: it holds
/// one timestamp and one duration. The decision of what counts as a "use", and
/// what happens when the budget is spent, belongs to `VaultManager`.
pub struct IdleTimer {
    /// Settable from the UI (Settings → exchange keys → auto-lock), so it sits
    /// behind its own lock; the expiry rule reads it on every check.
    budget: Mutex<Duration>,
    clock: Box<dyn Clock>,
    /// `None` means "nothing to expire" — the vault is locked or was never
    /// unlocked. Distinguishing that from "used at time 0" matters: a locked
    /// vault with a stale stamp would read as permanently expired, and the
    /// watchdog would spend every tick re-locking an already locked vault.
    last_use_ms: Mutex<Option<u64>>,
}

impl IdleTimer {
    pub fn new(budget: Duration, clock: Box<dyn Clock>) -> Self {
        Self {
            budget: Mutex::new(budget),
            clock,
            last_use_ms: Mutex::new(None),
        }
    }

    /// The configured timer: budget from the environment, monotonic clock.
    pub fn from_config() -> Self {
        Self::new(
            Duration::from_secs(crate::app::endpoints::vault_idle_minutes() * 60),
            Box::new(MonotonicClock::new()),
        )
    }

    /// Records a use. Resets the budget from now.
    pub fn touch(&self) {
        *self.last_use_ms.lock().expect("idle mutex") = Some(self.clock.now_ms());
    }

    /// Forgets the last use — the vault is locked, there is nothing to expire.
    pub fn clear(&self) {
        *self.last_use_ms.lock().expect("idle mutex") = None;
    }

    /// True when the budget has been spent since the last recorded use.
    ///
    /// False when nothing was ever recorded, so a locked vault is never
    /// "expired" — it is already in the state expiry would produce.
    pub fn expired(&self) -> bool {
        let guard = self.last_use_ms.lock().expect("idle mutex");
        match *guard {
            // `saturating_sub` rather than a bare subtraction: `Clock` promises
            // monotonicity, and a broken implementation should read as "not yet
            // expired on this tick", not panic in a release build and silently
            // underflow in a debug one.
            Some(last) => {
                self.clock.now_ms().saturating_sub(last) >= self.budget().as_millis() as u64
            }
            None => false,
        }
    }

    /// The budget in whole minutes, for the UI to state.
    ///
    /// The user is told the number the backend actually enforces rather than a
    /// literal repeated in a translation file, so a changed `.env` cannot leave
    /// the panel advertising a timeout that is not in force.
    pub fn budget_minutes(&self) -> u64 {
        self.budget().as_secs() / 60
    }

    fn budget(&self) -> Duration {
        *self.budget.lock().expect("idle mutex")
    }

    /// A new budget, counted from the last use already recorded: shortening
    /// it on a vault idle for longer than the new budget locks it on the next
    /// check, which is the safe reading of "lock after N minutes".
    pub fn set_budget(&self, budget: Duration) {
        *self.budget.lock().expect("idle mutex") = budget;
    }
}

/// A clock the test drives by hand, so the 30-minute budget is exercised in
/// microseconds. Shared handle: the timer owns one clone, the test keeps
/// another and advances it.
#[cfg(test)]
#[derive(Clone, Default)]
pub(crate) struct TestClock {
    ms: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

#[cfg(test)]
impl TestClock {
    pub(crate) fn advance(&self, ms: u64) {
        self.ms.fetch_add(ms, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
impl Clock for TestClock {
    fn now_ms(&self) -> u64 {
        self.ms.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timer(budget_ms: u64) -> (IdleTimer, TestClock) {
        let clock = TestClock::default();
        (
            IdleTimer::new(Duration::from_millis(budget_ms), Box::new(clock.clone())),
            clock,
        )
    }

    #[test]
    fn a_timer_that_was_never_touched_is_not_expired() {
        // Otherwise every watchdog tick would try to re-lock a vault that has
        // never been unlocked.
        let (t, clock) = timer(1_000);
        clock.advance(10_000);
        assert!(!t.expired());
    }

    #[test]
    fn the_budget_expires_exactly_at_the_boundary() {
        let (t, clock) = timer(1_000);
        t.touch();
        clock.advance(999);
        assert!(!t.expired(), "one millisecond short must still be alive");
        clock.advance(1);
        assert!(
            t.expired(),
            "the budget is spent at the boundary, not after it"
        );
    }

    #[test]
    fn a_use_inside_the_budget_defers_expiry() {
        // This is the whole point of the design: idle means NOT USED, not
        // "unlocked a while ago".
        let (t, clock) = timer(1_000);
        t.touch();
        clock.advance(900);
        t.touch();
        clock.advance(900);
        assert!(!t.expired());
        clock.advance(100);
        assert!(t.expired());
    }

    #[test]
    fn clearing_puts_the_timer_back_to_having_nothing_to_expire() {
        let (t, clock) = timer(1_000);
        t.touch();
        clock.advance(5_000);
        assert!(t.expired());
        t.clear();
        assert!(!t.expired());
    }

    #[test]
    fn a_shorter_budget_applies_to_the_running_session() {
        let (t, clock) = timer(10_000);
        t.touch();
        clock.advance(2_000);
        assert!(!t.expired());
        t.set_budget(Duration::from_millis(1_000));
        assert!(t.expired(), "idle past the new budget locks now");
    }

    #[test]
    fn the_budget_is_reported_in_whole_minutes() {
        let t = IdleTimer::new(Duration::from_secs(30 * 60), Box::new(TestClock::default()));
        assert_eq!(t.budget_minutes(), 30);
    }
}
