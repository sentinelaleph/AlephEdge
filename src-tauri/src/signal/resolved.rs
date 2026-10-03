//! Signals Sentinel has already resolved. Pure (no I/O), so the rules are
//! unit-testable.
//!
//! The buffer keeps a signal until it expires, but Sentinel records the
//! outcome as soon as price reaches the target or the stop. Without this
//! check a bot entered such a signal hours later (the paper runner's FIGHTUSDT
//! short on 28 Sep 2026: target hit 07:30 UTC, entered 18:34, stopped out).
//! Sentinel's public pending list is exactly the published signals with no
//! outcome yet; a buffered signal it covers but no longer lists is resolved.

use std::collections::{HashSet, VecDeque};
use std::time::Duration;

use super::model::OpenStatus;

/// How often the pending list is re-read while the stream is up.
pub const SWEEP_EVERY: Duration = Duration::from_secs(5 * 60);
/// A snapshot older than this cannot vouch for a signal (three missed sweeps).
pub const FRESH_FOR_MS: u64 = 15 * 60_000;
/// A signal published this close to the snapshot may not be in it yet.
const GRACE_MS: u64 = 60_000;
/// Resolved ids remembered, so a later lookup still answers Resolved.
const RESOLVED_CAP: usize = 2000;

/// The last pending list read from Sentinel.
#[derive(Default)]
pub struct PendingSnapshot {
    ids: HashSet<String>,
    /// When the request was sent (ms); 0 = never read.
    asof_ms: u64,
    /// Signals created before this are older than a truncated page reaches.
    covered_from_ms: u64,
    resolved: VecDeque<String>,
}

impl PendingSnapshot {
    /// Records a pending page read at `asof_ms`. `complete` is false when
    /// Sentinel's total exceeds the rows returned; the page then covers only
    /// signals from `oldest_created_ms` on. Returns the ids in `buffered`
    /// (id, created ms) that the page covers but no longer lists.
    pub fn update(
        &mut self,
        page_ids: HashSet<String>,
        oldest_created_ms: Option<u64>,
        complete: bool,
        asof_ms: u64,
        buffered: &[(String, u64)],
    ) -> Vec<String> {
        self.ids = page_ids;
        self.asof_ms = asof_ms;
        self.covered_from_ms = if complete {
            0
        } else {
            oldest_created_ms.unwrap_or(u64::MAX)
        };
        let gone: Vec<String> = buffered
            .iter()
            .filter(|(id, created)| self.covers(*created) && !self.ids.contains(id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in &gone {
            if !self.resolved.contains(id) {
                if self.resolved.len() >= RESOLVED_CAP {
                    self.resolved.pop_front();
                }
                self.resolved.push_back(id.clone());
            }
        }
        gone
    }

    fn covers(&self, created_ms: u64) -> bool {
        created_ms >= self.covered_from_ms && created_ms + GRACE_MS <= self.asof_ms
    }

    /// Whether Sentinel still lists the signal as open, as far as the last
    /// snapshot can tell at `now_ms`.
    pub fn status(&self, id: &str, created_ms: u64, now_ms: u64) -> OpenStatus {
        if self.resolved.iter().any(|r| r == id) {
            return OpenStatus::Resolved;
        }
        if self.asof_ms == 0 || now_ms.saturating_sub(self.asof_ms) > FRESH_FOR_MS {
            return OpenStatus::Unknown;
        }
        if self.ids.contains(id) || created_ms + GRACE_MS > self.asof_ms {
            return OpenStatus::Open; // listed, or published after the snapshot
        }
        if self.covers(created_ms) {
            OpenStatus::Resolved
        } else {
            OpenStatus::Unknown // older than a truncated page reaches
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: u64 = 1_790_600_000_000;
    const MIN: u64 = 60_000;

    fn ids(v: &[&str]) -> HashSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn buf(v: &[(&str, u64)]) -> Vec<(String, u64)> {
        v.iter().map(|(id, c)| (id.to_string(), *c)).collect()
    }

    #[test]
    fn a_covered_signal_missing_from_the_list_is_resolved() {
        let mut s = PendingSnapshot::default();
        let gone = s.update(ids(&["open"]), Some(T - 600 * MIN), true, T,
            &buf(&[("open", T - 60 * MIN), ("won", T - 300 * MIN)]));
        assert_eq!(gone, vec!["won".to_string()]);
        assert_eq!(s.status("won", T - 300 * MIN, T + MIN), OpenStatus::Resolved);
        assert_eq!(s.status("open", T - 60 * MIN, T + MIN), OpenStatus::Open);
    }

    #[test]
    fn a_signal_published_around_the_snapshot_is_left_alone() {
        let mut s = PendingSnapshot::default();
        let gone = s.update(ids(&[]), None, true, T, &buf(&[("new", T - 30_000), ("later", T + MIN)]));
        assert!(gone.is_empty(), "inside the grace window or after the read");
        assert_eq!(s.status("later", T + MIN, T + 2 * MIN), OpenStatus::Open);
    }

    #[test]
    fn a_truncated_page_only_judges_what_it_covers() {
        let mut s = PendingSnapshot::default();
        let gone = s.update(ids(&["a"]), Some(T - 100 * MIN), false, T,
            &buf(&[("a", T - 50 * MIN), ("old", T - 500 * MIN), ("b", T - 80 * MIN)]));
        assert_eq!(gone, vec!["b".to_string()], "old is outside the page");
        assert_eq!(s.status("old", T - 500 * MIN, T + MIN), OpenStatus::Unknown);
    }

    #[test]
    fn a_stale_or_missing_snapshot_is_unknown_but_resolved_stays_resolved() {
        let mut s = PendingSnapshot::default();
        assert_eq!(s.status("x", T, T), OpenStatus::Unknown, "never read");
        s.update(ids(&["x"]), None, true, T, &buf(&[("gone", T - 90 * MIN)]));
        assert_eq!(s.status("x", T - MIN * 10, T + FRESH_FOR_MS + 1), OpenStatus::Unknown);
        assert_eq!(s.status("gone", T - 90 * MIN, T + FRESH_FOR_MS + 1), OpenStatus::Resolved);
    }
}
