//! The alert message a TradingView alert sends, and what the app does with
//! one. Pure: no network, no app state.
//!
//! Format (the alert's "Message" box, JSON). Manual mode:
//!
//! ```text
//! {"action":"buy|sell|close","symbol":"{{ticker}}",
//!  "tp":<price> | "tp_pct":<percent>, "sl":<price> | "sl_pct":<percent>,
//!  "bot":"futures|spot", "size_usdt":<optional>, "price":"{{close}}",
//!  "passphrase":"<optional>", "id":"<optional>"}
//! ```
//!
//! Strategy mode (a Pine strategy's alert): `market_position` present.
//!
//! ```text
//! {"action":"{{strategy.order.action}}",
//!  "market_position":"{{strategy.market_position}}",
//!  "symbol":"{{ticker}}", "price":"{{close}}", "tp_pct":3, "sl_pct":1.5,
//!  "alert_message":{{strategy.order.alert_message}}}
//! ```
//!
//! - Manual: `buy` opens a long, `sell` a short (futures only), `close`
//!   closes the TradingView bot's own position on that symbol.
//! - Strategy: the order action (buy / sell) and the position after it
//!   (long / short / flat) map to open, close or flip against what the bot
//!   holds (`plan`): buy+long opens a long or flips a short, sell+flat closes
//!   a long, and so on. `alert_message` (an object, or a string holding
//!   one) fills fields the top level does not set: a Pine script can put
//!   its own tp / sl there.
//! - Exactly one of `tp` / `tp_pct` and one of `sl` / `sl_pct` for an
//!   entry. A percent is a positive distance from the price on the side the
//!   trade needs (`resolve_levels`); on a Live bot it is measured from the
//!   real fill (`live::levels_after_fill`).
//! - `bot` defaults to futures (the TradingView bot). `symbol` may carry
//!   TradingView's perpetual suffix (`.P`, `PERP`): a perpetual on the spot
//!   bot is refused (`tvPerpOnSpot`).
//! - Numbers may be JSON numbers or numeric strings ("65000.5"): a
//!   TradingView placeholder inside quotes still works.
//! - `passphrase` and `id` are optional. The passphrase is compared and
//!   never stored; `id` names the alert for the duplicate guard.
//! - A test alert (the relay's `"test":true` on the event, from Settings'
//!   "Send test alert", or `"test":true` in the message itself) is only
//!   recorded, as `tvTestOnly`: never opened, closed or priced, paper or
//!   live, whatever the switches say.

use std::fmt;

use serde_json::{Map, Value};

use crate::bot::engine::user_trade::{normalize_symbol, BOT_KIND, SIZE_INVALID};
use crate::bot::model::BotKind;
use crate::signal::model::Direction;

pub use crate::bot::engine::user_trade::{resolve_levels, Level};

/// Plain text, bad JSON, no or unknown action, no symbol, a field of the
/// wrong type.
pub const BAD_MESSAGE: &str = "tvBadMessage";
/// Take-profit / stop missing, given twice (price and %), or not a positive
/// number (a % must be below 100 where it moves the price toward zero).
pub const BAD_LEVELS: &str = "tvBadLevels";
/// Older than the stale window on the server's clock.
pub const EXPIRED: &str = "tvExpired";
/// Received by the relay while the app was offline: shown, never run.
pub const EXPIRED_OFFLINE: &str = "tvExpiredOffline";
/// The events reply carried no server time: age cannot be measured.
pub const NO_SERVER_TIME: &str = "tvServerTimeMissing";
/// `close` with no position of the TradingView bot on the symbol.
pub const NOTHING_TO_CLOSE: &str = "tvNothingToClose";
/// Recorded only: "Execute alerts" is off.
pub const RECEIVED: &str = "received";
/// Recorded only: a test alert. Never executed.
pub const TEST_ONLY: &str = "tvTestOnly";
pub const OPENED: &str = "opened";
pub const CLOSED: &str = "closed";
/// A strategy flip: closed, then opened.
pub const FLIPPED: &str = "flipped";
/// The spot bot is long-only: the existing code of the entry pipeline.
pub const SPOT_LONG_ONLY: &str = "spotLongOnly";
/// A perpetual (`.P`) symbol sent to the spot bot.
pub const PERP_ON_SPOT: &str = "tvPerpOnSpot";
/// `market_position` not long / short / flat.
pub const BAD_POSITION: &str = "tvBadPosition";
/// An entry while the bot already holds the symbol in that direction.
pub const ALREADY_OPEN: &str = "tvAlreadyOpen";
/// A manual entry while the bot holds the opposite direction (send close
/// first, or use a strategy alert, which flips).
pub const OPPOSITE_OPEN: &str = "tvOppositeOpen";
/// A strategy order that only reduces a position (partial exit).
pub const PARTIAL_EXIT: &str = "tvPartialExit";
/// The strategy says flat on the other side from what the bot holds.
pub const STRATEGY_MISMATCH: &str = "tvStrategyMismatch";

