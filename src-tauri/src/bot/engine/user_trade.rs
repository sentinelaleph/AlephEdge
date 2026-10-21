//! "New trade": the user opens ONE paper position on their own terms
//! (symbol, side, size, take-profit, stop-loss), entry at the live price now.
//!
//! No new pipeline: the request becomes a synthetic `Signal` (entry = live
//! price, the user's take-profit as its single target, the user's stop as its
//! stop) and goes through the same `entry::prepare_entry` in
//! `EntryMode::Manual` that "Take this signal now" uses (manual.rs). The
//! fill, sizing, liquidation gate, exchange minimum, membership, funding,
//! depth and risk-budget checks, management, P&L, history and alerts are
//! therefore the existing code. The position is flagged `manual`, so its
//! trade lands in `manual_trades` and History labels it.
//!
//! Kept apart from Sentinel's signals: the synthetic signal is never put in
//! the signal buffer (`SignalManager`), so the signal list, the signal stats,
//! the bots' automatic loop, the phone link and Telegram never see it as a
//! published signal; its id carries `ID_PREFIX`, which the precheck reads to
//! skip the "has Sentinel resolved it" lookup (it was never published) and
//! the UI reads to label the trade as the user's own.
//!
//! Paper or Live, chosen on the form (`TradeRequest::mode`, Paper by
//! default) and matched to the bot: a Paper trade opens on a Paper bot's
//! book only, a Live one on the Live Futures bot armed this session only,
//! after the typed LIVE with the money at risk shown. A Live trade goes
//! through the signal bots' own real entry (`live::open_live`: market order,
//! exchange-side stop and take-profit, the pilot cap), so no new order code
//! exists here. The open re-plans and refuses (`liveTermsChanged`) when the
//! margin is no longer the one the confirmation showed. TradingView alerts
//! plan through the same function (`plan_in`, `PlanFor::Alert`) on the
//! TradingView bot (`tv_trade`), with an id derived from the relay event.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::exchange::ExchangeManager;
use crate::risk::RiskManager;
use crate::signal::model::{Direction, Signal};

use super::super::model::{
    BotConfig, BotKind, OpenPosition, SizingMode, TakeProfitTarget, TradeMode, EXCHANGE_MIN_ORDER_USDT, FUTURES_FEE_RATE,
    SPOT_FEE_RATE,
};
use super::super::BotManager;
use super::entry::{self, EntryMode, Prepared};
use super::manual::{outcome_usdt, reward_risk, skip_code, PAPER_ONLY};
use super::{book, entry_rules, live_price, now_ms, precheck, sizing};

/// Every synthetic signal id starts with this; Sentinel's ids never do.
pub use super::entry_rules::USER_TRADE_ID_PREFIX as ID_PREFIX;
/// Feed note on a new trade.
pub const FEED_KEY: &str = "userTradeOpened";
/// The synthetic signal's `mode`.
pub const MODE: &str = "manual";
/// How long the synthetic signal counts as valid for the entry gates. The
/// open position has no time limit (`horizon_ms` = 0): it exits at the
/// user's take-profit or stop (or by hand, or the bot's loss cap).
const ENTRY_VALID_MS: u64 = 60 * 60 * 1000;

// Refusal codes (besides the skip keys the shared pipeline returns).
/// The Pump bot (or any bot other than Futures / Spot).
pub const BOT_KIND: &str = "tradeBotKind";
/// Not a USDT pair in plain letters and digits.
pub const SYMBOL_INVALID: &str = "tradeSymbolInvalid";
/// Not on the exchange's tradable list for the bot's market.
pub const SYMBOL_NOT_LISTED: &str = "tradeSymbolNotListed";
/// Take-profit or stop missing, zero, negative or not a number.
pub const LEVELS_INVALID: &str = "tradeLevelsInvalid";
/// Size not a positive number.
pub const SIZE_INVALID: &str = "tradeSizeInvalid";
/// Take-profit not beyond the live price in the trade's direction.
pub const TARGET_WRONG_SIDE: &str = "tradeTargetWrongSide";
/// Stop not on the losing side of the live price.
pub const STOP_WRONG_SIDE: &str = "tradeStopWrongSide";
/// Stop closer than both fees: the stop would lose about the fees alone.
pub const STOP_INSIDE_FEES: &str = "tradeStopInsideFees";
/// Take-profit closer than both fees: reaching it books a loss.
pub const TARGET_INSIDE_FEES: &str = "tradeTargetInsideFees";
/// A Live trade asked of a Paper bot.
pub const BOT_IS_PAPER: &str = "tradeBotIsPaper";
/// Real money only on the Futures bot (the order path is futures only).
pub const LIVE_FUTURES_ONLY: &str = "liveFuturesBinanceOnly";

