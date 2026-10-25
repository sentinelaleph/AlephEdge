//! The TradingView bot's entry (`BotKind::Tradingview`): one alert, one
//! position, through the New trade plan (`user_trade::plan_in`, the shared
//! entry pipeline in `EntryMode::Manual`) and, on a Live bot, the signal
//! bots' real entry (`live::open_live_with`: market order, exchange stop and
//! take-profit, the pilot cap).
//!
//! What a Live alert entry needs, all checked here at the open, every time:
//! the bot's stored mode Live AND its per-session arm (`cfg.live`, the typed
//! LIVE, never restored), the order path (`live_trading_allowed`: Binance
//! USDT-M futures, dry-run passed), the terms that arm confirmed still in
//! force (`live_terms::same_terms`), then `open_live_with`'s own checks (a
//! verified trade-only key in the unlocked vault, one-way mode, no exposure
//! on the symbol, the symbol's rules, the liquidation gate). The daily stop,
//! the risk level's leverage and position caps and the exchange minimum
//! come from the shared plan. "Stop all live" disarms (cfg.live = false).
//!
//! The synthetic signal id is derived from the relay event id
//! (`alert_signal_id`), and the entry's client id from it
//! (`entry_client_id`): an order whose answer was lost is never sent again
//! (the event is past the cursor) and is looked up by that client id
//! (`resolve_pending`, tvlink).

use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::exchange::providers::binance_parse::OrderFill;
use crate::exchange::providers::binance_requests::entry_client_id;
use crate::exchange::ExchangeManager;
use crate::risk::RiskManager;

use super::super::live_terms;
use super::super::model::{BotConfig, BotKind, OpenPosition, TakeProfitTarget, TradeMode};
use super::super::BotManager;
use super::live::{self, live_trading_allowed, Lookup};
use super::user_trade::{self, Level, PlanFor, TradeRequest};
use super::{book, precheck};

/// Prefix of an alert's synthetic id: a user trade (`manual-`), so the
/// precheck never asks Sentinel about it and History labels it the user's.
pub const ALERT_ID_PREFIX: &str = "manual-tv-";

/// `manual-tv-<24 hex of sha256(event id)>`: stable for one event, 34
/// characters, so the client id (`ae` + its first 34 letters and digits)
/// keeps the whole hash.
pub fn alert_signal_id(event_id: &str) -> String {
    let digest = Sha256::digest(event_id.as_bytes());
    let hex: String = digest.iter().take(12).map(|b| format!("{b:02x}")).collect();
    format!("{ALERT_ID_PREFIX}{hex}")
}

/// The entry order's client id for one relay event.
pub fn alert_client_id(event_id: &str) -> String {
    entry_client_id(&alert_signal_id(event_id))
}

/// A saved TradingView bot as the store keeps it (`hash_passphrase` is
/// `tv_passphrase::hash` in production): margin per position = budget /
/// positions, fixed sizing (an alert names its size), and the passphrase as
/// its Argon2id hash only: the form's text sets it, an empty text clears it,
/// none keeps the previous one (in whichever format it was saved). Any other
/// kind carries no TradingView settings.
pub fn normalize_config(
    prev: Option<&BotConfig>,
    mut next: BotConfig,
    hash_passphrase: &dyn Fn(&str) -> Result<String, String>,
) -> Result<BotConfig, String> {
    if next.kind != BotKind::Tradingview {
        next.tv = None;
        return Ok(next);
    }
    let mut tv = next.tv.take().ok_or_else(|| "tvSettingsMissing".to_string())?;
    match tv.passphrase.take() {
        Some(p) if p.trim().is_empty() => (tv.passphrase_hash, tv.passphrase_sha256, tv.passphrase_salt) = (None, None, None),
        Some(p) => {
            if p.chars().count() > 64 {
                return Err("tvPassphraseTooLong|64".to_string());
            }
            tv.passphrase_hash = Some(hash_passphrase(p.trim())?);
            (tv.passphrase_sha256, tv.passphrase_salt) = (None, None);
        }
        None => {
            let old = prev.and_then(|p| p.tv.as_ref());
            tv.passphrase_hash = old.and_then(|o| o.passphrase_hash.clone());
            tv.passphrase_sha256 = old.and_then(|o| o.passphrase_sha256.clone());
            tv.passphrase_salt = old.and_then(|o| o.passphrase_salt.clone());
        }
    }
    tv.name = tv.name.trim().to_string();
    tv.validate()?;
    if next.max_positions == 0 {
        return Err("botMaxPositionsMin".to_string());
    }
    next.capital = ((tv.budget_usdt / f64::from(next.max_positions)) * 100.0).floor() / 100.0;
    next.sizing = super::super::model::SizingMode::Fixed;
    next.symbols = next.symbols.iter().map(|s| s.trim().to_ascii_uppercase()).filter(|s| !s.is_empty()).collect();
    next.symbols.dedup();
    next.tv = Some(tv);
    Ok(next)
}

