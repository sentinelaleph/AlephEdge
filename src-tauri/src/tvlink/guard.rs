//! The TradingView bot's own limits and the alert guards, as pure functions
//! over plain values: the duplicate guard, the price-move guard, the size /
//! position / budget / daily-open / leverage / allow-list limits, and the
//! symbol mapping onto the bot's market. tasks.rs gathers the values and
//! calls these before any order; nothing here reads the network or the
//! clock.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::bot::model::{BotConfig, TvSettings};

/// The same body or the same `id` within this window is refused.
pub const DUPLICATE_WINDOW_MS: u64 = 5 * 60_000;
pub const DUPLICATE: &str = "tvDuplicate";
pub const PRICE_MOVED: &str = "tvPriceMoved";
pub const PRICE_REQUIRED: &str = "tvPriceRequired";
pub const SIZE_ABOVE_MAX: &str = "tvSizeAboveMax";
pub const POSITIONS_FULL: &str = "tvPositionsFull";
pub const BUDGET_FULL: &str = "tvBudgetFull";
pub const DAILY_OPENS_FULL: &str = "tvDailyOpensFull";
pub const NOT_ALLOWED: &str = "tvSymbolNotAllowed";
pub const PASSPHRASE: &str = "tvPassphraseWrong";
/// The bot still holds a 0.3.19 passphrase hash, which is no longer checked.
pub const PASSPHRASE_RESET: &str = "tvPassphraseReset";
pub const NOT_LISTED: &str = "tradeSymbolNotListed";

/// One alert the duplicate guard remembers (no body, only its hash).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SeenAlert {
    pub hash: String,
    pub tag: Option<String>,
    /// Server receive time (UNIX ms).
    pub at_ms: u64,
}