/// Whether `id` names a trade the user set up by hand, not a Sentinel
/// signal (defined with the pure entry rules, which the precheck reads).
pub use super::entry_rules::is_user_trade_id;

/// What the "New trade" form sends.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TradeRequest {
    pub kind: BotKind,
    pub symbol: String,
    pub direction: Direction,
    /// USDT notional; None = the bot's own sizing (fixed or risk).
    #[serde(default)]
    pub notional_usdt: Option<f64>,
    pub take_profit: f64,
    pub stop_loss: f64,
    /// Paper unless the user chose Live on the form. Must match the bot.
    #[serde(default)]
    pub mode: TradeMode,
    /// The typed word, for a Live trade.
    #[serde(default)]
    pub confirmation: Option<String>,
    /// The margin the Live confirmation showed (the preview's `capital`).
    /// The open re-plans and refuses (`liveTermsChanged`) when the money
    /// at risk is no longer this.
    #[serde(default)]
    pub confirmed_margin_usdt: Option<f64>,
}

/// A take-profit or stop as an alert gave it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum Level {
    Price(f64),
    /// Positive distance from the reference price, in percent.
    Pct(f64),
}

/// Take-profit and stop as prices from `price` for percents.
///
/// Long:  tp = price × (1 + tp_pct/100), sl = price × (1 − sl_pct/100).
/// Short: tp = price × (1 − tp_pct/100), sl = price × (1 + sl_pct/100).
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

/// What opening the trade now would do, from the paper fill model.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TradePreview {
    pub bot_kind: BotKind,
    pub symbol: String,
    pub direction: Direction,
    /// Paper fill: the live price.
    pub entry: f64,
    pub take_profit: f64,
    pub stop_loss: f64,
    pub qty: f64,
    pub notional_usdt: f64,
    /// The position's margin (fixed: notional / leverage; risk: the bot's).
    pub capital: f64,
    /// The bot's configured capital, the base of `risk_pct_of_capital`.
    pub bot_capital: f64,
    pub sizing_mode: SizingMode,
    /// notional / margin.
    pub effective_leverage: f64,
    /// The isolated leverage the exchange would run (futures; 1 for spot).
    pub leverage: u8,
    pub risk_capped: bool,
    /// NET loss at the stop, fees included; negative.
    pub loss_at_stop_usdt: f64,
    /// −loss as % of the bot's capital.
    pub risk_pct_of_capital: f64,
    /// NET gain at the take-profit, fees included.
    pub gain_at_target_usdt: f64,
    /// |take-profit − fill| / |fill − stop|.
    pub reward_risk: f64,
    /// Isolated liquidation price (futures above 1x long; None for spot).
    pub liquidation_price: Option<f64>,
    /// Both taker fees on the notional.
    pub fees_usdt: f64,
}

/// `BTCUSDT` from " btcusdt ": upper case, a USDT pair of letters and digits.
pub fn normalize_symbol(raw: &str) -> Result<String, String> {
    let s = raw.trim().to_ascii_uppercase();
    let ok = s.len() > 4 && s.ends_with("USDT") && s.chars().all(|c| c.is_ascii_alphanumeric());
    if ok {
        Ok(s)
    } else {
        Err(SYMBOL_INVALID.to_string())
    }
}