/// The stale window before a TradingView bot exists (spot paper alerts).
pub const DEFAULT_STALE_MS: u64 = 120_000;

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

/// `{{strategy.market_position}}`: the position after the order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Long,
    Short,
    Flat,
}

/// Text that must never reach a log or a store (`Debug` hides it).
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(pub String);

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Alert {
    pub action: Action,
    /// Strategy mode: the position after the order.
    pub market_position: Option<Position>,
    /// Normalized: `BTCUSDT`.
    pub symbol: String,
    /// The symbol carried TradingView's perpetual suffix (`.P`, `PERP`).
    pub perp: bool,
    pub bot: BotKind,
    /// `None` when the message gave none (a close, a strategy exit).
    pub tp: Option<Level>,
    pub sl: Option<Level>,
    pub size_usdt: Option<f64>,
    /// `{{close}}` when the alert sent it: the price guard's reference.
    pub price: Option<f64>,
    pub passphrase: Option<Secret>,
    /// The message's own `id`, for the duplicate guard.
    pub tag: Option<String>,
}

/// What could be read of a message that was refused, for the results table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Partial {
    pub action: Option<Action>,
    pub symbol: Option<String>,
    pub bot: Option<BotKind>,
}

impl Partial {
    pub fn of(a: &Alert) -> Self {
        Self { action: Some(a.action), symbol: Some(a.symbol.clone()), bot: Some(a.bot) }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Refused {
    pub code: String,
    pub seen: Partial,
}

/// `(BTCUSDT, perp)` from TradingView's forms: `BINANCE:BTCUSDT.P`,
/// `BTCUSDT.P`, `BTCUSDTPERP`, ` btcusdt `. Only USDT pairs
/// (`tradeSymbolInvalid`).
pub fn read_tv_symbol(raw: &str) -> Result<(String, bool), String> {
    let mut s = raw.trim().to_ascii_uppercase();
    if let Some((_, tail)) = s.rsplit_once(':') {
        s = tail.to_string();
    }
    let mut perp = false;
    if let Some(head) = s.strip_suffix(".P") {
        (s, perp) = (head.to_string(), true);
    } else if let Some(head) = s.strip_suffix("PERP") {
        (s, perp) = (head.to_string(), true);
    }
    normalize_symbol(&s).map(|s| (s, perp))
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
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.trim().to_ascii_lowercase())),
        Some(_) => Err(()),
    }
}

fn present(obj: &Map<String, Value>, k: &str) -> bool {
    obj.get(k).is_some_and(|v| !v.is_null() && v.as_str().is_none_or(|s| !s.trim().is_empty()))
}

/// One level from its price key and its percent key: exactly one present,
/// or neither (`Ok(None)`).
fn level(obj: &Map<String, Value>, price_key: &str, pct_key: &str) -> Result<Option<Level>, String> {
    let positive = |k: &str| number(&obj[k]).filter(|v| v.is_finite() && *v > 0.0).ok_or_else(|| BAD_LEVELS.to_string());
    match (present(obj, price_key), present(obj, pct_key)) {
        (true, false) => Ok(Some(Level::Price(positive(price_key)?))),
        (false, true) => Ok(Some(Level::Pct(positive(pct_key)?))),
        (false, false) => Ok(None),
        (true, true) => Err(BAD_LEVELS.to_string()),
    }
}

/// `alert_message` fills what the top level leaves out (an object, or a
/// string holding one: `{{strategy.order.alert_message}}`).
fn merged(mut obj: Map<String, Value>) -> Map<String, Value> {
    let inner = match obj.remove("alert_message") {
        Some(Value::Object(o)) => Some(o),
        Some(Value::String(s)) => match serde_json::from_str::<Value>(s.trim()) {
            Ok(Value::Object(o)) => Some(o),
            _ => None,
        },
        _ => None,
    };
    if let Some(inner) = inner {
        for (k, v) in inner {
            if k != "alert_message" && !present(&obj, &k) {
                obj.insert(k, v);
            }
        }
    }
    obj
}

