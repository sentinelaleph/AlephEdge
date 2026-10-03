//! BTC-break guard: turns Sentinel's own regime declaration into an ENTRY
//! filter.
//!
//! `/api/v1/market/btc-macro` exposes `btcd_veto_long` (the flag Sentinel's
//! pipeline uses to stop publishing alt longs) and the `pulse_alert` impulse
//! detector. While either reads a break, new LONG entries are refused (skip
//! `btcBreakGuard`). Shorts are NOT refused: the flag is a veto on alt longs
//! during BTC weakness, which is exactly when Sentinel publishes shorts
//! (≈96% of the book the week of 2026-09-16).
//!
//! While the feed is unreachable the regime is Unknown and both directions
//! are refused (never enter blind). Both refusals are transient: the signal
//! retries on later ticks until it expires.
//!
//! The guard never closes positions. A regime-triggered close recreated the
//! server guard's close arm, measured at −163 pts and OFF on the server.

use crate::market::model::BtcMacro;
use crate::signal::model::Direction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtcRegime {
    Normal,
    Break,
    /// Macro feed unreachable — regime genuinely unknown.
    Unknown,
}

impl BtcRegime {
    pub fn as_str(self) -> &'static str {
        match self {
            BtcRegime::Normal => "normal",
            BtcRegime::Break => "break",
            BtcRegime::Unknown => "unknown",
        }
    }
}

/// Classifies Sentinel's macro state. `None` = fetch failed.
pub fn classify(state: Option<&BtcMacro>) -> BtcRegime {
    match state {
        None => BtcRegime::Unknown,
        Some(m) if m.btcd_veto_long || m.pulse_alert.eq_ignore_ascii_case("dumping") => {
            BtcRegime::Break
        }
        Some(_) => BtcRegime::Normal,
    }
}

/// The skip key when `regime` refuses an entry in `direction`, else None.
pub fn entry_block(regime: BtcRegime, direction: Direction) -> Option<&'static str> {
    match (regime, direction) {
        (BtcRegime::Unknown, _) => Some("btcGuardUnknown"),
        (BtcRegime::Break, Direction::Long) => Some("btcBreakGuard"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn macro_state(veto: bool, pulse: &str) -> BtcMacro {
        BtcMacro {
            price: 65000.0,
            change_pct: 0.0,
            trend: "neutral".into(),
            trend_strength: 0.0,
            market_phase: String::new(),
            btcd_veto_long: veto,
            pulse_alert: pulse.into(),
        }
    }

    #[test]
    fn classify_maps_declaration_not_inference() {
        assert_eq!(classify(None), BtcRegime::Unknown);
        assert_eq!(
            classify(Some(&macro_state(false, "neutral"))),
            BtcRegime::Normal
        );
        assert_eq!(
            classify(Some(&macro_state(true, "neutral"))),
            BtcRegime::Break
        );
        assert_eq!(
            classify(Some(&macro_state(false, "dumping"))),
            BtcRegime::Break
        );
        // Pumping is not a break — the guard must not block a rally.
        assert_eq!(
            classify(Some(&macro_state(false, "pumping"))),
            BtcRegime::Normal
        );
    }
}
