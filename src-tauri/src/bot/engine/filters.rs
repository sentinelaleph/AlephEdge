//! Bot-level signal filters (PRD §5.2-5) — cheap, run before any network call.
//!
//! Extracted from `precheck.rs` when the evidence filters were added: the
//! combined block would have pushed that file past the 300-line rule.
//!
//! Every refusal returns a `Skip` with a key the UI renders, never a silent
//! drop (PRD §5.4). The combo/engine filters exist because Sentinel's own
//! published ledger separates its engines by a wide margin — a desk that can
//! see "this combo wins, that engine bleeds" but cannot act on it is only
//! half a product.

use crate::signal::model::{Direction, Signal};

use super::super::model::{BotConfig, BotKind};
use super::precheck::Skip;

/// The primary confluence source: the FIRST contributing item, which is what
/// Sentinel keys its per-engine outcome ledger on. Matching on anything else
/// would filter a different population than the published numbers describe.
fn primary_source(sig: &Signal) -> Option<&str> {
    sig.confluence
        .iter()
        .find(|item| item.weighted > 0.0 || item.score > 0.0)
        .map(|item| item.source.as_str())
}

fn list_contains(list: &[String], value: &str) -> bool {
    list.iter().any(|entry| entry.eq_ignore_ascii_case(value))
}

pub fn passes_filters(cfg: &BotConfig, sig: &Signal) -> Option<Skip> {
    if cfg.kind == BotKind::Spot && sig.direction == Direction::Short {
        return Some(Skip::new("spotLongOnly"));
    }
    if let Some(min) = cfg.min_confidence {
        if sig.confidence < min {
            return Some(Skip::with(
                "belowConfidence",
                format!("{:.0}%", sig.confidence * 100.0),
            ));
        }
    }
    if let Some(dir) = cfg.direction.as_deref() {
        let want_long = dir.eq_ignore_ascii_case("long");
        let is_long = sig.direction == Direction::Long;
        if !dir.eq_ignore_ascii_case("all") && want_long != is_long {
            return Some(Skip::new("directionFiltered"));
        }
    }
    if !cfg.symbols.is_empty() && !cfg.symbols.iter().any(|s| s.eq_ignore_ascii_case(&sig.symbol)) {
        return Some(Skip::new("symbolFiltered"));
    }
    if !cfg.combos.is_empty() {
        // A signal that matched no combo can never satisfy a combo whitelist —
        // reported with its own detail so the feed distinguishes "wrong combo"
        // from "no combo at all".
        match sig.combo.as_deref().filter(|c| !c.is_empty()) {
            None => return Some(Skip::with("comboFiltered", "none".into())),
            Some(combo) if !list_contains(&cfg.combos, combo) => {
                return Some(Skip::with("comboFiltered", combo.to_string()));
            }
            Some(_) => {}
        }
    }
    if !cfg.engines.is_empty() {
        match primary_source(sig) {
            None => return Some(Skip::with("engineFiltered", "none".into())),
            Some(source) if !list_contains(&cfg.engines, source) => {
                return Some(Skip::with("engineFiltered", source.to_string()));
            }
            Some(_) => {}
        }
    }
    None
}
