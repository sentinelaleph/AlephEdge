//! The fresh-signal gate (owner rule 2026-09-23): a bot enters only signals at
//! most `max_signal_age_min` minutes old; 0 turns the gate off.

use super::btc_break::BtcRegime;
use super::entry_rules::{pre_price_gate, signal_age_min, GateInput};
use super::precheck_rules::is_final;
use super::test_fixtures::{cfg, signal};
use crate::signal::time::parse_rfc3339_ms;

fn gate_at(age_min: u64, limit: u32) -> Option<String> {
    let mut c = cfg();
    c.max_signal_age_min = limit;
    // The fixture expires in 2099, so only the age gate can refuse it.
    let s = signal("long", 100.0, 102.0, 98.0);
    let created = parse_rfc3339_ms(&s.created_at).expect("fixture created_at");
    let now = created + age_min * 60_000;
    let input = GateInput {
        cfg: &c,
        sig: &s,
        regime: BtcRegime::Normal,
        now_ms: now,
        invalidated: false,
        regime_vetoed: false,
        pump_allowed: true,
        futures_supported: true,
        holds_market: false,
    };
    pre_price_gate(&input).map(|k| k.key.to_string())
}

#[test]
fn fresh_signal_passes_and_stale_one_is_refused() {
    assert_eq!(gate_at(239, 240), None);
    assert_eq!(gate_at(240, 240), None);
    assert_eq!(gate_at(241, 240).as_deref(), Some("tooOld"));
}

#[test]
fn zero_turns_the_age_gate_off() {
    assert_eq!(gate_at(10_000, 0), None);
}

#[test]
fn too_old_is_final_and_age_reads_minutes() {
    assert!(is_final("tooOld"));
    let sig = signal("long", 100.0, 102.0, 98.0);
    let created = parse_rfc3339_ms(&sig.created_at).unwrap();
    assert_eq!(signal_age_min(&sig, created + 90 * 60_000), Some(90));
}

#[test]
fn new_bots_default_to_four_hours() {
    let c = crate::bot::model::BotConfig::new_default(crate::bot::model::BotKind::Futures, "binance");
    assert_eq!(c.max_signal_age_min, 240);
    let old: crate::bot::model::BotConfig = serde_json::from_value(serde_json::json!({
        "kind": "futures", "exchangeId": "binance", "maxPositions": 3, "capital": 100.0, "leverage": 2
    }))
    .unwrap();
    assert_eq!(old.max_signal_age_min, 240, "configs saved before the rule get it too");
}
