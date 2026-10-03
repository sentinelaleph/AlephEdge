//! Shared builders for the engine's pure-function tests.

use crate::bot::model::{BotConfig, BotKind, OpenPosition};
use crate::signal::model::{ManagementPlan, Signal};

use super::entry::new_position;
use super::take_profit::TpResolution;

pub fn cfg() -> BotConfig {
    BotConfig {
        kind: BotKind::Futures,
        exchange_id: "binance".into(),
        max_positions: 5,
        capital: 100.0,
        leverage: 1,
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

/// A signal built through the real deserialiser. Expires far in the future.
pub fn signal(direction: &str, entry: f64, tp1: f64, sl: f64) -> Signal {
    signal_at(direction, entry, tp1, sl, "2026-09-16T10:00:00Z")
}

pub fn signal_at(direction: &str, entry: f64, tp1: f64, sl: f64, created_at: &str) -> Signal {
    serde_json::from_value(serde_json::json!({
        "id": "sig-1", "symbol": "SOLUSDT", "timeframe": "4h",
        "direction": direction, "mode": "hybrid",
        "entry": entry, "tp": [tp1], "sl": sl, "confidence": 0.7,
        "confluence": [{"source": "ob", "score": 0.8, "weighted": 0.3}],
        "rr": 0.6,
        "expires_at": "2099-01-01T00:00:00Z", "created_at": created_at
    }))
    .expect("fixture signal parses")
}

pub fn plan(breakeven: f64, partial: f64, fraction: f64) -> ManagementPlan {
    ManagementPlan {
        breakeven_at_r: Some(breakeven),
        partial_at_r: Some(partial),
        partial_fraction: Some(fraction),
    }
}

/// A position filled exactly at the signal entry, 1x leverage.
pub fn position(sig: &Signal, plan: Option<ManagementPlan>) -> OpenPosition {
    let mut sig = sig.clone();
    sig.management_plan = plan;
    new_position(
        &cfg(),
        &sig,
        sig.entry,
        1,
        0,
        &TpResolution::signal_tp1(&sig),
    )
}