/// Reads one alert body. Refusals carry what was readable.
pub fn parse(body: &str) -> Result<Alert, Refused> {
    let mut seen = Partial::default();
    let refuse = |code: &str, seen: &Partial| Refused { code: code.to_string(), seen: seen.clone() };
    let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(body.trim()) else {
        return Err(refuse(BAD_MESSAGE, &seen));
    };
    let obj = merged(obj);
    let market_position = match word(&obj, "market_position") {
        Ok(None) => None,
        Ok(Some(p)) if p == "long" => Some(Position::Long),
        Ok(Some(p)) if p == "short" => Some(Position::Short),
        Ok(Some(p)) if p == "flat" => Some(Position::Flat),
        _ => return Err(refuse(BAD_POSITION, &seen)),
    };
    let action = match word(&obj, "action") {
        Ok(Some(a)) if a == "buy" => Action::Buy,
        Ok(Some(a)) if a == "sell" => Action::Sell,
        Ok(Some(a)) if a == "close" && market_position.is_none() => Action::Close,
        _ => return Err(refuse(BAD_MESSAGE, &seen)),
    };
    seen.action = Some(action);
    let raw_symbol = match obj.get("symbol") {
        Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
        _ => return Err(refuse(BAD_MESSAGE, &seen)),
    };
    let (symbol, perp) = read_tv_symbol(&raw_symbol).map_err(|c| {
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
    if perp && bot == BotKind::Spot {
        return Err(refuse(PERP_ON_SPOT, &seen));
    }
    let passphrase = match obj.get("passphrase") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(Secret(s.clone())),
        Some(_) => return Err(refuse(BAD_MESSAGE, &seen)),
    };
    let tag = match obj.get("id") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.trim().is_empty() => None,
        Some(Value::String(s)) => Some(s.trim().chars().take(64).collect()),
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(_) => return Err(refuse(BAD_MESSAGE, &seen)),
    };
    let price = match obj.get("price") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.trim().is_empty() => None,
        Some(v) => match number(v) {
            Some(p) if p.is_finite() && p > 0.0 => Some(p),
            _ => return Err(refuse(BAD_MESSAGE, &seen)),
        },
    };
    let base = Alert {
        action,
        market_position,
        symbol,
        perp,
        bot,
        tp: None,
        sl: None,
        size_usdt: None,
        price,
        passphrase,
        tag,
    };
    if action == Action::Close {
        return Ok(base);
    }
    if action == Action::Sell && bot == BotKind::Spot && market_position != Some(Position::Flat) {
        return Err(refuse(SPOT_LONG_ONLY, &seen));
    }
    let size_usdt = match obj.get("size_usdt") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.trim().is_empty() => None,
        Some(v) => match number(v) {
            Some(n) if n.is_finite() && n > 0.0 => Some(n),
            Some(_) => return Err(refuse(SIZE_INVALID, &seen)),
            None => return Err(refuse(BAD_MESSAGE, &seen)),
        },
    };
    let tp = level(&obj, "tp", "tp_pct").map_err(|c| refuse(&c, &seen))?;
    let sl = level(&obj, "sl", "sl_pct").map_err(|c| refuse(&c, &seen))?;
    // A manual entry needs both; a strategy alert only when it opens (`plan`).
    if market_position.is_none() && (tp.is_none() || sl.is_none()) {
        return Err(refuse(BAD_LEVELS, &seen));
    }
    // A percent that would put a level at or below zero, on either side.
    let too_far = |l: Option<Level>| matches!(l, Some(Level::Pct(p)) if p >= 100.0);
    let toward_zero = match (action, market_position) {
        (Action::Buy, _) => too_far(sl),
        (Action::Sell, None) | (Action::Sell, Some(Position::Short)) => too_far(tp),
        _ => false,
    };
    if toward_zero {
        return Err(refuse(BAD_LEVELS, &seen));
    }
    Ok(Alert { tp, sl, size_usdt, ..base })
}

/// Whether either level needs a reference price.
pub fn needs_price(a: &Alert) -> bool {
    matches!(a.tp, Some(Level::Pct(_))) || matches!(a.sl, Some(Level::Pct(_)))
}

/// What one alert does against what the TradingView bot holds on its
/// symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    Open(Direction),
    Close,
    /// Close the held position, then (only once it is gone) open this way.
    Flip(Direction),
}

/// Maps an alert to open, close or flip given the bot's own position on the
/// symbol (`held`, its direction). Refusals are codes.
pub fn plan(a: &Alert, held: Option<Direction>) -> Result<Plan, String> {
    use Direction::{Long, Short};
    let code = |c: &str| Err(c.to_string());
    let Some(pos) = a.market_position else {
        return match (a.action, held) {
            (Action::Close, Some(_)) => Ok(Plan::Close),
            (Action::Close, None) => code(NOTHING_TO_CLOSE),
            (Action::Buy, None) => Ok(Plan::Open(Long)),
            (Action::Sell, None) => Ok(Plan::Open(Short)),
            (Action::Buy, Some(Long)) | (Action::Sell, Some(Short)) => code(ALREADY_OPEN),
            (Action::Buy, Some(Short)) | (Action::Sell, Some(Long)) => code(OPPOSITE_OPEN),
        };
    };
    match (a.action, pos, held) {
        // An entry, or a reversal through zero.
        (Action::Buy, Position::Long, None) => Ok(Plan::Open(Long)),
        (Action::Sell, Position::Short, None) => Ok(Plan::Open(Short)),
        (Action::Buy, Position::Long, Some(Short)) => Ok(Plan::Flip(Long)),
        (Action::Sell, Position::Short, Some(Long)) => Ok(Plan::Flip(Short)),
        // Pyramiding: one position per symbol.
        (Action::Buy, Position::Long, Some(Long)) | (Action::Sell, Position::Short, Some(Short)) => code(ALREADY_OPEN),
        // An exit to flat.
        (Action::Sell, Position::Flat, Some(Long)) | (Action::Buy, Position::Flat, Some(Short)) => Ok(Plan::Close),
        (_, Position::Flat, None) => code(NOTHING_TO_CLOSE),
        (Action::Sell, Position::Flat, Some(Short)) | (Action::Buy, Position::Flat, Some(Long)) => code(STRATEGY_MISMATCH),
        // A partial exit: the strategy still holds the side it reduced.
        (Action::Sell, Position::Long, _) | (Action::Buy, Position::Short, _) => code(PARTIAL_EXIT),
        (Action::Close, _, _) => code(BAD_MESSAGE),
    }
}