/// An alert whose size is below the smallest order the symbol takes:
/// `tvBelowSymbolMinimum|BTCUSDT|83.00|50.00|50.00` (symbol, the minimum, the
/// size asked, the bot's max size), so the refusal names the number.
pub const BELOW_SYMBOL_MIN: &str = "tvBelowSymbolMinimum";

/// The smallest order (USDT notional) a Binance futures symbol takes at
/// `price` (pure): the smallest quantity on the lot step that is at least
/// `min_qty` and whose notional reaches MIN_NOTIONAL, at that price, rounded
/// up to the cent. None without a usable price or step.
pub fn symbol_min_usdt(min_qty: f64, step: f64, min_notional: f64, price: f64) -> Option<f64> {
    if !(price.is_finite() && price > 0.0 && step.is_finite() && step > 0.0) {
        return None;
    }
    let by_notional = if min_notional > 0.0 { min_notional / price } else { 0.0 };
    let need = min_qty.max(by_notional).max(step);
    // Up to the step (an epsilon keeps an exact multiple where it is).
    let qty = ((need / step) - 1e-9).ceil() * step;
    Some(((qty * price) * 100.0 - 1e-6).ceil() / 100.0)
}

/// Refuses an alert of `size` (USDT) below the symbol's smallest order,
/// naming both (pure). `max_size` is the bot's own cap, shown so the user
/// sees which setting to raise.
pub fn symbol_min_refusal(symbol: &str, size: f64, max_size: f64, min_usdt: Option<f64>) -> Result<(), String> {
    match min_usdt {
        Some(min) if size + 1e-9 < min => Err(format!("{BELOW_SYMBOL_MIN}|{symbol}|{min:.2}|{size:.2}|{max_size:.2}")),
        _ => Ok(()),
    }
}

/// The open position of the TradingView bot on `symbol`, if any.
pub fn own_position<'a>(book: &'a [OpenPosition], symbol: &str) -> Option<&'a OpenPosition> {
    book.iter().find(|p| p.bot_kind == BotKind::Tradingview && p.symbol.eq_ignore_ascii_case(symbol))
}

/// The bot (other than the TradingView bot) holding `symbol`, if any.
pub fn other_holder(book: &[OpenPosition], symbol: &str) -> Option<BotKind> {
    book.iter()
        .find(|p| p.bot_kind != BotKind::Tradingview && p.symbol.eq_ignore_ascii_case(symbol))
        .map(|p| p.bot_kind)
}

/// Why a Live TradingView bot may not take a real entry now (pure): stored
/// mode Live, armed this session, an order venue, the armed terms.
pub fn live_entry_refusal(cfg: &BotConfig, order_path: bool, armed: Option<&live_terms::LiveTerms>, now: &live_terms::LiveTerms) -> Result<(), String> {
    if cfg.mode != TradeMode::Live {
        return Err(user_trade::BOT_IS_PAPER.to_string());
    }
    if !cfg.live {
        return Err("liveArmRequired".to_string());
    }
    if !order_path {
        return Err(user_trade::LIVE_FUTURES_ONLY.to_string());
    }
    live_terms::same_terms(armed, now)
}

/// What an alert entry did.
#[derive(Debug, Clone)]
pub struct AlertOpen {
    pub pos: OpenPosition,
    /// Quantity the plan sized at the live price (paper fill model).
    pub requested_qty: f64,
}

/// Why an alert entry opened nothing, and whether a real order's outcome
/// stayed unknown (then `planned` is what was sent, for the later lookup).
#[derive(Debug, Clone)]
pub struct AlertRefusal {
    pub code: String,
    pub outcome_unknown: bool,
    pub planned: Option<OpenPosition>,
}

impl From<String> for AlertRefusal {
    fn from(code: String) -> Self {
        Self { code, outcome_unknown: false, planned: None }
    }
}