/// Why the trade's mode does not fit the bot (pure). Paper: a Paper bot,
/// never a Live one (armed or not). Live: the Futures bot, Live and armed
/// this session, with the typed word.
pub fn mode_refusal(cfg: &BotConfig, req: &TradeRequest) -> Result<(), String> {
    match req.mode {
        TradeMode::Paper if cfg.live || cfg.mode == TradeMode::Live => Err(PAPER_ONLY.to_string()),
        TradeMode::Paper => Ok(()),
        TradeMode::Live => {
            if req.kind != BotKind::Futures {
                return Err(LIVE_FUTURES_ONLY.to_string());
            }
            if cfg.mode != TradeMode::Live {
                return Err(BOT_IS_PAPER.to_string());
            }
            if !cfg.live {
                return Err("liveArmRequired".to_string());
            }
            if req.confirmation.as_deref().map(str::trim) != Some(crate::bot::LIVE_CONFIRMATION) {
                return Err(format!("liveConfirmRequired|{}", crate::bot::LIVE_CONFIRMATION));
            }
            Ok(())
        }
    }
}

/// Checks that need no network, in order: a saved config, the mode, the
/// daily stop, a valid config, a Futures or Spot bot, spot long only, the
/// symbol, the levels, the size, and no position of this bot on the symbol.
/// Returns the normalized symbol.
pub fn refusal(cfg: Option<&BotConfig>, tripped: bool, req: &TradeRequest, held: bool) -> Result<String, String> {
    let Some(cfg) = cfg else {
        return Err("botNotConfigured".to_string());
    };
    mode_refusal(cfg, req)?;
    if tripped {
        return Err("botDailyLossTripped".to_string());
    }
    cfg.validate()?;
    if !matches!(req.kind, BotKind::Futures | BotKind::Spot) {
        return Err(BOT_KIND.to_string());
    }
    if req.kind == BotKind::Spot && req.direction == Direction::Short {
        return Err("spotLongOnly".to_string());
    }
    let symbol = normalize_symbol(&req.symbol)?;
    let level_ok = |v: f64| v.is_finite() && v > 0.0;
    if !(level_ok(req.take_profit) && level_ok(req.stop_loss)) {
        return Err(LEVELS_INVALID.to_string());
    }
    if let Some(n) = req.notional_usdt {
        if !n.is_finite() || n <= 0.0 {
            return Err(SIZE_INVALID.to_string());
        }
        if n < EXCHANGE_MIN_ORDER_USDT {
            return Err(format!("belowExchangeMinimum|{n:.2}"));
        }
    }
    if held {
        return Err("marketHeld".to_string());
    }
    Ok(symbol)
}

/// Both taker fees as a fraction of notional on `kind`'s market.
pub fn round_trip_fee(kind: BotKind) -> f64 {
    2.0 * if kind.uses_futures_market() { FUTURES_FEE_RATE } else { SPOT_FEE_RATE }
}

/// The levels against the live `price`: the take-profit beyond it and the
/// stop behind it in the trade's direction, each farther away than both fees
/// (`fee_frac`, a fraction of notional). Details are the distance needed.
pub fn level_refusal(direction: Direction, price: f64, take_profit: f64, stop: f64, fee_frac: f64) -> Result<(), String> {
    let (tp_ok, sl_ok) = match direction {
        Direction::Long => (take_profit > price, stop < price),
        Direction::Short => (take_profit < price, stop > price),
    };
    if !tp_ok {
        return Err(TARGET_WRONG_SIDE.to_string());
    }
    if !sl_ok {
        return Err(STOP_WRONG_SIDE.to_string());
    }
    let need = format!("{:.2}%", fee_frac * 100.0);
    if (price - stop).abs() / price <= fee_frac {
        return Err(format!("{STOP_INSIDE_FEES}|{need}"));
    }
    if (take_profit - price).abs() / price <= fee_frac {
        return Err(format!("{TARGET_INSIDE_FEES}|{need}"));
    }
    Ok(())
}

/// The bot's config as this one trade uses it: the take-profit is the
/// user's own level (TP1 of the synthetic signal, no custom % or per-symbol
/// override moves it), and a set USDT size is fixed sizing at the bot's
/// risk-clamped leverage (`leverage_cap`), margin = notional / leverage.
pub fn trade_config(cfg: &BotConfig, req: &TradeRequest, leverage_cap: u8) -> BotConfig {
    let mut c = cfg.clone();
    c.take_profit = TakeProfitTarget::Tp1;
    c.take_profit_overrides.clear();
    if let Some(n) = req.notional_usdt {
        c.sizing = SizingMode::Fixed;
        c.capital = n / f64::from(leverage_cap.max(1));
    }
    c
}

