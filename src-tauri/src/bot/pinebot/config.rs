//! A saved Pine bot as the store keeps it (`bot::commands::apply_config`):
//! the script's hash recomputed from its text, symbols upper case and
//! unique, Ignore entries dropped from the trigger map, margin per position
//! = budget / positions, fixed sizing (an entry's size is the bot's), and a
//! Live bot disarmed by any edit of what its arm confirmed.

use crate::bot::model::{BotConfig, BotKind, PineAction, SizingMode, TradeMode};

pub fn normalize_config(prev: Option<&BotConfig>, mut next: BotConfig) -> Result<BotConfig, String> {
    if next.kind != BotKind::Pine {
        next.pine = None;
        return Ok(next);
    }
    let mut p = next.pine.take().ok_or_else(|| "pineSettingsMissing".to_string())?;
    p.name = p.name.trim().to_string();
    p.script_hash = crate::pine::script_hash(&p.script);
    let mut symbols: Vec<String> = Vec::new();
    for s in p.symbols.iter().map(|s| s.trim().to_ascii_uppercase()).filter(|s| !s.is_empty()) {
        if !symbols.contains(&s) {
            symbols.push(s);
        }
    }
    p.symbols = symbols;
    p.triggers.retain(|k, a| *a != PineAction::Ignore && !k.trim().is_empty());
    let title = |t: Option<String>| t.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    p.stop_input = title(p.stop_input.take());
    p.target_input = title(p.target_input.take());
    p.validate()?;
    if next.max_positions == 0 {
        return Err("botMaxPositionsMin".to_string());
    }
    next.capital = ((p.budget_usdt / f64::from(next.max_positions)) * 100.0).floor() / 100.0;
    next.sizing = SizingMode::Fixed;
    next.symbols = p.symbols.clone();
    let prev = prev.filter(|c| c.kind == BotKind::Pine);
    // D4: a Live bot never loses its own stop by a settings save.
    if prev.is_some_and(|c| c.mode == TradeMode::Live) || next.mode == TradeMode::Live {
        if let Some(code) = p.live_refusal() {
            return Err(code.to_string());
        }
    }
    next.pine = Some(p);
    // An edit of the script, its triggers, inputs, bars or exits on an armed
    // bot disarms it: the user types LIVE again for the new script.
    // (apply_config keeps the arm through `live_terms_changed`, which reads
    // the same comparison.)
    if prev.is_some_and(|c| c.live && script_terms_changed(c, &next)) {
        next.live = false;
    }
    Ok(next)
}

/// Whether what a Pine bot's Live arm confirmed changed between two saves:
/// its budget and size per entry, or the script's terms
/// (`PineSettings::same_script_terms`). Any other kind: false.
pub fn script_terms_changed(prev: &BotConfig, next: &BotConfig) -> bool {
    if prev.kind != BotKind::Pine && next.kind != BotKind::Pine {
        return false;
    }
    match (prev.pine.as_ref(), next.pine.as_ref()) {
        (Some(a), Some(b)) => !a.same_script_terms(b) || (a.budget_usdt, a.max_size_usdt) != (b.budget_usdt, b.max_size_usdt),
        (None, None) => false,
        _ => true,
    }
}