/// Opens one alert's position on the TradingView bot: Paper on a Paper bot,
/// a real order on a Live, armed one. `levels` are the alert's own (a
/// percent is put at the real fill and re-checked there).
pub async fn open_alert(app: &AppHandle, req: &TradeRequest, event_id: &str, levels: (Level, Level)) -> Result<AlertOpen, AlertRefusal> {
    let bots = app.state::<BotManager>();
    let cfg = bots.config_for(BotKind::Tradingview).ok_or_else(|| "tvBotRequired".to_string())?;
    let id = alert_signal_id(event_id);
    let live = cfg.mode == TradeMode::Live;
    let limits = app.state::<RiskManager>().limits();
    if live {
        // Checked before any network call, and again by open_live_with.
        live_entry_refusal(
            &cfg,
            live_trading_allowed(&cfg, BotKind::Tradingview),
            bots.armed_terms(BotKind::Tradingview).as_ref(),
            &live_terms::signal_terms(&cfg, &limits),
        )?;
    } else if cfg.live {
        return Err(super::manual::PAPER_ONLY.to_string().into());
    }
    let req = TradeRequest { mode: cfg.mode, ..req.clone() };
    let (mut pos, _) = user_trade::plan_in(app, &req, PlanFor::Alert { id: &id }).await?;
    let requested_qty = pos.qty;
    if !live {
        if !book::open_position_if_free(app, pos.clone()) {
            return Err("tvAlreadyOpen".to_string().into());
        }
        bots.push_skip(&pos.symbol, "tvAlertOpened", None);
        bots.ensure_loop(app.clone());
        return Ok(AlertOpen { pos, requested_qty });
    }
    let leverage = precheck::effective_leverage(&cfg, &limits);
    let tcfg = user_trade::trade_config(&cfg, &req, leverage);
    let sig = user_trade::synthetic_signal(&req, &pos.symbol, pos.entry, super::now_ms(), pos.signal_id.clone());
    let opened = live::open_live_with(app, &tcfg, &sig, pos.entry, pos.notional_usdt, leverage, TakeProfitTarget::Tp1, Some(levels)).await;
    let open = match opened {
        Ok(o) => o,
        Err(r) => {
            return Err(AlertRefusal { code: r.key.to_string(), outcome_unknown: r.outcome_unknown, planned: Some(pos) });
        }
    };
    bots.pilot_used_saved(app, BotKind::Tradingview);
    apply_fill(&mut pos, open.entry, open.qty, Some(open.entry_order_id), open.stop_algo_id, open.tp_algo_id, open.tp.tp, open.sl);
    book::open_position(app, pos.clone());
    bots.push_skip(&pos.symbol, "tvAlertOpened", Some("live".to_string()));
    bots.ensure_loop(app.clone());
    // The daily stop or "Stop all live" can land while the order was on the
    // wire; their close-all ran on a book without this position.
    if bots.kill_switch_tripped() || !bots.config_for(BotKind::Tradingview).is_some_and(|c| c.live) {
        book::close_now(app, &pos.signal_id, BotKind::Tradingview, "stopped").await;
    }
    Ok(AlertOpen { pos, requested_qty })
}

/// The planned position as the exchange filled and protected it.
#[allow(clippy::too_many_arguments)]
pub fn apply_fill(
    pos: &mut OpenPosition,
    entry: f64,
    qty: f64,
    order_id: Option<i64>,
    stop_algo_id: Option<i64>,
    tp_algo_id: Option<i64>,
    tp: f64,
    sl: f64,
) {
    pos.live = true;
    pos.entry = entry;
    pos.qty = qty;
    pos.notional_usdt = qty * entry;
    pos.entry_order_id = order_id;
    pos.unprotected = stop_algo_id.is_none();
    pos.stop_algo_id = stop_algo_id;
    pos.tp_algo_id = tp_algo_id;
    pos.tp = tp;
    pos.sl = sl;
}

/// One lookup's verdict on an alert entry whose outcome stayed unknown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pending {
    /// It filled: protect it and book it.
    Filled(OrderFill),
    /// Binance says it did not and will not execute.
    Failed,
    /// No verdict yet (or the order is not found yet): look again later.
    Wait,
}