/// A unique synthetic id: `manual-<ms>-<8 hex>`; `seq` tells apart ids made
/// in the same millisecond (a random number in production).
pub fn new_id(now_ms: u64, seq: u32) -> String {
    format!("{ID_PREFIX}{now_ms}-{seq:08x}")
}

/// UTC RFC3339 with millis ("2026-10-16T09:30:00.123Z") of UNIX `ms`.
pub fn rfc3339_utc(ms: u64) -> String {
    let secs = ms / 1000;
    let days = (secs / 86_400) as i64;
    let sod = secs % 86_400;
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        sod / 3600,
        (sod / 60) % 60,
        sod % 60,
        ms % 1000
    )
}

/// The synthetic signal for `req` at the live `price`: entry = price, the
/// user's take-profit as the single target, the user's stop. No timeframe,
/// no confluence, no management plan (no breakeven or partial: the user's
/// two levels are the whole plan).
pub fn synthetic_signal(req: &TradeRequest, symbol: &str, price: f64, now_ms: u64, id: String) -> Signal {
    Signal {
        id,
        symbol: symbol.to_string(),
        timeframe: None,
        market_type: Some(if req.kind.uses_futures_market() { "futures" } else { "spot" }.to_string()),
        direction: req.direction,
        mode: MODE.to_string(),
        entry: price,
        tp: vec![req.take_profit],
        sl: req.stop_loss,
        confidence: 0.0,
        regime: None,
        confluence: Vec::new(),
        combo: None,
        rr: reward_risk(price, req.take_profit, req.stop_loss),
        investment_score: None,
        regenerated: false,
        regenerated_at: None,
        management_plan: None,
        instrumentation: None,
        expires_at: rfc3339_utc(now_ms + ENTRY_VALID_MS),
        created_at: rfc3339_utc(now_ms),
    }
}

/// The open position as the book keeps it: manual, and no time limit.
pub fn mark_user_position(pos: &mut OpenPosition) {
    pos.manual = true;
    pos.horizon_ms = 0;
}

/// The preview of `pos` (`bot_capital`: the bot's own configured capital).
pub fn preview_of(pos: &OpenPosition, bot_capital: f64) -> TradePreview {
    let (loss, gain, fees) = outcome_usdt(pos);
    let futures = pos.bot_kind.uses_futures_market();
    TradePreview {
        bot_kind: pos.bot_kind,
        symbol: pos.symbol.clone(),
        direction: pos.direction,
        entry: pos.entry,
        take_profit: pos.tp,
        stop_loss: pos.sl,
        qty: pos.qty,
        notional_usdt: pos.notional_usdt,
        capital: pos.capital,
        bot_capital,
        sizing_mode: pos.sizing_mode,
        effective_leverage: sizing::leverage_of(pos),
        leverage: if futures { sizing::isolated_leverage(pos) } else { 1 },
        risk_capped: pos.risk_capped,
        loss_at_stop_usdt: loss,
        risk_pct_of_capital: if bot_capital > 0.0 { -loss / bot_capital * 100.0 } else { 0.0 },
        gain_at_target_usdt: gain,
        reward_risk: reward_risk(pos.entry, pos.tp, pos.sl),
        liquidation_price: sizing::liquidation_price(pos),
        fees_usdt: fees,
    }
}

/// Who asks for the trade: the New trade form, or a TradingView alert on
/// the TradingView bot (its synthetic id is derived from the relay event,
/// so a lost order can be looked up by its client id).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlanFor<'a> {
    Form,
    Alert { id: &'a str },
}

