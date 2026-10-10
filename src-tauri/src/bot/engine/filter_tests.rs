//! Evidence-filter tests. The combo/engine whitelists are the desk's lever for
//! acting on Sentinel's published per-combo and per-engine records, so the
//! contract that matters is: an empty list trades everything, a non-empty list
//! trades ONLY its members, and every refusal is reported with a reason.

use super::filters::passes_filters;
use crate::bot::model::{BotConfig, BotKind};
use crate::signal::model::{ConfluenceItem, Direction, Signal};

fn cfg() -> BotConfig {
    BotConfig {
        kind: BotKind::Futures,
        exchange_id: "binance".into(),
        max_positions: 2,
        capital: 100.0,
        leverage: 2,
        min_confidence: None,
        direction: None,
        symbols: vec![],
        combos: vec![],
        engines: vec![],
        live: false,
        max_loss_pct: None,
        sizing: crate::bot::model::SizingMode::Fixed,
        risk_per_trade_pct: 1.0,
        take_profit: Default::default(),
        take_profit_overrides: Default::default(),
        // Fixtures carry fixed dates; the age gate has its own tests.
        max_signal_age_min: 0,
    }
}

fn sig(combo: Option<&str>, sources: &[&str]) -> Signal {
    Signal {
        id: "s1".into(),
        symbol: "BTCUSDT".into(),
        timeframe: None,
        market_type: None,
        direction: Direction::Long,
        mode: "hybrid".into(),
        entry: 100.0,
        tp: vec![110.0],
        sl: 95.0,
        confidence: 0.9,
        regime: None,
        confluence: sources
            .iter()
            .map(|s| ConfluenceItem {
                source: (*s).into(),
                score: 0.8,
                weighted: 0.3,
            })
            .collect(),
        combo: combo.map(str::to_string),
        rr: 2.0,
        investment_score: None,
        regenerated: false,
        regenerated_at: None,
        management_plan: None,
        instrumentation: None,
        expires_at: "2026-01-01T00:00:00Z".into(),
        created_at: "2026-01-01T00:00:00Z".into(),
    }
}

#[test]
fn empty_lists_trade_everything() {
    let c = cfg();
    assert!(passes_filters(&c, &sig(Some("stophunt_snap"), &["fvg"])).is_none());
    assert!(passes_filters(&c, &sig(None, &[])).is_none());
}

#[test]
fn combo_whitelist_admits_only_its_members() {
    let mut c = cfg();
    c.combos = vec!["stophunt_snap".into(), "breaker_flip".into()];

    assert!(passes_filters(&c, &sig(Some("stophunt_snap"), &["fvg"])).is_none());

    let skip = passes_filters(&c, &sig(Some("fvg_continuation"), &["fvg"])).expect("refused");
    assert_eq!(skip.key, "comboFiltered");
    assert_eq!(skip.detail.as_deref(), Some("fvg_continuation"));
}

// A generic signal matched no combo at all. It can never satisfy a whitelist,
// and the feed must say "none" rather than name a combo that does not exist.
#[test]
fn combo_whitelist_refuses_a_signal_with_no_combo() {
    let mut c = cfg();
    c.combos = vec!["stophunt_snap".into()];

    for empty in [None, Some("")] {
        let skip = passes_filters(&c, &sig(empty, &["fvg"])).expect("refused");
        assert_eq!(skip.key, "comboFiltered");
        assert_eq!(skip.detail.as_deref(), Some("none"));
    }
}

// Sentinel keys its per-engine ledger on the PRIMARY (first contributing)
// source, so the filter must match that and not merely "appears anywhere" —
// otherwise it would select a different population than the published numbers.
#[test]
fn engine_whitelist_matches_the_primary_source_only() {
    let mut c = cfg();
    c.engines = vec!["fvg".into()];

    assert!(passes_filters(&c, &sig(None, &["fvg", "ema"])).is_none());

    let skip = passes_filters(&c, &sig(None, &["ema", "fvg"])).expect("refused");
    assert_eq!(skip.key, "engineFiltered");
    assert_eq!(skip.detail.as_deref(), Some("ema"));
}

#[test]
fn engine_whitelist_is_case_insensitive_and_reports_no_engine() {
    let mut c = cfg();
    c.engines = vec!["FVG".into()];
    assert!(passes_filters(&c, &sig(None, &["fvg"])).is_none());

    let skip = passes_filters(&c, &sig(None, &[])).expect("refused");
    assert_eq!(skip.key, "engineFiltered");
    assert_eq!(skip.detail.as_deref(), Some("none"));
}

// Pre-existing filters must keep firing first; the new ones are additive.
#[test]
fn earlier_filters_still_take_precedence() {
    let mut c = cfg();
    c.symbols = vec!["ETHUSDT".into()];
    c.combos = vec!["stophunt_snap".into()];

    let skip = passes_filters(&c, &sig(Some("breaker_flip"), &["fvg"])).expect("refused");
    assert_eq!(skip.key, "symbolFiltered");
}