/// Reads one `order_by_client_id` answer for a pending alert entry: the
/// entry path's own rule (`live::unknown_entry_lookup`), and an order still
/// not found `NOT_FOUND_FINAL_MS` after it was sent is final: it never
/// reached the exchange.
pub fn pending_verdict(lookup: Lookup, not_found: bool, age_ms: u64) -> Pending {
    match lookup {
        Lookup::Filled(f) => Pending::Filled(f),
        Lookup::Failed => Pending::Failed,
        Lookup::LookAgain if not_found && age_ms >= NOT_FOUND_FINAL_MS => Pending::Failed,
        Lookup::LookAgain => Pending::Wait,
    }
}

/// An order Binance still does not know this long after it was sent was
/// never accepted.
pub const NOT_FOUND_FINAL_MS: u64 = 120_000;

/// Looks up one pending alert entry (`planned`, sent with
/// `alert_client_id(event_id)`). A late fill is protected at the alert's
/// levels from its fill and booked on the TradingView bot. Returns the
/// verdict; `Filled` means the position is now in the book (or the stop
/// could not be placed and it was flattened: then `Failed`).
pub async fn resolve_pending(app: &AppHandle, event_id: &str, planned: &OpenPosition, levels: (Level, Level), age_ms: u64) -> Pending {
    let bots = app.state::<BotManager>();
    if bots.has_position(&planned.signal_id, BotKind::Tradingview) {
        return Pending::Filled(OrderFill { order_id: planned.entry_order_id.unwrap_or(0), avg_price: planned.entry, executed_qty: planned.qty });
    }
    let Some(cred) = app.state::<crate::vault::VaultManager>().credential(&planned.exchange_id) else {
        return Pending::Wait;
    };
    let exchange = app.state::<ExchangeManager>();
    let client_id = alert_client_id(event_id);
    let res = exchange.order_by_client_id(&cred, &planned.symbol, &client_id).await;
    let not_found = matches!(&res, Err(crate::exchange::model::BinanceKeyCheckError::Rejected { code: -2013, .. }));
    let verdict = pending_verdict(live::unknown_entry_lookup(res), not_found, age_ms);
    let Pending::Filled(fill) = verdict else {
        return verdict;
    };
    let Ok(rules) = exchange.symbol_rules(&cred, &planned.symbol).await else {
        return Pending::Wait;
    };
    let cfg = bots.config_for(BotKind::Tradingview);
    let leverage = planned.leverage.max(1);
    let fee = user_trade::round_trip_fee(BotKind::Tradingview);
    let mut sig = user_trade::synthetic_signal(
        &TradeRequest {
            kind: BotKind::Tradingview,
            symbol: planned.symbol.clone(),
            direction: planned.direction,
            notional_usdt: Some(planned.notional_usdt),
            take_profit: planned.tp,
            stop_loss: planned.sl,
            mode: TradeMode::Live,
            confirmation: None,
            confirmed_margin_usdt: None,
        },
        &planned.symbol,
        fill.avg_price,
        super::now_ms(),
        planned.signal_id.clone(),
    );
    let long = planned.direction == crate::signal::model::Direction::Long;
    match live::levels_after_fill(planned.direction, fill.avg_price, levels, leverage, fee) {
        Ok((tp, sl)) => (sig.tp, sig.sl) = (vec![tp], sl),
        Err(_) => {
            return if live::flatten_confirmed(&exchange, &cred, &planned.symbol, long, fill.executed_qty).await {
                Pending::Failed
            } else {
                bots.push_skip(&planned.symbol, live::NOTE_UNPROTECTED, None);
                Pending::Wait
            };
        }
    }
    match live::protect_fill(&exchange, &bots, &cred, &sig, TakeProfitTarget::Tp1, &fill, fill.avg_price, rules.tick).await {
        Ok(p) => {
            let mut pos = planned.clone();
            apply_fill(&mut pos, fill.avg_price, fill.executed_qty, Some(fill.order_id), p.stop_algo_id, p.tp_algo_id, p.tp.tp, sig.sl);
            book::open_position(app, pos);
            bots.pilot_used_saved(app, BotKind::Tradingview);
            bots.push_skip(&planned.symbol, "tvLateFillAdopted", None);
            bots.ensure_loop(app.clone());
            // A bot stopped meanwhile does not keep it.
            if bots.kill_switch_tripped() || !cfg.is_some_and(|c| c.live) {
                book::close_now(app, &planned.signal_id, BotKind::Tradingview, "stopped").await;
            }
            Pending::Filled(fill)
        }
        // The stop failed and the emergency flatten confirmed flat.
        Err(_) => Pending::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::live_terms::signal_terms;
    use crate::risk::model::RiskLevel;

    fn fixture(symbol: &str) -> OpenPosition {
        use crate::bot::engine::test_fixtures::{position, signal};
        let mut p = position(&signal("long", 100.0, 110.0, 95.0), None);
        p.symbol = symbol.to_string();
        p
    }

    fn cfg(mode: TradeMode, live: bool) -> BotConfig {
        let mut c = BotConfig::new_default(BotKind::Tradingview, "binance");
        (c.mode, c.live, c.capital, c.max_positions) = (mode, live, 20.0, 5);
        c
    }

    /// A random test passphrase, made at run time (no literal secrets).
    fn random_text() -> String {
        format!("{:032x}", rand::random::<u128>())
    }

    #[test]
    fn a_saved_tradingview_bot_keeps_only_a_hash_of_its_passphrase() {
        use super::super::tv_passphrase::{check, hash, Verdict};
        let pass = random_text();
        let mut form = BotConfig::new_default(BotKind::Tradingview, "binance");
        form.max_positions = 4;
        form.symbols = vec![" btcusdt ".into(), "".into()];
        {
            let tv = form.tv.as_mut().unwrap();
            tv.budget_usdt = 100.0;
            tv.passphrase = Some(format!("  {pass} "));
        }
        let saved = normalize_config(None, form.clone(), &hash).unwrap();
        let tv = saved.tv.as_ref().unwrap();
        assert_eq!(saved.capital, 25.0, "margin per position = budget / positions");
        assert_eq!(saved.sizing, crate::bot::model::SizingMode::Fixed);
        assert_eq!(saved.symbols, vec!["BTCUSDT".to_string()]);
        assert_eq!(tv.passphrase, None, "the text itself is never kept");
        assert!(tv.passphrase_hash.as_deref().is_some_and(|h| h.starts_with("argon2id$")));
        assert!(tv.passphrase_sha256.is_none() && tv.passphrase_salt.is_none());
        let json = serde_json::to_string(&saved).unwrap();
        assert!(!json.contains(&pass) && !json.contains("\"passphrase\""), "{json}");
        // The alert side.
        assert_eq!(check(tv, Some(&pass)), Verdict::Match);
        assert_eq!(check(tv, Some(&format!(" {pass}"))), Verdict::Match);
        assert_eq!(check(tv, Some(&format!("{pass}!"))), Verdict::Refused);
        assert_eq!(check(tv, None), Verdict::Refused, "a set passphrase must be sent");
        // A save without a new passphrase keeps it; an empty one clears it.
        let mut again = saved.clone();
        again.tv.as_mut().unwrap().passphrase = None;
        let kept = normalize_config(Some(&saved), again.clone(), &hash).unwrap();
        assert_eq!(kept.tv.as_ref().unwrap().passphrase_hash, tv.passphrase_hash);
        again.tv.as_mut().unwrap().passphrase = Some(String::new());
        let cleared = normalize_config(Some(&saved), again, &hash).unwrap();
        assert!(cleared.tv.as_ref().unwrap().passphrase_hash.is_none());
        assert_eq!(check(cleared.tv.as_ref().unwrap(), None), Verdict::NoPassphrase, "no passphrase: any alert");
        // A 0.3.19 bot: a save without a new passphrase keeps the old fields
        // (still refused, the user is told to re-enter); a new passphrase
        // replaces them with Argon2id.
        let mut old = saved.clone();
        {
            let salt = random_text();
            let o = old.tv.as_mut().unwrap();
            o.passphrase_hash = None;
            o.passphrase_sha256 = Some(format!("{salt}{salt}"));
            o.passphrase_salt = Some(salt);
        }
        let mut form2 = old.clone();
        form2.tv.as_mut().unwrap().passphrase = None;
        let kept = normalize_config(Some(&old), form2.clone(), &hash).unwrap();
        assert_eq!(check(kept.tv.as_ref().unwrap(), Some(&pass)), Verdict::ResetNeeded);
        let other = random_text();
        form2.tv.as_mut().unwrap().passphrase = Some(other.clone());
        let renewed = normalize_config(Some(&old), form2, &hash).unwrap();
        let r = renewed.tv.as_ref().unwrap();
        assert!(r.passphrase_sha256.is_none() && r.passphrase_salt.is_none());
        assert_eq!(check(r, Some(&other)), Verdict::Match);
        assert_eq!(check(r, Some(&pass)), Verdict::Refused);
        // A hash that fails refuses the save.
        let mut form3 = form.clone();
        form3.tv.as_mut().unwrap().passphrase = Some(random_text());
        let failing = |_: &str| -> Result<String, String> { Err("tvPassphraseHashFailed".into()) };
        assert_eq!(normalize_config(None, form3, &failing).unwrap_err(), "tvPassphraseHashFailed");
        // Out-of-range limits are refused, not clamped.
        let mut bad = form.clone();
        bad.tv.as_mut().unwrap().stale_s = 4;
        assert_eq!(normalize_config(None, bad, &hash).unwrap_err(), "tvStaleRange|5–120");
        let mut bad = form.clone();
        bad.tv.as_mut().unwrap().price_guard_pct = 0.0;
        assert!(normalize_config(None, bad, &hash).unwrap_err().starts_with("tvPriceGuardRange|"));
        let mut bad = form;
        bad.tv.as_mut().unwrap().max_size_usdt = 4.0;
        assert!(normalize_config(None, bad, &hash).unwrap_err().starts_with("tvMaxSizeInvalid|"));
        // Any other bot carries no TradingView settings.
        let mut fut = BotConfig::new_default(BotKind::Futures, "binance");
        fut.tv = Some(Default::default());
        assert!(normalize_config(None, fut, &hash).unwrap().tv.is_none());
    }

    #[test]
    fn real_money_can_be_switched_on_for_the_tradingview_bot_on_binance_futures_only() {
        use crate::bot::commands::live_enable_refusal;
        assert_eq!(live_enable_refusal(true, BotKind::Tradingview, "binance", "LIVE", false), None);
        for venue in ["bybit", "okx"] {
            assert!(live_enable_refusal(true, BotKind::Tradingview, venue, "LIVE", false).is_some(), "{venue}: no dry run yet");
        }
        assert_eq!(live_enable_refusal(true, BotKind::Tradingview, "mexc", "LIVE", false), Some("liveFuturesBinanceOnly".into()));
        assert_eq!(live_enable_refusal(true, BotKind::Spot, "binance", "LIVE", false), Some("liveFuturesBinanceOnly".into()));
        assert_eq!(live_enable_refusal(true, BotKind::Tradingview, "binance", "live", false), Some("liveConfirmRequired|LIVE".into()));
    }

    #[test]
    fn the_client_id_is_derived_from_the_event_and_fits_binance() {
        let a = alert_client_id("41");
        assert_eq!(a, alert_client_id("41"), "stable: a lost order is looked up by it");
        assert_ne!(a, alert_client_id("42"));
        assert!(a.len() <= 36 && a.chars().all(|c| c.is_ascii_alphanumeric()), "{a}");
        assert!(a.starts_with("aemanualtv"));
        let id = alert_signal_id("41");
        assert!(crate::bot::engine::user_trade::is_user_trade_id(&id), "a user trade: never looked up at Sentinel");
        assert_eq!(id.len(), ALERT_ID_PREFIX.len() + 24);
        // The whole hash survives the client id's 34-character cut.
        assert_eq!(&a[2..], id.replace('-', "").as_str());
        // A long or odd event id still makes a valid one.
        let long = alert_client_id(&"x-".repeat(200));
        assert!(long.len() <= 36 && long.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn a_live_alert_entry_needs_mode_arm_venue_and_the_armed_terms() {
        let limits = RiskLevel::Balanced.limits();
        let armed = cfg(TradeMode::Live, true);
        let terms = signal_terms(&armed, &limits);
        assert_eq!(live_entry_refusal(&armed, true, Some(&terms), &terms), Ok(()));
        // Paper bot, or Live but not armed this session (a restart).
        assert_eq!(live_entry_refusal(&cfg(TradeMode::Paper, false), true, Some(&terms), &terms), Err("tradeBotIsPaper".into()));
        assert_eq!(live_entry_refusal(&cfg(TradeMode::Live, false), true, Some(&terms), &terms), Err("liveArmRequired".into()));
        // Not an order venue (Bybit / OKX before their run, spot).
        assert_eq!(live_entry_refusal(&armed, false, Some(&terms), &terms), Err("liveFuturesBinanceOnly".into()));
        // Armed, but the terms in force moved since.
        let mut now = terms.clone();
        now.leverage += 1;
        assert!(live_entry_refusal(&armed, true, Some(&terms), &now).unwrap_err().starts_with("liveTermsChanged|"));
        assert_eq!(live_entry_refusal(&armed, true, None, &terms), Err("liveArmRequired".into()));
    }

    #[test]
    fn a_close_finds_only_the_tradingview_bots_own_position() {
        let mut fut = fixture("BTCUSDT");
        fut.bot_kind = BotKind::Futures;
        let mut tv = fixture("ETHUSDT");
        tv.bot_kind = BotKind::Tradingview;
        let book = vec![fut, tv];
        assert!(own_position(&book, "BTCUSDT").is_none(), "the Futures bot's position is not the alert bot's");
        assert_eq!(other_holder(&book, "btcusdt"), Some(BotKind::Futures));
        assert_eq!(own_position(&book, "ethusdt").map(|p| p.symbol.as_str()), Some("ETHUSDT"));
        assert_eq!(other_holder(&book, "ETHUSDT"), None);
    }

    // Testnet run, 21 Oct: a BTC alert at max size 50 USDT was refused by the
    // 0.001 BTC minimum (~83 USDT) with no number in the refusal.
    #[test]
    fn an_alert_below_the_symbols_minimum_names_the_minimum() {
        // BTCUSDT: min qty 0.001, step 0.001, MIN_NOTIONAL 50, at 83,000.
        let btc = symbol_min_usdt(0.001, 0.001, 50.0, 83_000.0);
        assert_eq!(btc, Some(83.0));
        assert_eq!(
            symbol_min_refusal("BTCUSDT", 50.0, 50.0, btc),
            Err("tvBelowSymbolMinimum|BTCUSDT|83.00|50.00|50.00".to_string())
        );
        assert_eq!(symbol_min_refusal("BTCUSDT", 83.0, 100.0, btc), Ok(()), "exactly the minimum");
        // MIN_NOTIONAL binds: 0.1 step at 30 = 3 USDT, the filter says 5.
        assert_eq!(symbol_min_usdt(0.1, 0.1, 5.0, 30.0), Some(6.0), "two steps reach 5 USDT");
        assert_eq!(symbol_min_usdt(1.0, 1.0, 5.0, 2.5), Some(5.0));
        // Unknown price or rules: no figure, nothing refused here (the order path checks again).
        assert_eq!(symbol_min_usdt(0.001, 0.001, 50.0, 0.0), None);
        assert_eq!(symbol_min_refusal("BTCUSDT", 50.0, 50.0, None), Ok(()));
    }

    #[test]
    fn an_unknown_entry_is_looked_up_never_resent() {
        let fill = OrderFill { order_id: 9, avg_price: 2.0, executed_qty: 3.0 };
        assert_eq!(pending_verdict(Lookup::Filled(fill), false, 0), Pending::Filled(fill));
        assert_eq!(pending_verdict(Lookup::Failed, false, 0), Pending::Failed);
        // Not found yet: may still be in Binance's pipeline.
        assert_eq!(pending_verdict(Lookup::LookAgain, true, 5_000), Pending::Wait);
        assert_eq!(pending_verdict(Lookup::LookAgain, true, NOT_FOUND_FINAL_MS), Pending::Failed);
        // A lookup that failed (network) is never a verdict, however old.
        assert_eq!(pending_verdict(Lookup::LookAgain, false, 10 * NOT_FOUND_FINAL_MS), Pending::Wait);
    }

    #[test]
    fn a_late_fill_takes_the_real_fill_and_its_protection() {
        let mut pos = fixture("SOLUSDT");
        apply_fill(&mut pos, 101.0, 0.5, Some(7), Some(11), None, 104.0, 99.0);
        assert!(pos.live && !pos.unprotected);
        assert_eq!((pos.entry, pos.qty, pos.notional_usdt), (101.0, 0.5, 50.5));
        assert_eq!((pos.entry_order_id, pos.stop_algo_id, pos.tp_algo_id), (Some(7), Some(11), None));
        assert_eq!((pos.tp, pos.sl), (104.0, 99.0));
        apply_fill(&mut pos, 101.0, 0.5, Some(7), None, None, 104.0, 99.0);
        assert!(pos.unprotected, "no exchange stop: the book flattens it");
    }
}
