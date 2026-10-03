//! Which signals each bot has decided, and which transient skips it has
//! already reported.
//!
//! A signal used to be marked "seen" before any check ran, so a momentary
//! condition (macro feed down, price unavailable, position cap full) refused
//! it forever. Now only a FINAL outcome judges a signal (opened, refused by a
//! static property, expired, superseded); transient skips retry every tick
//! until expiry and are reported once per (bot, signal, reason) so the feed
//! is not spammed.

use std::collections::{HashMap, VecDeque};

use super::model::BotKind;

/// FIFO bound per bot. Above the signal buffer (500) so a still-buffered
/// signal never ages out of the ledger and gets re-judged.
const JUDGED_CAP: usize = 2000;
const NOTED_CAP: usize = 4000;

#[derive(Default)]
pub struct JudgeLedger {
    judged: HashMap<BotKind, VecDeque<String>>,
    noted: HashMap<BotKind, VecDeque<(String, String)>>,
}

impl JudgeLedger {
    pub fn is_judged(&self, kind: BotKind, signal_id: &str) -> bool {
        self.judged
            .get(&kind)
            .is_some_and(|q| q.iter().any(|id| id == signal_id))
    }

    /// Records a final decision. Returns false when it was already judged.
    pub fn judge(&mut self, kind: BotKind, signal_id: &str) -> bool {
        if self.is_judged(kind, signal_id) {
            return false;
        }
        let q = self.judged.entry(kind).or_default();
        if q.len() >= JUDGED_CAP {
            q.pop_front();
        }
        q.push_back(signal_id.to_string());
        true
    }

    /// True the first time this (bot, signal, reason) is noted — the caller
    /// pushes the visible skip only then.
    pub fn note_once(&mut self, kind: BotKind, signal_id: &str, reason: &str) -> bool {
        let q = self.noted.entry(kind).or_default();
        if q.iter().any(|(id, r)| id == signal_id && r == reason) {
            return false;
        }
        if q.len() >= NOTED_CAP {
            q.pop_front();
        }
        q.push_back((signal_id.to_string(), reason.to_string()));
        true
    }
}
