//! Key-age and IP-restriction warnings. Warnings only: a key that passes the
//! permission check is stored either way; these say what would make a leak
//! of it less likely or less harmful.
//!
//! Why: the 3Commas leak (Dec 2022) drained accounts through keys WITHOUT
//! withdrawal permission, by wash trades, and the keys used were years old
//! and usable from any IP. A key bound to the user's IP is useless to whoever
//! copies it elsewhere; a key rotated every few months limits how long a
//! copy is worth anything.

use serde::{Deserialize, Serialize};

/// A key older than this is due for rotation.
pub const ROTATE_AFTER_DAYS: u64 = 90;
const DAY_MS: u64 = 86_400_000;

/// Where a key's age comes from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgeSource {
    /// The exchange's own creation date (Binance `createTime`).
    Exchange,
    /// When the key entered this vault (added or renewed): the exchange did
    /// not say, so the key may be older than this.
    #[default]
    Vault,
}

/// What the UI shows next to a key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeySafety {
    /// True only when the exchange said the key has NO IP restriction. An
    /// unknown answer (Bybit, OKX, a key not checked since 0.3.15) is false.
    pub no_ip_restriction: bool,
    /// Whole days since the key was created (see `age_from`).
    pub age_days: u64,
    pub age_from: AgeSource,
    /// `age_days` is over `ROTATE_AFTER_DAYS`.
    pub rotate: bool,
}

impl KeySafety {
    /// The one warning the live readiness check shows, worst first: an open,
    /// old key is the 3Commas case.
    pub fn worst_code(&self) -> Option<&'static str> {
        match (self.no_ip_restriction, self.rotate) {
            (true, true) => Some("keyNoIpAndOld"),
            (true, false) => Some("keyNoIpRestriction"),
            (false, true) => Some("keyOld"),
            (false, false) => None,
        }
    }
}

/// Classifies one key. `exchange_created` wins over `vault_date` (the add or
/// renewal date); a date of 0 or in the future counts as age 0.
pub fn classify(exchange_created: Option<u64>, vault_date: u64, ip_restricted: Option<bool>, now: u64) -> KeySafety {
    let (from_ms, age_from) = match exchange_created {
        Some(t) if t > 0 => (t, AgeSource::Exchange),
        _ => (vault_date, AgeSource::Vault),
    };
    let age_days = if from_ms == 0 { 0 } else { now.saturating_sub(from_ms) / DAY_MS };
    KeySafety {
        no_ip_restriction: ip_restricted == Some(false),
        age_days,
        age_from,
        rotate: age_days > ROTATE_AFTER_DAYS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;

    fn days_ago(d: u64) -> u64 {
        NOW - d * DAY_MS
    }

    #[test]
    fn the_exchange_date_wins_and_rotation_starts_after_90_days() {
        let s = classify(Some(days_ago(91)), days_ago(2), Some(true), NOW);
        assert_eq!((s.age_days, s.age_from, s.rotate), (91, AgeSource::Exchange, true));
        let s = classify(Some(days_ago(90)), days_ago(400), Some(true), NOW);
        assert_eq!((s.age_days, s.rotate), (90, false), "90 days is not yet over");
        assert_eq!(s.worst_code(), None);
    }

    #[test]
    fn without_an_exchange_date_the_vault_date_is_used() {
        let s = classify(None, days_ago(120), None, NOW);
        assert_eq!((s.age_days, s.age_from, s.rotate), (120, AgeSource::Vault, true));
        assert_eq!(s.worst_code(), Some("keyOld"));
        // A zero createTime is "not said", not 1970.
        assert_eq!(classify(Some(0), days_ago(3), None, NOW).age_from, AgeSource::Vault);
    }

    #[test]
    fn only_an_explicit_no_restriction_warns_about_ip() {
        assert!(classify(None, NOW, Some(false), NOW).no_ip_restriction);
        assert!(!classify(None, NOW, Some(true), NOW).no_ip_restriction);
        assert!(!classify(None, NOW, None, NOW).no_ip_restriction, "unknown is not a warning");
        assert_eq!(classify(None, NOW, Some(false), NOW).worst_code(), Some("keyNoIpRestriction"));
        assert_eq!(classify(Some(days_ago(365)), NOW, Some(false), NOW).worst_code(), Some("keyNoIpAndOld"));
    }

    #[test]
    fn odd_dates_do_not_raise_a_warning() {
        let future = classify(Some(NOW + DAY_MS), 0, None, NOW);
        assert_eq!((future.age_days, future.rotate), (0, false));
        let unknown = classify(None, 0, None, NOW);
        assert_eq!((unknown.age_days, unknown.rotate), (0, false));
    }
}
