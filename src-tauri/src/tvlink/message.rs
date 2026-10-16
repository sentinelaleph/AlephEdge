//! The alert message a TradingView alert sends, and what the app does with
//! one. Pure: no network, no app state.
//!
//! Format (the alert's "Message" box, JSON):
//!
//! ```text
//! {"action":"buy|sell|close","symbol":"{{ticker}}",
//!  "tp":<price> | "tp_pct":<percent>, "sl":<price> | "sl_pct":<percent>,
//!  "bot":"futures|spot", "size_usdt":<optional>}
//! ```
//!
//! - `buy` opens a long, `sell` a short (futures only), `close` closes the
//!   app's own open user trade on that symbol and bot.
//! - Exactly one of `tp` / `tp_pct` and one of `sl` / `sl_pct` for buy and
//!   sell. A percent is a positive distance from the live price at execution,
//!   on the side the trade needs (see `resolve_levels`).
//! - `bot` defaults to futures; no `size_usdt` = the bot's own sizing.
//! - Numbers may be JSON numbers or numeric strings ("65000.5"): a
//!   TradingView placeholder inside quotes still works.

use serde_json::{Map, Value};

use crate::bot::engine::user_trade::{normalize_symbol, BOT_KIND, SIZE_INVALID};
use crate::bot::model::BotKind;
use crate::signal::model::Direction;

/// Plain text, bad JSON, no or unknown action, no symbol, a field of the
/// wrong type.
pub const BAD_MESSAGE: &str = "tvBadMessage";
/// Take-profit / stop missing, given twice (price and %), or not a positive
/// number (a % must be below 100 where it moves the price toward zero).
pub const BAD_LEVELS: &str = "tvBadLevels";
/// Received more than `MAX_AGE_MS` before the app read it.
pub const EXPIRED: &str = "tvExpired";
/// `close` with no user trade of that bot open on the symbol.
pub const NOTHING_TO_CLOSE: &str = "tvNothingToClose";
/// Recorded only: "Execute alerts" is off.
pub const RECEIVED: &str = "received";
pub const OPENED: &str = "opened";
pub const CLOSED: &str = "closed";
/// The spot bot is long-only: the existing code of the entry pipeline.
pub const SPOT_LONG_ONLY: &str = "spotLongOnly";

/// An alert received longer ago than this is never executed: the app may
/// have been closed for hours, and an old buy must not fill at today's price.
pub const MAX_AGE_MS: u64 = 120_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Buy,
    Sell,
    Close,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Buy => "buy",
            Action::Sell => "sell",
            Action::Close => "close",
        }
    }
}

/// A take-profit or stop as the alert gave it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Level {
    Price(f64),
    /// Positive distance from the live price, in percent.
    Pct(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Alert {
    pub action: Action,
    /// Normalized: `BTCUSDT`.
    pub symbol: String,
    pub bot: BotKind,
    /// `None` for `close`.
    pub tp: Option<Level>,
    pub sl: Option<Level>,
    pub size_usdt: Option<f64>,
}

impl Alert {
    pub fn direction(&self) -> Option<Direction> {
        match self.action {
            Action::Buy => Some(Direction::Long),
            Action::Sell => Some(Direction::Short),
            Action::Close => None,
        }
    }
}

/// What could be read of a message that was refused, for the results table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Partial {
    pub action: Option<Action>,
    pub symbol: Option<String>,
    pub bot: Option<BotKind>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Refused {
    pub code: String,
    pub seen: Partial,
}

/// `BTCUSDT` from TradingView's forms: `BINANCE:BTCUSDT.P`, `BTCUSDT.P`,
/// `BTCUSDTPERP`, ` btcusdt `. Only USDT pairs (`tradeSymbolInvalid`).
pub fn normalize_tv_symbol(raw: &str) -> Result<String, String> {
    let mut s = raw.trim().to_ascii_uppercase();
    if let Some((_, tail)) = s.rsplit_once(':') {
        s = tail.to_string();
    }
    if let Some(head) = s.strip_suffix(".P") {
        s = head.to_string();
    } else if let Some(head) = s.strip_suffix("PERP") {
        s = head.to_string();
    }
    normalize_symbol(&s)
}

/// A JSON number, or a string holding one.
fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// The text of a string field, trimmed and lower case.
fn word(obj: &Map<String, Value>, key: &str) -> Result<Option<String>, ()> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.trim().to_ascii_lowercase())),
        Some(_) => Err(()),
    }
}