/// 16 hex characters of SHA-256 of the trimmed body.
pub fn body_hash(body: &str) -> String {
    Sha256::digest(body.trim().as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Whether an alert with `hash` / `tag` received at `at_ms` repeats one of
/// `seen` within the window (either way round: clocks of two relays' rows
/// may tie).
pub fn is_duplicate(seen: &[SeenAlert], hash: &str, tag: Option<&str>, at_ms: u64) -> bool {
    seen.iter().any(|s| {
        let close = s.at_ms.abs_diff(at_ms) <= DUPLICATE_WINDOW_MS;
        close && (s.hash == hash || (tag.is_some() && s.tag.as_deref() == tag))
    })
}

/// Remembers one alert and forgets those older than the window.
pub fn remember(seen: &mut Vec<SeenAlert>, entry: SeenAlert) {
    let newest = entry.at_ms.max(seen.iter().map(|s| s.at_ms).max().unwrap_or(0));
    seen.push(entry);
    seen.retain(|s| newest.saturating_sub(s.at_ms) <= DUPLICATE_WINDOW_MS);
}

/// The price-move guard: the live price more than `guard_pct` away from
/// the alert's `price` refuses (`tvPriceMoved|0.73%`). No price: refused
/// when the bot requires one, else nothing to compare.
pub fn price_refusal(alert_price: Option<f64>, live: f64, guard_pct: f64, require: bool) -> Result<(), String> {
    let Some(p) = alert_price else {
        return if require { Err(PRICE_REQUIRED.to_string()) } else { Ok(()) };
    };
    let moved = (live - p).abs() / p * 100.0;
    if moved > guard_pct + 1e-9 {
        return Err(format!("{PRICE_MOVED}|{moved:.2}%"));
    }
    Ok(())
}

/// What the bot holds and has done, for `limit_refusal`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Usage {
    /// Open positions of the TradingView bot.
    pub open: usize,
    /// Their margin (USDT).
    pub margin_usdt: f64,
    /// Alert entries made today (UTC).
    pub opens_today: u32,
}

/// The TradingView bot's own limits on one entry of `size_usdt` notional:
/// the allow list, the per-alert size cap, the daily entry cap, the
/// position cap and the budget (margin = notional / leverage). A flip
/// checks after its close leg, so the closed position is no longer in
/// `use_`.
pub fn limit_refusal(cfg: &BotConfig, tv: &TvSettings, symbol: &str, size_usdt: f64, leverage: u8, use_: Usage) -> Result<(), String> {
    if !cfg.symbols.is_empty() && !cfg.symbols.iter().any(|s| s.eq_ignore_ascii_case(symbol)) {
        return Err(NOT_ALLOWED.to_string());
    }
    if size_usdt > tv.max_size_usdt + 1e-9 {
        return Err(format!("{SIZE_ABOVE_MAX}|{:.2}", tv.max_size_usdt));
    }
    if use_.opens_today >= tv.max_opens_per_day {
        return Err(format!("{DAILY_OPENS_FULL}|{}", tv.max_opens_per_day));
    }
    let (open, used) = (use_.open, use_.margin_usdt);
    if open >= usize::from(cfg.max_positions) {
        return Err(format!("{POSITIONS_FULL}|{}", cfg.max_positions));
    }
    let margin = size_usdt / f64::from(leverage.max(1));
    if used + margin > tv.budget_usdt + 0.005 {
        return Err(format!("{BUDGET_FULL}|{:.2}", (tv.budget_usdt - used).max(0.0)));
    }
    Ok(())
}

/// Binance's multiplier prefixes on futures symbols, largest first.
const MULTIPLIERS: [(&str, f64); 4] = [("1000000", 1e6), ("1M", 1e6), ("10000", 1e4), ("1000", 1e3)];

/// The symbol on the bot's market and the factor its prices take: as
/// given when listed (or nothing is listed to check against), else the
/// renamed multiplier form (`PEPEUSDT` from a spot chart is `1000PEPEUSDT`
/// on futures, prices x1000; and back). Not found: `tradeSymbolNotListed`.
pub fn map_symbol(symbol: &str, listed: &[String]) -> Result<(String, f64), String> {
    let has = |s: &str| listed.iter().any(|l| l.eq_ignore_ascii_case(s));
    if listed.is_empty() || has(symbol) {
        return Ok((symbol.to_string(), 1.0));
    }
    for (prefix, m) in MULTIPLIERS {
        let renamed = format!("{prefix}{symbol}");
        if has(&renamed) {
            return Ok((renamed, m));
        }
        if let Some(rest) = symbol.strip_prefix(prefix) {
            if rest.len() > 4 && has(rest) {
                return Ok((rest.to_string(), 1.0 / m));
            }
        }
    }
    Err(NOT_LISTED.to_string())
}

/// UTC day start (ms) of `ms`.
pub fn utc_day(ms: u64) -> u64 {
    ms - ms % 86_400_000
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::model::BotKind;

    const T: u64 = 1_760_000_000_000;

    fn tv() -> (BotConfig, TvSettings) {
        let mut cfg = BotConfig::new_default(BotKind::Tradingview, "binance");
        cfg.max_positions = 2;
        let tv = TvSettings { budget_usdt: 40.0, max_size_usdt: 60.0, max_opens_per_day: 3, ..TvSettings::default() };
        (cfg, tv)
    }

    #[test]
    fn the_same_body_or_id_within_five_minutes_is_a_duplicate() {
        let mut seen = Vec::new();
        let h = body_hash(r#"{"action":"buy"}"#);
        assert_eq!(h, body_hash("  {\"action\":\"buy\"}\n"), "whitespace around the body does not matter");
        assert_eq!(h.len(), 16);
        remember(&mut seen, SeenAlert { hash: h.clone(), tag: Some("a1".into()), at_ms: T });
        assert!(is_duplicate(&seen, &h, None, T + 1_000));
        assert!(is_duplicate(&seen, &h, None, T + DUPLICATE_WINDOW_MS));
        assert!(!is_duplicate(&seen, &h, None, T + DUPLICATE_WINDOW_MS + 1), "after the window it runs");
        // Another body with the same id.
        assert!(is_duplicate(&seen, "other", Some("a1"), T + 60_000));
        assert!(!is_duplicate(&seen, "other", Some("a2"), T + 60_000));
        assert!(!is_duplicate(&seen, "other", None, T + 60_000), "no id: only the body counts");
        // Old rows are forgotten as new ones arrive.
        remember(&mut seen, SeenAlert { hash: "x".into(), tag: None, at_ms: T + DUPLICATE_WINDOW_MS + 10 });
        assert_eq!(seen.len(), 1);
    }

    #[test]
    fn the_price_guard_refuses_a_move_past_its_percent() {
        assert_eq!(price_refusal(Some(100.0), 100.5, 0.5, true), Ok(()), "exactly 0.5%");
        assert_eq!(price_refusal(Some(100.0), 99.6, 0.5, true), Ok(()));
        assert_eq!(price_refusal(Some(100.0), 100.73, 0.5, true), Err("tvPriceMoved|0.73%".into()));
        assert_eq!(price_refusal(Some(100.0), 98.0, 0.5, false), Err("tvPriceMoved|2.00%".into()));
        assert_eq!(price_refusal(Some(100.0), 98.0, 2.5, true), Ok(()), "the user's own guard");
        assert_eq!(price_refusal(None, 98.0, 0.5, true), Err(PRICE_REQUIRED.into()));
        assert_eq!(price_refusal(None, 98.0, 0.5, false), Ok(()));
    }

    #[test]
    fn the_bots_own_limits_are_enforced_on_every_entry() {
        let (mut cfg, tv) = tv();
        let none = Usage::default();
        assert_eq!(limit_refusal(&cfg, &tv, "BTCUSDT", 60.0, 3, none), Ok(()));
        assert_eq!(limit_refusal(&cfg, &tv, "BTCUSDT", 61.0, 3, none), Err("tvSizeAboveMax|60.00".into()));
        let today = Usage { opens_today: 3, ..none };
        assert_eq!(limit_refusal(&cfg, &tv, "BTCUSDT", 10.0, 3, today), Err("tvDailyOpensFull|3".into()));
        let full = Usage { open: 2, margin_usdt: 20.0, opens_today: 0 };
        assert_eq!(limit_refusal(&cfg, &tv, "BTCUSDT", 10.0, 3, full), Err("tvPositionsFull|2".into()));
        // Budget 40: 30 used, a 60 notional at 3x needs 20 of margin.
        let used = Usage { open: 1, margin_usdt: 30.0, opens_today: 0 };
        assert_eq!(limit_refusal(&cfg, &tv, "BTCUSDT", 60.0, 3, used), Err("tvBudgetFull|10.00".into()));
        assert_eq!(limit_refusal(&cfg, &tv, "BTCUSDT", 30.0, 3, used), Ok(()));
        // The allow list.
        cfg.symbols = vec!["ETHUSDT".into()];
        assert_eq!(limit_refusal(&cfg, &tv, "BTCUSDT", 10.0, 3, none), Err(NOT_ALLOWED.into()));
        assert_eq!(limit_refusal(&cfg, &tv, "ethusdt", 10.0, 3, none), Ok(()));
    }

    #[test]
    fn renamed_multiplier_symbols_map_with_their_price_factor() {
        let futures: Vec<String> = ["BTCUSDT", "1000PEPEUSDT", "1000000MOGUSDT", "1MBABYDOGEUSDT"].map(String::from).to_vec();
        assert_eq!(map_symbol("BTCUSDT", &futures), Ok(("BTCUSDT".into(), 1.0)));
        assert_eq!(map_symbol("1000PEPEUSDT", &futures), Ok(("1000PEPEUSDT".into(), 1.0)), "the .P ticker itself");
        assert_eq!(map_symbol("PEPEUSDT", &futures), Ok(("1000PEPEUSDT".into(), 1000.0)), "a spot chart's ticker");
        assert_eq!(map_symbol("MOGUSDT", &futures), Ok(("1000000MOGUSDT".into(), 1e6)));
        assert_eq!(map_symbol("BABYDOGEUSDT", &futures), Ok(("1MBABYDOGEUSDT".into(), 1e6)));
        let spot: Vec<String> = ["PEPEUSDT", "BTCUSDT"].map(String::from).to_vec();
        assert_eq!(map_symbol("1000PEPEUSDT", &spot), Ok(("PEPEUSDT".into(), 0.001)));
        assert_eq!(map_symbol("DOGEUSDT", &futures), Err(NOT_LISTED.into()));
        assert_eq!(map_symbol("ANYUSDT", &[]), Ok(("ANYUSDT".into(), 1.0)), "no list: the exchange decides");
    }

    #[test]
    fn utc_day_starts_at_midnight() {
        assert_eq!(utc_day(86_400_000 * 3 + 5), 86_400_000 * 3);
    }
}