/// The pre-network checks of an alert entry on the TradingView bot (pure):
/// a saved config, the daily stop, a valid config, the TradingView bot,
/// the symbol, the levels, the size, and no position of this bot on it.
pub fn alert_refusal(cfg: Option<&BotConfig>, tripped: bool, req: &TradeRequest, held: bool) -> Result<String, String> {
    let Some(cfg) = cfg else {
        return Err("tvBotRequired".to_string());
    };
    if tripped {
        return Err("botDailyLossTripped".to_string());
    }
    cfg.validate()?;
    if req.kind != BotKind::Tradingview || cfg.kind != BotKind::Tradingview {
        return Err(BOT_KIND.to_string());
    }
    let symbol = normalize_symbol(&req.symbol)?;
    let level_ok = |v: f64| v.is_finite() && v > 0.0;
    if !(level_ok(req.take_profit) && level_ok(req.stop_loss)) {
        return Err(LEVELS_INVALID.to_string());
    }
    match req.notional_usdt {
        Some(n) if n.is_finite() && n >= EXCHANGE_MIN_ORDER_USDT => {}
        Some(n) if n.is_finite() && n > 0.0 => return Err(format!("belowExchangeMinimum|{n:.2}")),
        _ => return Err(SIZE_INVALID.to_string()),
    }
    if held {
        return Err("tvAlreadyOpen".to_string());
    }
    Ok(symbol)
}

/// The paper position the request would open now (and the bot's capital),
/// or the refusal code (`key` or `key|detail`).
async fn plan(app: &AppHandle, req: &TradeRequest) -> Result<(OpenPosition, f64), String> {
    plan_in(app, req, PlanFor::Form).await
}

pub(crate) async fn plan_in(app: &AppHandle, req: &TradeRequest, by: PlanFor<'_>) -> Result<(OpenPosition, f64), String> {
    let bots = app.state::<BotManager>();
    let cfg = bots.config_for(req.kind);
    let held = bots.holds_symbol(req.kind, req.symbol.trim());
    let symbol = match by {
        PlanFor::Form => refusal(cfg.as_ref(), bots.kill_switch_tripped(), req, held)?,
        PlanFor::Alert { .. } => alert_refusal(cfg.as_ref(), bots.kill_switch_tripped(), req, held)?,
    };
    let Some(cfg) = cfg else {
        return Err("botNotConfigured".to_string());
    };
    let exchange = app.state::<ExchangeManager>();
    // The list the strategy form's symbol picker reads; empty when the
    // exchange has none to give (then the live quote decides).
    let listed = exchange.symbols(&cfg.exchange_id, req.kind.uses_futures_market()).await;
    if !listed.is_empty() && !listed.iter().any(|s| s.eq_ignore_ascii_case(&symbol)) {
        return Err(SYMBOL_NOT_LISTED.to_string());
    }
    let price = live_price(&exchange, &cfg.exchange_id, &symbol, req.kind)
        .await
        .filter(|p| p.is_finite() && *p > 0.0)
        .ok_or_else(|| "priceUnavailable".to_string())?;
    level_refusal(req.direction, price, req.take_profit, req.stop_loss, round_trip_fee(req.kind))?;
    let leverage = precheck::effective_leverage(&cfg, &app.state::<RiskManager>().limits());
    let tcfg = trade_config(&cfg, req, leverage);
    let now = now_ms();
    let id = match by {
        PlanFor::Form => new_id(now, rand::random()),
        PlanFor::Alert { id } => id.to_string(),
    };
    let sig = synthetic_signal(req, &symbol, price, now, id);
    let Prepared { plan, ctx } = entry::prepare_entry(app, &tcfg, &sig, bots.btc_regime(), EntryMode::Manual, held, now)
        .await
        .map_err(|s| skip_code(&s))?
        // A manual entry never waits (`entry_rules::plan_entry_in`).
        .ok_or_else(|| "priceUnavailable".to_string())?;
    let mut pos = entry_rules::finish_position(&tcfg, &sig, plan.price, &plan.tp, &plan, now, &ctx);
    mark_user_position(&mut pos);
    Ok((pos, cfg.capital))
}