/// One level from its price key and its percent key: exactly one present.
fn level(obj: &Map<String, Value>, price_key: &str, pct_key: &str) -> Result<Level, String> {
    let present = |k: &str| obj.get(k).is_some_and(|v| !v.is_null());
    match (present(price_key), present(pct_key)) {
        (true, false) => {
            let v = number(&obj[price_key]).ok_or(BAD_LEVELS)?;
            if v.is_finite() && v > 0.0 {
                Ok(Level::Price(v))
            } else {
                Err(BAD_LEVELS.to_string())
            }
        }
        (false, true) => {
            let v = number(&obj[pct_key]).ok_or(BAD_LEVELS)?;
            if v.is_finite() && v > 0.0 {
                Ok(Level::Pct(v))
            } else {
                Err(BAD_LEVELS.to_string())
            }
        }
        _ => Err(BAD_LEVELS.to_string()),
    }
}

/// Reads one alert body. Refusals carry what was readable.
pub fn parse(body: &str) -> Result<Alert, Refused> {
    let mut seen = Partial::default();
    let refuse = |code: &str, seen: &Partial| Refused { code: code.to_string(), seen: seen.clone() };
    let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(body.trim()) else {
        return Err(refuse(BAD_MESSAGE, &seen));
    };
    let action = match word(&obj, "action") {
        Ok(Some(a)) if a == "buy" => Action::Buy,
        Ok(Some(a)) if a == "sell" => Action::Sell,
        Ok(Some(a)) if a == "close" => Action::Close,
        _ => return Err(refuse(BAD_MESSAGE, &seen)),
    };
    seen.action = Some(action);
    let raw_symbol = match obj.get("symbol") {
        Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
        _ => return Err(refuse(BAD_MESSAGE, &seen)),
    };
    let symbol = normalize_tv_symbol(&raw_symbol).map_err(|c| {
        let mut s = seen.clone();
        s.symbol = Some(raw_symbol.trim().to_string());
        refuse(&c, &s)
    })?;
    seen.symbol = Some(symbol.clone());
    let bot = match word(&obj, "bot") {
        Ok(None) => BotKind::Futures,
        Ok(Some(b)) if b == "futures" => BotKind::Futures,
        Ok(Some(b)) if b == "spot" => BotKind::Spot,
        Ok(Some(_)) => return Err(refuse(BOT_KIND, &seen)),
        Err(()) => return Err(refuse(BAD_MESSAGE, &seen)),
    };
    seen.bot = Some(bot);
    if action == Action::Close {
        return Ok(Alert { action, symbol, bot, tp: None, sl: None, size_usdt: None });
    }
    if action == Action::Sell && bot == BotKind::Spot {
        return Err(refuse(SPOT_LONG_ONLY, &seen));
    }
    let size_usdt = match obj.get("size_usdt") {
        None | Some(Value::Null) => None,
        Some(v) => match number(v) {
            Some(n) if n.is_finite() && n > 0.0 => Some(n),
            Some(_) => return Err(refuse(SIZE_INVALID, &seen)),
            None => return Err(refuse(BAD_MESSAGE, &seen)),
        },
    };
    let tp = level(&obj, "tp", "tp_pct").map_err(|c| refuse(&c, &seen))?;
    let sl = level(&obj, "sl", "sl_pct").map_err(|c| refuse(&c, &seen))?;
    // A percent that would put a level at or below zero.
    let too_far = |l: Level| matches!(l, Level::Pct(p) if p >= 100.0);
    let toward_zero = match action {
        Action::Buy => sl,
        _ => tp,
    };
    if too_far(toward_zero) {
        return Err(refuse(BAD_LEVELS, &seen));
    }
    Ok(Alert { action, symbol, bot, tp: Some(tp), sl: Some(sl), size_usdt })
}

/// Take-profit and stop as prices, from the live `price` for percents.
///
/// Long (buy):   tp = price × (1 + tp_pct/100), sl = price × (1 − sl_pct/100).
/// Short (sell): tp = price × (1 − tp_pct/100), sl = price × (1 + sl_pct/100).
///
/// So `"tp_pct":3,"sl_pct":1.5` on a long at 100 is tp 103, sl 98.5; on a
/// short at 100 it is tp 97, sl 101.5. A price level is used as given; the
/// entry pipeline then checks its side against the live price.
pub fn resolve_levels(direction: Direction, price: f64, tp: Level, sl: Level) -> (f64, f64) {
    let up = |pct: f64| price * (1.0 + pct / 100.0);
    let down = |pct: f64| price * (1.0 - pct / 100.0);
    let tp = match (tp, direction) {
        (Level::Price(p), _) => p,
        (Level::Pct(p), Direction::Long) => up(p),
        (Level::Pct(p), Direction::Short) => down(p),
    };
    let sl = match (sl, direction) {
        (Level::Price(p), _) => p,
        (Level::Pct(p), Direction::Long) => down(p),
        (Level::Pct(p), Direction::Short) => up(p),
    };
    (tp, sl)
}

