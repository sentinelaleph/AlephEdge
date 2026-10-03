//! Real Sentinel signals, not hand-written ones.
//!
//! `tests/fixtures/sentinel_pending_2026-09-16.json` is the verbatim body of
//! `GET https://ribqa.com/api/public/signals/pending?limit=3` captured on
//! 2026-09-16 — three live shorts on the current geometry (stop 2.5×ATR, TP1
//! below 1R, management plan attached). Before the 2026-09-16 audit, every one
//! of them would have been refused by the old R:R ≥ 1.0 floor. These tests pin
//! that the desk now reads them, judges them tradable, and manages them the
//! way Sentinel's ledger does.

use super::backfill::{parse_pending_body, Row};
use super::model::{Direction, Signal};
use super::time::parse_rfc3339_ms;
use crate::bot::engine::geometry::{
    fill_decision, geometry_coherent, horizon_ms, risk_r, FillDecision,
};

const BODY: &str = include_str!("../../tests/fixtures/sentinel_pending_2026-09-16.json");

fn live_signals() -> Vec<Signal> {
    parse_pending_body(BODY)
        .expect("captured body parses")
        .into_iter()
        .map(|row| match row {
            Row::Ok(sig) => *sig,
            Row::Bad { symbol, error, .. } => {
                panic!("live signal {symbol} failed to parse: {error}")
            }
        })
        .collect()
}

#[test]
fn every_captured_live_signal_parses_with_its_plan() {
    let sigs = live_signals();
    assert_eq!(sigs.len(), 3);
    for sig in &sigs {
        assert_eq!(sig.direction, Direction::Short, "{}", sig.symbol);
        assert!(!sig.tp.is_empty(), "{}", sig.symbol);
        let plan = sig
            .management_plan
            .expect("live signals carry a management plan");
        assert_eq!(plan.breakeven_at_r, Some(0.5));
        assert_eq!(plan.partial_at_r, Some(1.0));
        assert_eq!(plan.partial_fraction, Some(0.5));
    }
}

#[test]
fn live_geometry_is_accepted_even_though_tp1_is_below_one_r() {
    for sig in live_signals() {
        let r = risk_r(&sig);
        let tp1_in_r = (sig.entry - sig.tp[0]).abs() / r;
        assert!(
            tp1_in_r < 1.0,
            "{} fixture should sit below 1R, got {tp1_in_r:.2}",
            sig.symbol
        );
        assert!(
            geometry_coherent(&sig),
            "{} must be judged tradable",
            sig.symbol
        );
        // The 1R partial is farther than TP1, so it can never fire: the whole
        // position exits at TP1 (the ordering bug Sentinel fixed the same day).
        let partial_nearer = 1.0 * r < (sig.tp[0] - sig.entry).abs();
        assert!(!partial_nearer, "{}", sig.symbol);
    }
}

#[test]
fn live_signals_fill_at_entry_wait_for_a_retrace_and_refuse_a_gap_through_the_stop() {
    for sig in live_signals() {
        let created = parse_rfc3339_ms(&sig.created_at).expect("created_at parses");
        // At the published entry, right after publication: opens.
        assert_eq!(
            fill_decision(&sig, sig.entry, created + 1_000),
            FillDecision::Fill,
            "{}",
            sig.symbol
        );
        // 1% below entry: the move already ran without us. Selling there is a
        // worse fill than published, so the desk does NOT chase it; like the
        // ledger's resting entry, it waits for price to come back to entry.
        let ran_away = sig.entry * 0.99;
        assert_eq!(
            fill_decision(&sig, ran_away, created + 1_000),
            FillDecision::Wait,
            "{}",
            sig.symbol
        );
        assert_eq!(
            fill_decision(&sig, ran_away, created + 10 * 60_000),
            FillDecision::Wait,
            "{}",
            sig.symbol
        );
        // Entry touched only by gapping through the stop: the price is unusable
        // and the signal is refused for good.
        let through_stop = sig.sl * 1.001;
        assert_eq!(
            fill_decision(&sig, through_stop, created + 10 * 60_000),
            FillDecision::Refuse,
            "{}",
            sig.symbol
        );
    }
}

#[test]
fn live_horizon_is_expiry_plus_72_hours() {
    for sig in live_signals() {
        let expires = parse_rfc3339_ms(&sig.expires_at).expect("expires_at parses");
        assert_eq!(horizon_ms(&sig), expires + 72 * 3_600_000, "{}", sig.symbol);
    }
}

/// Parses bodies captured from production right now. Opt-in: capture with
/// `curl -o $DIR/pending.json https://ribqa.com/api/public/signals/pending?limit=300`
/// (and `vetoed.json` from `/vetoed?limit=300`), then run with
/// `ALEPH_LIVE_DIR=$DIR cargo test -- --ignored current_production`.
#[test]
#[ignore]
fn current_production_pending_and_vetoed_bodies_parse() {
    let dir = std::env::var("ALEPH_LIVE_DIR").expect("ALEPH_LIVE_DIR");
    let pending = std::fs::read_to_string(format!("{dir}/pending.json")).expect("pending.json");
    let page = super::backfill::parse_pending_page(&pending).expect("pending parses");
    let bad: Vec<String> = page
        .rows
        .iter()
        .filter_map(|r| match r {
            Row::Bad { symbol, error, .. } => Some(format!("{symbol}: {error}")),
            Row::Ok(_) => None,
        })
        .collect();
    assert!(bad.is_empty(), "rows failed to parse: {bad:?}");
    assert!(!page.rows.is_empty());
    assert!(page.total.is_some(), "total missing: truncation undetectable");
    for row in &page.rows {
        if let Row::Ok(sig) = row {
            assert!(parse_rfc3339_ms(&sig.created_at).is_some(), "{}", sig.created_at);
            assert!(parse_rfc3339_ms(&sig.expires_at).is_some(), "{}", sig.expires_at);
        }
    }
    let vetoed = std::fs::read_to_string(format!("{dir}/vetoed.json")).expect("vetoed.json");
    let raw: serde_json::Value = serde_json::from_str(&vetoed).unwrap();
    let rows = raw["data"].as_array().unwrap().len();
    let events = super::backfill::parse_vetoed_body(&vetoed).expect("vetoed parses");
    eprintln!("pending rows {} total {:?}; vetoed rows {rows} -> regime vetoes {}", page.rows.len(), page.total, events.len());
    assert!(events.iter().all(|e| !e.signal_id.is_empty()));
}