/// The live price `kind`'s bot would fill `symbol` at now, on its own
/// exchange and market: the base of levels given as a percent (TradingView
/// alerts, `tvlink`, Paper only). Refuses as a Paper `open` would for a
/// missing or Live bot.
pub async fn quote(app: &AppHandle, kind: BotKind, symbol: &str) -> Result<f64, String> {
    let cfg = app.state::<BotManager>().config_for(kind).ok_or_else(|| "botNotConfigured".to_string())?;
    if cfg.live || cfg.mode == TradeMode::Live {
        return Err(PAPER_ONLY.to_string());
    }
    if !matches!(kind, BotKind::Futures | BotKind::Spot) {
        return Err(BOT_KIND.to_string());
    }
    let symbol = normalize_symbol(symbol)?;
    live_price(&app.state::<ExchangeManager>(), &cfg.exchange_id, &symbol, kind)
        .await
        .filter(|p| p.is_finite() && *p > 0.0)
        .ok_or_else(|| "priceUnavailable".to_string())
}

pub async fn preview(app: &AppHandle, req: &TradeRequest) -> Result<TradePreview, String> {
    plan(app, req).await.map(|(p, capital)| preview_of(&p, capital))
}

/// Opens the trade now: on the bot's paper book, or for a Live trade as a
/// real market order with its exchange stop and take-profit (the signal
/// bots' real entry). The book is checked again at the open: a double
/// click, or the bot's loop taking the symbol meanwhile, opens nothing twice.
pub async fn open(app: &AppHandle, req: &TradeRequest) -> Result<OpenPosition, String> {
    let (mut pos, _) = plan(app, req).await?;
    let bots = app.state::<BotManager>();
    if req.mode == TradeMode::Live {
        // The confirmation showed the preview's margin; this plan, made now,
        // must put the same money at risk (the backend re-check).
        super::super::live_terms::confirmed_entry(pos.capital, req.confirmed_margin_usdt)?;
        pos = open_real(app, req, pos).await?;
        // A position the exchange holds is booked whatever the book says
        // meanwhile; the real entry refused a symbol already held.
        book::open_position(app, pos.clone());
        bots.push_skip(&pos.symbol, FEED_KEY, Some("live".to_string()));
        bots.ensure_loop(app.clone());
        // The daily stop or "Stop all live" can land while the order was on
        // the wire; their close-all ran on a book without this position.
        if bots.kill_switch_tripped() || !bots.config_for(req.kind).is_some_and(|c| c.live) {
            book::close_now(app, &pos.signal_id, req.kind, "stopped").await;
        }
        return Ok(pos);
    }
    if !book::open_position_if_free(app, pos.clone()) {
        return Err("marketHeld".to_string());
    }
    bots.push_skip(&pos.symbol, FEED_KEY, None);
    bots.ensure_loop(app.clone());
    Ok(pos)
}

/// The real entry of a Live trade planned as `pos` (paper fill model): the
/// gate is re-read now (`live_trading_allowed`), then `live::open_live`
/// places the market order, the stop and the take-profit at the planned
/// size (capped by the pilot). The position records the REAL fill.
async fn open_real(app: &AppHandle, req: &TradeRequest, mut pos: OpenPosition) -> Result<OpenPosition, String> {
    let bots = app.state::<BotManager>();
    let cfg = bots.config_for(req.kind).ok_or_else(|| "botNotConfigured".to_string())?;
    mode_refusal(&cfg, req)?;
    if !super::live::live_trading_allowed(&cfg, req.kind) {
        return Err(LIVE_FUTURES_ONLY.to_string());
    }
    let leverage = precheck::effective_leverage(&cfg, &app.state::<RiskManager>().limits());
    let tcfg = trade_config(&cfg, req, leverage);
    let now = now_ms();
    let sig = synthetic_signal(req, &pos.symbol, pos.entry, now, pos.signal_id.clone());
    let open = super::live::open_live(app, &tcfg, &sig, pos.entry, pos.notional_usdt, leverage, TakeProfitTarget::Tp1)
        .await
        .map_err(str::to_string)?;
    bots.pilot_used_saved(app, req.kind);
    pos.live = true;
    pos.entry = open.entry;
    pos.qty = open.qty;
    pos.entry_order_id = Some(open.entry_order_id);
    pos.unprotected = open.stop_algo_id.is_none();
    pos.stop_algo_id = open.stop_algo_id;
    pos.tp_algo_id = open.tp_algo_id;
    Ok(pos)
}