/// Whether either level needs the live price.
pub fn needs_price(a: &Alert) -> bool {
    matches!(a.tp, Some(Level::Pct(_))) || matches!(a.sl, Some(Level::Pct(_)))
}

/// What to do with one event.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// Store this outcome code; nothing runs.
    Record { code: String, seen: Partial },
    Open(Alert),
    Close(Alert),
}

/// The stale guard: `received_at_ms` (server time) more than `MAX_AGE_MS`
/// before `now_ms`, or unknown. A time slightly in the future (clocks
/// differ) is fresh.
pub fn is_stale(received_at_ms: Option<u64>, now_ms: u64) -> bool {
    match received_at_ms {
        Some(at) => now_ms.saturating_sub(at) > MAX_AGE_MS,
        None => true,
    }
}

/// The decision for one event body. Execute off records `received` (or why
/// the message could not be read, so a template can be fixed before
/// execution is turned on); an old event is `tvExpired`, never run.
pub fn decide(body: &str, received_at_ms: Option<u64>, execute: bool, now_ms: u64) -> Decision {
    let alert = match parse(body) {
        Ok(a) => a,
        Err(r) => return Decision::Record { code: r.code, seen: r.seen },
    };
    let seen = Partial { action: Some(alert.action), symbol: Some(alert.symbol.clone()), bot: Some(alert.bot) };
    if !execute {
        return Decision::Record { code: RECEIVED.to_string(), seen };
    }
    if is_stale(received_at_ms, now_ms) {
        return Decision::Record { code: EXPIRED.to_string(), seen };
    }
    match alert.action {
        Action::Close => Decision::Close(alert),
        _ => Decision::Open(alert),
    }
}