/// What to do with one event before the bot is consulted.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// Store this outcome code; nothing runs.
    Record { code: String, seen: Partial },
    Execute(Alert),
}

/// The time facts of one event, all on the server's clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clock {
    /// When the relay received the alert.
    pub received_at_ms: Option<u64>,
    /// The server's `now` of the reply that carried it, advanced by the
    /// time since that reply arrived; None = the reply had none.
    pub server_now_ms: Option<u64>,
    /// The relay marked it as received while the app was offline.
    pub app_offline: bool,
}

/// The stale guard on the server's clock: older than `window_ms`, undated,
/// or no server time to measure against. A received time slightly ahead of
/// `now` is fresh. The PC clock is never used.
pub fn stale_refusal(clock: &Clock, window_ms: u64) -> Option<&'static str> {
    let Some(now) = clock.server_now_ms else {
        return Some(NO_SERVER_TIME);
    };
    match clock.received_at_ms {
        Some(at) if now.saturating_sub(at) <= window_ms => None,
        _ => Some(EXPIRED),
    }
}

/// `"test": true` in the message itself.
pub fn marks_test(body: &str) -> bool {
    matches!(serde_json::from_str::<Value>(body.trim()), Ok(Value::Object(o)) if o.get("test") == Some(&Value::Bool(true)))
}

/// The decision for one event. A test event (`test`, or `"test":true` in
/// the body) is checked first and only ever recorded as `tvTestOnly`.
/// Execute off records `received` (or why the message could not be read,
/// so a template can be fixed before execution is turned on); an alert the
/// relay received while the app was offline is `tvExpiredOffline`, an old
/// one `tvExpired`: never run.
pub fn decide(body: &str, test: bool, execute: bool, clock: &Clock, window_ms: u64) -> Decision {
    let parsed = parse(body);
    if test || marks_test(body) {
        let seen = match parsed {
            Ok(a) => Partial::of(&a),
            Err(r) => r.seen,
        };
        return Decision::Record { code: TEST_ONLY.to_string(), seen };
    }
    let alert = match parsed {
        Ok(a) => a,
        Err(r) => return Decision::Record { code: r.code, seen: r.seen },
    };
    let seen = Partial::of(&alert);
    if !execute {
        return Decision::Record { code: RECEIVED.to_string(), seen };
    }
    if clock.app_offline {
        return Decision::Record { code: EXPIRED_OFFLINE.to_string(), seen };
    }
    if let Some(code) = stale_refusal(clock, window_ms) {
        return Decision::Record { code: code.to_string(), seen };
    }
    Decision::Execute(alert)
}

