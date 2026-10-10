//! Which signals each bot has decided, and which transient skips it has
//! already reported.
//!
//! A signal used to be marked "seen" before any check ran, so a momentary
//! condition (macro feed down, price unavailable, position cap full) refused
//! it forever. Now only a FINAL outcome judges a signal (opened, refused by a
//! static property, expired, superseded); transient skips retry every tick
//! until expiry and are reported once per (bot, signal, reason) so the feed
//! is not spammed.
//!
//! A verdict keeps the reason that produced it. A refusal that came from the
//! bot's own settings (a filter, the age limit, the take-profit target, the
//! sizing) is dropped when those settings change, so the new settings judge
//! the still-pending signal again. Before that, only an app restart (which
//! empties this memory-only ledger) re-checked them: the same signal opened
//! or not depending on when the app last restarted.

use std::collections::{HashMap, VecDeque};

use super::model::BotKind;

/// FIFO bound per bot. Above the signal buffer (500) so a still-buffered
/// signal never ages out of the ledger and gets re-judged.
const JUDGED_CAP: usize = 2000;
const NOTED_CAP: usize = 4000;

/// Final skip keys that follow from the bot's settings rather than from the
/// signal's lifecycle or a real order. Opened, expired, invalidated, vetoed,
/// crossed-level and live-order verdicts are never in this list: clearing
/// them could enter a signal twice or re-send a refused real order.
pub fn depends_on_settings(key: &str) -> bool {
    matches!(
        key,
        "belowConfidence"
            | "directionFiltered"
            | "symbolFiltered"
            | "comboFiltered"
            | "engineFiltered"
            | "spotOnlyExchange"
            | "tooOld"
            | "tpCustomInvalid"
            | "tpNotBeyondFill"
            | "sizingInvalidStop"
            | "liveStopBeyondLiquidation"
    )
}

#[derive(Default)]
pub struct JudgeLedger {
    /// (signal id, the skip key that judged it; None = opened, or seeded
    /// from the trade table / a restored position).
    judged: HashMap<BotKind, VecDeque<(String, Option<String>)>>,
    noted: HashMap<BotKind, VecDeque<(String, String)>>,
}

impl JudgeLedger {
    pub fn is_judged(&self, kind: BotKind, signal_id: &str) -> bool {
        self.judged
            .get(&kind)
            .is_some_and(|q| q.iter().any(|(id, _)| id == signal_id))
    }

    /// Records a final decision with no skip reason (opened, already traded,
    /// restored). Returns false when it was already judged.
    pub fn judge(&mut self, kind: BotKind, signal_id: &str) -> bool {
        self.push_judged(kind, signal_id, None)
    }

    /// Records a final skip verdict and the key that produced it.
    pub fn judge_for(&mut self, kind: BotKind, signal_id: &str, reason: &str) -> bool {
        self.push_judged(kind, signal_id, Some(reason))
    }

    fn push_judged(&mut self, kind: BotKind, signal_id: &str, reason: Option<&str>) -> bool {
        if self.is_judged(kind, signal_id) {
            return false;
        }
        let q = self.judged.entry(kind).or_default();
        if q.len() >= JUDGED_CAP {
            q.pop_front();
        }
        q.push_back((signal_id.to_string(), reason.map(str::to_string)));
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

    /// `kind`'s settings changed: forgets its settings-based refusals
    /// (`depends_on_settings`) and the once-only notes of those signals, so
    /// they are judged again under the new settings on the next tick.
    /// Lifecycle verdicts and other bots are untouched. Returns how many
    /// verdicts were dropped.
    pub fn clear_settings_verdicts(&mut self, kind: BotKind) -> usize {
        let Some(q) = self.judged.get_mut(&kind) else {
            return 0;
        };
        let mut cleared = Vec::new();
        q.retain(|(id, reason)| {
            let drop = reason.as_deref().is_some_and(depends_on_settings);
            if drop {
                cleared.push(id.clone());
            }
            !drop
        });
        if let Some(notes) = self.noted.get_mut(&kind) {
            notes.retain(|(id, _)| !cleared.contains(id));
        }
        cleared.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_settings_change_drops_only_that_bots_settings_verdicts() {
        let mut l = JudgeLedger::default();
        assert!(l.judge_for(BotKind::Futures, "dir", "directionFiltered"));
        assert!(l.judge_for(BotKind::Futures, "old", "tooOld"));
        assert!(l.judge_for(BotKind::Futures, "crossed", "fillCrossedLevel"));
        assert!(l.judge_for(BotKind::Futures, "sent", "liveOrderFailed"));
        assert!(l.judge(BotKind::Futures, "opened"));
        assert!(l.judge_for(BotKind::Spot, "dir", "directionFiltered"));
        assert!(l.note_once(BotKind::Futures, "dir", "capitalCap"));
        assert!(l.note_once(BotKind::Futures, "held", "invalidatedHeld"));

        assert_eq!(l.clear_settings_verdicts(BotKind::Futures), 2);
        assert!(!l.is_judged(BotKind::Futures, "dir"));
        assert!(!l.is_judged(BotKind::Futures, "old"));
        for kept in ["crossed", "sent", "opened"] {
            assert!(l.is_judged(BotKind::Futures, kept), "{kept}");
        }
        assert!(l.is_judged(BotKind::Spot, "dir"), "another bot keeps its verdict");
        assert!(l.note_once(BotKind::Futures, "dir", "capitalCap"), "a cleared signal's note shows again");
        assert!(!l.note_once(BotKind::Futures, "held", "invalidatedHeld"), "other notes stay once-only");
    }

    #[test]
    fn lifecycle_and_live_keys_are_not_settings() {
        for key in ["fillCrossedLevel", "expired", "invalidated", "vetoedByRegime", "liveOrderFailed", "incoherentGeometry", "spotLongOnly", "notPumpSignal"] {
            assert!(!depends_on_settings(key), "{key}");
        }
    }
}