/// The open user trade (`manual-` id) of `kind` on `symbol`, if any: a
/// `close` alert never touches a bot's own signal positions (Sentinel ids)
/// or DCA / Grid cycles (not in this book). Takes (id, kind, symbol) rows.
pub fn close_target<'a>(rows: impl IntoIterator<Item = (&'a str, BotKind, &'a str)>, kind: BotKind, symbol: &str) -> Option<String> {
    rows.into_iter()
        .find(|(id, k, s)| crate::bot::engine::user_trade::is_user_trade_id(id) && *k == kind && s.eq_ignore_ascii_case(symbol))
        .map(|(id, _, _)| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_760_000_000_000;

    fn alert(body: &str) -> Alert {
        parse(body).unwrap_or_else(|r| panic!("{body}: {r:?}"))
    }

    fn code(body: &str) -> String {
        parse(body).unwrap_err().code
    }

    #[test]
    fn every_tradingview_symbol_form_becomes_the_pair() {
        for raw in ["BINANCE:BTCUSDT.P", "BTCUSDT.P", "BTCUSDTPERP", "btcusdt", " BTCUSDT ", "BYBIT:BTCUSDT", "binance:btcusdt.p"] {
            assert_eq!(normalize_tv_symbol(raw).as_deref(), Ok("BTCUSDT"), "{raw}");
        }
        for raw in ["BTCUSD", "BINANCE:ETHBTC", "BTCUSDC.P", "", "USDT", "BTC-USDT", "{{ticker}}"] {
            assert_eq!(normalize_tv_symbol(raw).unwrap_err(), "tradeSymbolInvalid", "{raw}");
        }
    }

    #[test]
    fn a_buy_with_prices_reads_as_given() {
        let a = alert(r#"{"action":"buy","symbol":"BINANCE:SOLUSDT.P","tp":210,"sl":"190.5"}"#);
        assert_eq!(a.action, Action::Buy);
        assert_eq!(a.symbol, "SOLUSDT");
        assert_eq!(a.bot, BotKind::Futures, "futures is the default bot");
        assert_eq!((a.tp, a.sl), (Some(Level::Price(210.0)), Some(Level::Price(190.5))));
        assert_eq!(a.size_usdt, None);
        assert_eq!(a.direction(), Some(Direction::Long));
        assert!(!needs_price(&a));
    }

    #[test]
    fn percents_resolve_on_the_side_each_trade_needs() {
        let long = alert(r#"{"action":"BUY","symbol":"BTCUSDT","tp_pct":3,"sl_pct":1.5,"bot":"spot","size_usdt":50}"#);
        assert_eq!(long.bot, BotKind::Spot);
        assert_eq!(long.size_usdt, Some(50.0));
        assert!(needs_price(&long));
        let (tp, sl) = resolve_levels(Direction::Long, 100.0, long.tp.unwrap(), long.sl.unwrap());
        assert!((tp - 103.0).abs() < 1e-9 && (sl - 98.5).abs() < 1e-9, "{tp} {sl}");

        let short = alert(r#"{"action":"sell","symbol":"BTCUSDT","tp_pct":"3","sl_pct":1.5}"#);
        assert_eq!(short.direction(), Some(Direction::Short));
        let (tp, sl) = resolve_levels(Direction::Short, 100.0, short.tp.unwrap(), short.sl.unwrap());
        assert!((tp - 97.0).abs() < 1e-9 && (sl - 101.5).abs() < 1e-9, "{tp} {sl}");

        // Mixed: a price take-profit and a percent stop.
        let (tp, sl) = resolve_levels(Direction::Short, 200.0, Level::Price(180.0), Level::Pct(2.0));
        assert_eq!((tp, sl), (180.0, 204.0));
    }

    #[test]
    fn levels_must_be_given_once_and_be_positive() {
        let cases = [
            r#"{"action":"buy","symbol":"BTCUSDT","sl":1}"#,
            r#"{"action":"buy","symbol":"BTCUSDT","tp":2}"#,
            r#"{"action":"buy","symbol":"BTCUSDT","tp":2,"tp_pct":3,"sl":1}"#,
            r#"{"action":"buy","symbol":"BTCUSDT","tp":2,"sl":1,"sl_pct":1}"#,
            r#"{"action":"buy","symbol":"BTCUSDT","tp":0,"sl":1}"#,
            r#"{"action":"buy","symbol":"BTCUSDT","tp":2,"sl":-1}"#,
            r#"{"action":"buy","symbol":"BTCUSDT","tp_pct":-3,"sl_pct":1}"#,
            r#"{"action":"buy","symbol":"BTCUSDT","tp":"abc","sl":1}"#,
            r#"{"action":"buy","symbol":"BTCUSDT","tp":2,"sl":{"x":1}}"#,
            // A long's stop or a short's take-profit at -100% is price zero.
            r#"{"action":"buy","symbol":"BTCUSDT","tp_pct":300,"sl_pct":100}"#,
            r#"{"action":"sell","symbol":"BTCUSDT","tp_pct":100,"sl_pct":5}"#,
        ];
        for body in cases {
            assert_eq!(code(body), BAD_LEVELS, "{body}");
        }
        // A short may set its stop 150% above; a long its target 300% above.
        assert!(parse(r#"{"action":"sell","symbol":"BTCUSDT","tp_pct":5,"sl_pct":150}"#).is_ok());
        assert!(parse(r#"{"action":"buy","symbol":"BTCUSDT","tp_pct":300,"sl_pct":5}"#).is_ok());
    }

    #[test]
    fn plain_text_and_bad_json_are_bad_messages() {
        for body in [
            "BTCUSDT crossed up",
            "",
            "{not json",
            "[1,2]",
            r#""buy""#,
            r#"{"symbol":"BTCUSDT","tp":2,"sl":1}"#,
            r#"{"action":"hold","symbol":"BTCUSDT","tp":2,"sl":1}"#,
            r#"{"action":7,"symbol":"BTCUSDT"}"#,
            r#"{"action":"buy","tp":2,"sl":1}"#,
            r#"{"action":"buy","symbol":42,"tp":2,"sl":1}"#,
            r#"{"action":"buy","symbol":"BTCUSDT","bot":1,"tp":2,"sl":1}"#,
            r#"{"action":"buy","symbol":"BTCUSDT","tp":2,"sl":1,"size_usdt":"lots"}"#,
        ] {
            assert_eq!(code(body), BAD_MESSAGE, "{body}");
        }
    }

    #[test]
    fn bot_side_and_size_refusals_use_the_existing_codes() {
        assert_eq!(code(r#"{"action":"sell","symbol":"BTCUSDT","bot":"spot","tp":1,"sl":2}"#), "spotLongOnly");
        assert_eq!(code(r#"{"action":"buy","symbol":"BTCUSDT","bot":"pump","tp":2,"sl":1}"#), "tradeBotKind");
        assert_eq!(code(r#"{"action":"buy","symbol":"BTCUSDT","tp":2,"sl":1,"size_usdt":0}"#), "tradeSizeInvalid");
        assert_eq!(code(r#"{"action":"buy","symbol":"ETHBTC","tp":2,"sl":1}"#), "tradeSymbolInvalid");
        let r = parse(r#"{"action":"buy","symbol":"ETHBTC","tp":2,"sl":1}"#).unwrap_err();
        assert_eq!(r.seen.action, Some(Action::Buy));
        assert_eq!(r.seen.symbol.as_deref(), Some("ETHBTC"), "the raw symbol is kept for the table");
    }

    #[test]
    fn close_needs_only_the_symbol_and_bot() {
        let a = alert(r#"{"action":"close","symbol":"BINANCE:ETHUSDT.P"}"#);
        assert_eq!((a.action, a.symbol.as_str(), a.bot, a.tp, a.sl), (Action::Close, "ETHUSDT", BotKind::Futures, None, None));
        let s = alert(r#"{"action":"close","symbol":"ETHUSDT","bot":"spot","tp":"ignored"}"#);
        assert_eq!(s.bot, BotKind::Spot);
    }

    #[test]
    fn the_stale_guard_refuses_old_and_undated_events() {
        assert!(!is_stale(Some(NOW), NOW));
        assert!(!is_stale(Some(NOW - MAX_AGE_MS), NOW), "exactly 120 s is still on time");
        assert!(is_stale(Some(NOW - MAX_AGE_MS - 1), NOW));
        assert!(is_stale(Some(NOW - 6 * 3_600_000), NOW), "an alert from hours ago");
        assert!(!is_stale(Some(NOW + 5_000), NOW), "a server clock slightly ahead");
        assert!(is_stale(None, NOW));

        let buy = r#"{"action":"buy","symbol":"BTCUSDT","tp_pct":2,"sl_pct":1}"#;
        match decide(buy, Some(NOW - 600_000), true, NOW) {
            Decision::Record { code, seen } => {
                assert_eq!(code, EXPIRED);
                assert_eq!(seen.symbol.as_deref(), Some("BTCUSDT"));
            }
            d => panic!("{d:?}"),
        }
        assert!(matches!(decide(buy, Some(NOW - 1_000), true, NOW), Decision::Open(_)));
        let close = r#"{"action":"close","symbol":"BTCUSDT"}"#;
        assert!(matches!(decide(close, Some(NOW - 1_000), true, NOW), Decision::Close(_)));
        assert!(matches!(decide(close, Some(NOW - 600_000), true, NOW), Decision::Record { ref code, .. } if code == EXPIRED));
    }

    #[test]
    fn execute_off_records_and_never_runs() {
        let buy = r#"{"action":"buy","symbol":"BTCUSDT","tp_pct":2,"sl_pct":1}"#;
        for at in [Some(NOW), Some(NOW - 600_000), None] {
            match decide(buy, at, false, NOW) {
                Decision::Record { code, seen } => {
                    assert_eq!(code, RECEIVED);
                    assert_eq!(seen.action, Some(Action::Buy));
                }
                d => panic!("{d:?}"),
            }
        }
        assert!(matches!(decide(r#"{"action":"close","symbol":"BTCUSDT"}"#, Some(NOW), false, NOW), Decision::Record { ref code, .. } if code == RECEIVED));
        // A message that cannot be read says so either way (never executed).
        assert!(matches!(decide("hello", Some(NOW), false, NOW), Decision::Record { ref code, .. } if code == BAD_MESSAGE));
        assert!(matches!(decide("hello", Some(NOW), true, NOW), Decision::Record { ref code, .. } if code == BAD_MESSAGE));
    }

    #[test]
    fn close_touches_only_the_users_own_trade_of_that_bot() {
        let rows = vec![
            ("sig-abc", BotKind::Futures, "BTCUSDT"),
            ("manual-1-0000000a", BotKind::Spot, "BTCUSDT"),
            ("manual-2-0000000b", BotKind::Futures, "ETHUSDT"),
            ("manual-3-0000000c", BotKind::Futures, "BTCUSDT"),
        ];
        assert_eq!(close_target(rows.clone(), BotKind::Futures, "BTCUSDT").as_deref(), Some("manual-3-0000000c"));
        assert_eq!(close_target(rows.clone(), BotKind::Spot, "btcusdt").as_deref(), Some("manual-1-0000000a"));
        // Only a Sentinel signal position on the symbol: nothing to close.
        let only_signal = vec![("sig-abc", BotKind::Futures, "BTCUSDT"), ("sb_dca_1", BotKind::Futures, "BTCUSDT")];
        assert_eq!(close_target(only_signal, BotKind::Futures, "BTCUSDT"), None);
        assert_eq!(close_target(rows, BotKind::Futures, "SOLUSDT"), None);
    }
}
