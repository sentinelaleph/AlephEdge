//! The pending-signal set the bot engine reads each tick. Pure (no I/O) so
//! dedup, regeneration and expiry pruning are unit-testable.
//!
//! Keyed by signal id: the stream and the reconnect backfill deliver the same
//! signals, and a 20-slot FIFO (the old buffer) truncated a 300-row backfill
//! to its last 20 rows.

use std::collections::VecDeque;

use super::model::Signal;
use super::time::parse_rfc3339_ms;

/// Room for a full backfill page (300) plus live arrivals.
pub const BUFFER_CAP: usize = 500;
/// Superseded ids remembered so a later backfill cannot resurrect them.
const SUPERSEDED_CAP: usize = 1000;

#[derive(Default)]
pub struct SignalBuffer {
    signals: VecDeque<Signal>,
    superseded: VecDeque<String>,
}

/// What a push did. `superseded` lists the signals a regeneration dropped
/// (including the pushed one itself when a newer regeneration already exists).
#[derive(Default)]
pub struct PushOutcome {
    pub added: bool,
    pub superseded: Vec<Signal>,
}

fn created_ms(sig: &Signal) -> u64 {
    parse_rfc3339_ms(&sig.created_at).unwrap_or(0)
}

impl SignalBuffer {
    pub fn push(&mut self, sig: Signal, now_ms: u64) -> PushOutcome {
        let mut out = PushOutcome::default();
        if self.signals.iter().any(|s| s.id == sig.id) || self.superseded.contains(&sig.id) {
            return out;
        }
        let key = sig.market_key();
        let created = created_ms(&sig);
        // An older signal arriving after its regenerated replacement (typical
        // for backfill) is superseded on arrival.
        let newer_regen = self
            .signals
            .iter()
            .any(|s| s.regenerated && s.market_key() == key && created_ms(s) > created);
        if newer_regen {
            self.remember_superseded(&sig.id);
            out.superseded.push(sig);
            return out;
        }
        if sig.regenerated {
            let (dropped, kept): (Vec<_>, Vec<_>) = self
                .signals
                .drain(..)
                .partition(|s| s.market_key() == key && created_ms(s) <= created);
            self.signals = kept.into();
            for s in dropped {
                self.remember_superseded(&s.id);
                out.superseded.push(s);
            }
        }
        self.prune_expired(now_ms);
        if self.signals.len() >= BUFFER_CAP {
            // Evict the OLDEST by publication time, not the first inserted:
            // insertion order is arrival order, and a backfill can deliver a
            // newer signal before an older one.
            if let Some(oldest) = self
                .signals
                .iter()
                .enumerate()
                .min_by_key(|(_, s)| created_ms(s))
                .map(|(i, _)| i)
            {
                self.signals.remove(oldest);
            }
        }
        self.signals.push_back(sig);
        out.added = true;
        out
    }

    /// Expired signals can never fill, so they are the first to go — FIFO
    /// eviction alone would drop live pending signals under a large backfill.
    fn prune_expired(&mut self, now_ms: u64) {
        self.signals
            .retain(|s| parse_rfc3339_ms(&s.expires_at).is_some_and(|exp| exp > now_ms));
    }

    fn remember_superseded(&mut self, id: &str) {
        if self.superseded.len() >= SUPERSEDED_CAP {
            self.superseded.pop_front();
        }
        self.superseded.push_back(id.to_string());
    }

    /// Newest-first by publication time (`created_at`), the order the bots
    /// walk (owner rule 2026-09-23: the freshest setup claims a scarce slot).
    /// Insertion order alone was not that: the stream, the reconnect backfill
    /// and the 5-minute sweep all add signals, and the pending list arrives
    /// newest first. Ties keep the later arrival first; unparseable
    /// timestamps sort last.
    pub fn recent(&self) -> Vec<Signal> {
        let mut out: Vec<Signal> = self.signals.iter().rev().cloned().collect();
        out.sort_by_key(|s| std::cmp::Reverse(created_ms(s)));
        out
    }

    /// (id, created ms) of every buffered signal.
    pub fn id_created(&self) -> Vec<(String, u64)> {
        self.signals
            .iter()
            .map(|s| (s.id.clone(), created_ms(s)))
            .collect()
    }

    /// Removes the given ids; returns the signals removed.
    pub fn remove_ids(&mut self, ids: &[String]) -> Vec<Signal> {
        let (removed, kept): (Vec<_>, Vec<_>) =
            self.signals.drain(..).partition(|s| ids.contains(&s.id));
        self.signals = kept.into();
        removed
    }

    pub fn symbol_for(&self, id: &str) -> Option<String> {
        self.signals
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.symbol.clone())
    }
}