/// The open user trade (`manual-` id) of `kind` on `symbol`, if any: a
/// spot `close` alert never touches a bot's own signal positions (Sentinel
/// ids) or DCA / Grid cycles (not in this book). Takes (id, kind, symbol).
pub fn close_target<'a>(rows: impl IntoIterator<Item = (&'a str, BotKind, &'a str)>, kind: BotKind, symbol: &str) -> Option<String> {
    rows.into_iter()
        .find(|(id, k, s)| crate::bot::engine::user_trade::is_user_trade_id(id) && *k == kind && s.eq_ignore_ascii_case(symbol))
        .map(|(id, _, _)| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_760_000_000_000;
    const WINDOW: u64 = 30_000;

    fn alert(body: &str) -> Alert {
        parse(body).unwrap_or_else(|r| panic!("{body}: {r:?}"))
    }

    fn code(body: &str) -> String {
        parse(body).unwrap_err().code
    }

    fn clock(at: Option<u64>) -> Clock {
        Clock { received_at_ms: at, server_now_ms: Some(NOW), app_offline: false }
    }

    fn normalize_tv_symbol(raw: &str) -> Result<String, String> {
        read_tv_symbol(raw).map(|(s, _)| s)
    }

    #[test]
    fn every_tradingview_symbol_form_becomes_the_pair() {
        for raw in ["BINANCE:BTCUSDT.P", "BTCUSDT.P", "BTCUSDTPERP", "btcusdt", " BTCUSDT ", "BYBIT:BTCUSDT", "binance:btcusdt.p"] {
            assert_eq!(normalize_tv_symbol(raw).as_deref(), Ok("BTCUSDT"), "{raw}");
        }
        assert_eq!(read_tv_symbol("BINANCE:1000PEPEUSDT.P"), Ok(("1000PEPEUSDT".to_string(), true)));
        assert_eq!(read_tv_symbol("BINANCE:PEPEUSDT"), Ok(("PEPEUSDT".to_string(), false)));
        for raw in ["BTCUSD", "BINANCE:ETHBTC", "BTCUSDC.P", "", "USDT", "BTC-USDT", "{{ticker}}"] {
            assert_eq!(normalize_tv_symbol(raw).unwrap_err(), "tradeSymbolInvalid", "{raw}");
        }
    }

    #[test]
    fn a_perpetual_on_the_spot_bot_is_refused() {
        assert_eq!(code(r#"{"action":"buy","symbol":"BINANCE:BTCUSDT.P","bot":"spot","tp":2,"sl":1}"#), PERP_ON_SPOT);
        assert_eq!(code(r#"{"action":"close","symbol":"BTCUSDTPERP","bot":"spot"}"#), PERP_ON_SPOT);
        let a = alert(r#"{"action":"buy","symbol":"BINANCE:BTCUSDT.P","tp":2,"sl":1}"#);
        assert!(a.perp && a.bot == BotKind::Futures);
    }

    #[test]
    fn a_buy_with_prices_reads_as_given() {
        let a = alert(r#"{"action":"buy","symbol":"BINANCE:SOLUSDT.P","tp":210,"sl":"190.5","price":"200.1","id":"x1","passphrase":"pw"}"#);
        assert_eq!(a.action, Action::Buy);
        assert_eq!(a.symbol, "SOLUSDT");
        assert_eq!(a.bot, BotKind::Futures, "futures is the default bot");
        assert_eq!((a.tp, a.sl), (Some(Level::Price(210.0)), Some(Level::Price(190.5))));
        assert_eq!((a.size_usdt, a.price), (None, Some(200.1)));
        assert_eq!(a.tag.as_deref(), Some("x1"));
        assert_eq!(a.passphrase, Some(Secret("pw".into())));
        assert!(!format!("{a:?}").contains("\"pw\""), "the passphrase never prints");
        assert_eq!(plan(&a, None), Ok(Plan::Open(Direction::Long)));
        assert!(!needs_price(&a) && a.market_position.is_none());
    }

    #[test]
    fn percents_resolve_on_the_side_each_trade_needs() {
        let long = alert(r#"{"action":"BUY","symbol":"BTCUSDT","tp_pct":3,"sl_pct":1.5,"bot":"spot","size_usdt":50}"#);
        assert_eq!(long.bot, BotKind::Spot);
        assert_eq!(long.size_usdt, Some(50.0));
        assert_eq!(long.action, Action::Buy);
        assert!(needs_price(&long));
        let (tp, sl) = resolve_levels(Direction::Long, 100.0, long.tp.unwrap(), long.sl.unwrap());
        assert!((tp - 103.0).abs() < 1e-9 && (sl - 98.5).abs() < 1e-9, "{tp} {sl}");
        let short = alert(r#"{"action":"sell","symbol":"BTCUSDT","tp_pct":"3","sl_pct":1.5}"#);
        assert_eq!(plan(&short, None), Ok(Plan::Open(Direction::Short)));
        let (tp, sl) = resolve_levels(Direction::Short, 100.0, short.tp.unwrap(), short.sl.unwrap());
        assert!((tp - 97.0).abs() < 1e-9 && (sl - 101.5).abs() < 1e-9, "{tp} {sl}");
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
            r#"{"action":"buy","symbol":"BTCUSDT","tp_pct":300,"sl_pct":100}"#,
            r#"{"action":"sell","symbol":"BTCUSDT","tp_pct":100,"sl_pct":5}"#,
            r#"{"action":"buy","symbol":"BTCUSDT","market_position":"long","tp_pct":3,"sl_pct":100}"#,
        ];
        for body in cases {
            assert_eq!(code(body), BAD_LEVELS, "{body}");
        }
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
            r#"{"action":"buy","symbol":"BTCUSDT","tp":2,"sl":1,"price":"{{close}}"}"#,
            r#"{"action":"buy","symbol":"BTCUSDT","tp":2,"sl":1,"passphrase":5}"#,
            r#"{"action":"close","symbol":"BTCUSDT","market_position":"flat"}"#,
        ] {
            assert_eq!(code(body), BAD_MESSAGE, "{body}");
        }
        assert_eq!(code(r#"{"action":"buy","symbol":"BTCUSDT","market_position":"sideways"}"#), BAD_POSITION);
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

    /// The builder's strategy template with TradingView's placeholders filled.
    fn strategy(action: &str, position: &str) -> Alert {
        alert(&format!(
            r#"{{"action":"{action}","market_position":"{position}","symbol":"BINANCE:BTCUSDT.P","price":"65000","tp_pct":3,"sl_pct":1.5,"size_usdt":"40"}}"#
        ))
    }

    #[test]
    fn strategy_placeholders_map_to_open_close_and_flip() {
        use Direction::{Long, Short};
        let cases = [
            ("buy", "long", None, Ok(Plan::Open(Long))),
            ("sell", "short", None, Ok(Plan::Open(Short))),
            ("buy", "long", Some(Short), Ok(Plan::Flip(Long))),
            ("sell", "short", Some(Long), Ok(Plan::Flip(Short))),
            ("sell", "flat", Some(Long), Ok(Plan::Close)),
            ("buy", "flat", Some(Short), Ok(Plan::Close)),
            ("buy", "long", Some(Long), Err(ALREADY_OPEN)),
            ("sell", "short", Some(Short), Err(ALREADY_OPEN)),
            ("sell", "flat", None, Err(NOTHING_TO_CLOSE)),
            ("buy", "flat", None, Err(NOTHING_TO_CLOSE)),
            ("sell", "flat", Some(Short), Err(STRATEGY_MISMATCH)),
            ("buy", "flat", Some(Long), Err(STRATEGY_MISMATCH)),
            ("sell", "long", Some(Long), Err(PARTIAL_EXIT)),
            ("buy", "short", Some(Short), Err(PARTIAL_EXIT)),
        ];
        for (action, position, held, want) in cases {
            let a = strategy(action, position);
            assert!(a.market_position.is_some());
            assert_eq!(plan(&a, held), want.map_err(str::to_string), "{action} {position} {held:?}");
        }
    }

    #[test]
    fn manual_actions_map_against_the_held_position() {
        use Direction::{Long, Short};
        let buy = alert(r#"{"action":"buy","symbol":"BTCUSDT","tp_pct":2,"sl_pct":1}"#);
        let sell = alert(r#"{"action":"sell","symbol":"BTCUSDT","tp_pct":2,"sl_pct":1}"#);
        let close = alert(r#"{"action":"close","symbol":"BTCUSDT"}"#);
        assert_eq!(plan(&buy, None), Ok(Plan::Open(Long)));
        assert_eq!(plan(&sell, None), Ok(Plan::Open(Short)));
        assert_eq!(plan(&buy, Some(Long)), Err(ALREADY_OPEN.into()));
        assert_eq!(plan(&buy, Some(Short)), Err(OPPOSITE_OPEN.into()), "manual mode never flips");
        assert_eq!(plan(&close, Some(Short)), Ok(Plan::Close));
        assert_eq!(plan(&close, None), Err(NOTHING_TO_CLOSE.into()));
    }

    #[test]
    fn a_strategy_exit_needs_no_levels_and_alert_message_fills_them() {
        let exit = alert(r#"{"action":"sell","market_position":"flat","symbol":"BTCUSDT"}"#);
        assert_eq!((exit.tp, exit.sl), (None, None));
        // The Pine script's alert_message, as an object and as a string.
        let obj = alert(r#"{"action":"buy","market_position":"long","symbol":"BTCUSDT","alert_message":{"tp_pct":4,"sl_pct":2,"tp":null}}"#);
        assert_eq!((obj.tp, obj.sl), (Some(Level::Pct(4.0)), Some(Level::Pct(2.0))));
        let text = alert(r#"{"action":"buy","market_position":"long","symbol":"BTCUSDT","sl_pct":1,"alert_message":"{\"tp\":70000,\"sl_pct\":9}"}"#);
        assert_eq!((text.tp, text.sl), (Some(Level::Price(70000.0)), Some(Level::Pct(1.0))), "the top level wins");
        // Unreadable alert_message text adds nothing.
        let plain = alert(r#"{"action":"buy","market_position":"long","symbol":"BTCUSDT","alert_message":"go long"}"#);
        assert_eq!(plain.tp, None);
        // A whole message box of only {{strategy.order.alert_message}}.
        let whole = alert(r#"{"action":"sell","market_position":"short","symbol":"ETHUSDT","tp_pct":2,"sl_pct":1}"#);
        assert_eq!(plan(&whole, None), Ok(Plan::Open(Direction::Short)));
    }

    /// The Settings builder's output (src/lib/tvMessage.test.ts) once
    /// TradingView has filled its placeholders.
    #[test]
    fn the_apps_builder_messages_parse_once_tradingview_fills_them() {
        let fill = |s: &str| {
            s.replace("{{ticker}}", "BINANCE:BTCUSDT.P")
                .replace("{{close}}", "65000.5")
                .replace("{{timenow}}", "2026-10-20T10:00:00Z")
                .replace("{{strategy.order.action}}", "sell")
                .replace("{{strategy.market_position}}", "short")
                .replace("{{strategy.order.alert_message}}", r#"{"size_usdt":30}"#)
                .replace("{{plot_1}}", "64000")
        };
        let buy = alert(&fill(r#"{"action":"buy","symbol":"{{ticker}}","tp_pct":3,"sl_pct":1.5,"price":"{{close}}","id":"{{timenow}}"}"#));
        assert_eq!((buy.symbol.as_str(), buy.perp, buy.price), ("BTCUSDT", true, Some(65000.5)));
        assert_eq!(buy.tag.as_deref(), Some("2026-10-20T10:00:00Z"));
        let sell = alert(&fill(r#"{"action":"sell","symbol":"{{ticker}}","tp_pct":3,"sl_pct":1.5,"size_usdt":40,"price":"{{close}}"}"#));
        assert_eq!((sell.size_usdt, plan(&sell, None)), (Some(40.0), Ok(Plan::Open(Direction::Short))));
        let close = alert(&fill(r#"{"action":"close","symbol":"{{ticker}}","price":"{{close}}","id":"{{timenow}}"}"#));
        assert_eq!(plan(&close, Some(Direction::Long)), Ok(Plan::Close));
        let strat = alert(&fill(
            r#"{"action":"{{strategy.order.action}}","market_position":"{{strategy.market_position}}","symbol":"{{ticker}}","tp_pct":3,"sl_pct":1.5,"alert_message":{{strategy.order.alert_message}}}"#,
        ));
        assert_eq!((strat.size_usdt, plan(&strat, Some(Direction::Long))), (Some(30.0), Ok(Plan::Flip(Direction::Short))));
        let prices = alert(&fill(r#"{"action":"buy","symbol":"{{ticker}}","tp":70000,"sl":"{{plot_1}}"}"#));
        assert_eq!((prices.tp, prices.sl), (Some(Level::Price(70000.0)), Some(Level::Price(64000.0))));
        let spot = alert(r#"{"action":"buy","symbol":"BINANCE:BTCUSDT","bot":"spot","tp_pct":3,"sl_pct":1.5,"price":"65000","passphrase":"pw1","id":"trend-a"}"#);
        assert_eq!((spot.bot, spot.passphrase, spot.tag.as_deref()), (BotKind::Spot, Some(Secret("pw1".into())), Some("trend-a")));
    }

    #[test]
    fn the_stale_guard_uses_the_servers_clock_only() {
        assert_eq!(stale_refusal(&clock(Some(NOW)), WINDOW), None);
        assert_eq!(stale_refusal(&clock(Some(NOW - WINDOW)), WINDOW), None, "exactly the window is on time");
        assert_eq!(stale_refusal(&clock(Some(NOW - WINDOW - 1)), WINDOW), Some(EXPIRED));
        assert_eq!(stale_refusal(&clock(Some(NOW + 5_000)), WINDOW), None, "received slightly ahead");
        assert_eq!(stale_refusal(&clock(None), WINDOW), Some(EXPIRED), "undated");
        // No server time in the reply: never measured on the PC clock.
        let no_now = Clock { received_at_ms: Some(NOW), server_now_ms: None, app_offline: false };
        assert_eq!(stale_refusal(&no_now, WINDOW), Some(NO_SERVER_TIME));

        let buy = r#"{"action":"buy","symbol":"BTCUSDT","tp_pct":2,"sl_pct":1}"#;
        match decide(buy, false, true, &clock(Some(NOW - 31_000)), WINDOW) {
            Decision::Record { code, seen } => {
                assert_eq!(code, EXPIRED);
                assert_eq!(seen.symbol.as_deref(), Some("BTCUSDT"));
            }
            d => panic!("{d:?}"),
        }
        assert!(matches!(decide(buy, false, true, &clock(Some(NOW - 1_000)), WINDOW), Decision::Execute(_)));
        // The same age under the paper default window (120 s) is on time.
        assert!(matches!(decide(buy, false, true, &clock(Some(NOW - 31_000)), DEFAULT_STALE_MS), Decision::Execute(_)));
    }

    #[test]
    fn an_alert_received_while_offline_is_shown_and_never_run() {
        let buy = r#"{"action":"buy","symbol":"BTCUSDT","tp_pct":2,"sl_pct":1}"#;
        let late = Clock { received_at_ms: Some(NOW - 1_000), server_now_ms: Some(NOW), app_offline: true };
        match decide(buy, false, true, &late, WINDOW) {
            Decision::Record { code, seen } => {
                assert_eq!(code, EXPIRED_OFFLINE);
                assert_eq!(seen.action, Some(Action::Buy));
            }
            d => panic!("{d:?}"),
        }
        // Execute off: still only received.
        assert!(matches!(decide(buy, false, false, &late, WINDOW), Decision::Record { ref code, .. } if code == RECEIVED));
    }

    #[test]
    fn execute_off_records_and_never_runs() {
        let buy = r#"{"action":"buy","symbol":"BTCUSDT","tp_pct":2,"sl_pct":1}"#;
        for at in [Some(NOW), Some(NOW - 600_000), None] {
            match decide(buy, false, false, &clock(at), WINDOW) {
                Decision::Record { code, seen } => {
                    assert_eq!(code, RECEIVED);
                    assert_eq!(seen.action, Some(Action::Buy));
                }
                d => panic!("{d:?}"),
            }
        }
        assert!(matches!(decide("hello", false, false, &clock(Some(NOW)), WINDOW), Decision::Record { ref code, .. } if code == BAD_MESSAGE));
        assert!(matches!(decide("hello", false, true, &clock(Some(NOW)), WINDOW), Decision::Record { ref code, .. } if code == BAD_MESSAGE));
    }

    /// The relay's test body (`TVWebhookTestBody` on the server).
    const SERVER_TEST_BODY: &str = r#"{"action":"buy","symbol":"BTCUSDT","tp_pct":3,"sl_pct":1.5,"bot":"futures","test":true}"#;

    #[test]
    fn a_test_alert_is_only_ever_recorded() {
        let bodies = [
            SERVER_TEST_BODY,
            r#"{"action":"buy","symbol":"BTCUSDT","tp_pct":2,"sl_pct":1}"#,
            r#"{"action":"sell","symbol":"ETHUSDT","tp":1,"sl":2,"bot":"futures"}"#,
            r#"{"action":"close","symbol":"BTCUSDT","bot":"spot"}"#,
            r#"{"action":"buy","market_position":"long","symbol":"BTCUSDT","tp_pct":2,"sl_pct":1}"#,
            "hello",
        ];
        for body in bodies {
            for execute in [true, false] {
                for offline in [true, false] {
                    for at in [Some(NOW), Some(NOW - 1_000), Some(NOW - 600_000), None] {
                        let c = Clock { received_at_ms: at, server_now_ms: Some(NOW), app_offline: offline };
                        match decide(body, true, execute, &c, WINDOW) {
                            Decision::Record { code, .. } => assert_eq!(code, TEST_ONLY, "{body}"),
                            d => panic!("test alert reached {d:?} ({body}, execute {execute})"),
                        }
                    }
                }
            }
        }
        match decide(SERVER_TEST_BODY, true, true, &clock(Some(NOW)), WINDOW) {
            Decision::Record { seen, .. } => {
                assert_eq!(seen, Partial { action: Some(Action::Buy), symbol: Some("BTCUSDT".into()), bot: Some(BotKind::Futures) });
            }
            d => panic!("{d:?}"),
        }
    }

    #[test]
    fn test_true_in_the_message_is_a_test_even_without_the_relay_flag() {
        assert!(matches!(decide(SERVER_TEST_BODY, false, true, &clock(Some(NOW)), WINDOW), Decision::Record { ref code, .. } if code == TEST_ONLY));
        let close = r#"{"action":"close","symbol":"BTCUSDT","test":true}"#;
        assert!(matches!(decide(close, false, true, &clock(Some(NOW)), WINDOW), Decision::Record { ref code, .. } if code == TEST_ONLY));
        assert!(marks_test(SERVER_TEST_BODY));
        for body in [r#"{"action":"buy","test":"true"}"#, r#"{"action":"buy","test":false}"#, r#"{"action":"buy"}"#, "test", "[true]"] {
            assert!(!marks_test(body), "{body}");
        }
        let fresh_buy = r#"{"action":"buy","symbol":"BTCUSDT","tp_pct":2,"sl_pct":1,"test":false}"#;
        assert!(matches!(decide(fresh_buy, false, true, &clock(Some(NOW)), WINDOW), Decision::Execute(_)));
    }

    #[test]
    fn a_spot_close_touches_only_the_users_own_trade_of_that_bot() {
        let rows = vec![
            ("sig-abc", BotKind::Futures, "BTCUSDT"),
            ("manual-1-0000000a", BotKind::Spot, "BTCUSDT"),
            ("manual-2-0000000b", BotKind::Futures, "ETHUSDT"),
            ("manual-3-0000000c", BotKind::Futures, "BTCUSDT"),
        ];
        assert_eq!(close_target(rows.clone(), BotKind::Futures, "BTCUSDT").as_deref(), Some("manual-3-0000000c"));
        assert_eq!(close_target(rows.clone(), BotKind::Spot, "btcusdt").as_deref(), Some("manual-1-0000000a"));
        let only_signal = vec![("sig-abc", BotKind::Futures, "BTCUSDT"), ("sb_dca_1", BotKind::Futures, "BTCUSDT")];
        assert_eq!(close_target(only_signal, BotKind::Futures, "BTCUSDT"), None);
        assert_eq!(close_target(rows, BotKind::Futures, "SOLUSDT"), None);
    }
}
