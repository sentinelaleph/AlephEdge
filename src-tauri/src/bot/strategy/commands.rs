//! Tauri command surface for the strategy bots (DCA / Grid, paper only).
//! All commands return `Result<_, String>`; an error string is an i18n code
//! (`strategy.errors.*`), optionally `code|field`.
//!
//! No command here can express a real-money intent: there is no live flag
//! to set, and no command reaches the exchange close commands (real orders).
//! A Live bot is made and armed in `bot::strategy_live`; `strategy_start`
//! only asks it whether a Live bot may start (`start_refusal`).

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::bot::BotManager;
use crate::exchange::ExchangeManager;
use crate::membership::MembershipManager;
use crate::risk::model::RiskLevel;
use crate::risk::RiskManager;
use crate::store::strategy as db;
use crate::store::{utc_day_start_ms, StoreManager};

use super::engine::{self, now_ms};
use super::limits::{budget_cap_pct, DEFAULT_BOT_DD_STOP_PCT, MAX_STRATEGY_BOTS, PORTFOLIO_DD_STOP_PCT};
use super::model::{
    BotRunState, GridParams, GridRange, MarketKind, RestartPolicy, Side, SimOrder,
    Spacing, StartCondition, StrategyBot, StrategyConfig, StrategyKind, StrategyNote,
    StrategyParams, Utilisation, CONFIG_SCHEMA_VERSION,
};
use super::presets::{self, Preset};
use super::stats::{self, ClosedCycle, StrategyStats};
use super::validate::{self, PreviewDto, StrategyError};
use super::{driver, new_bot_id, StrategyBotView, StrategyManager, StrategyRisk};

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|_| "app data dir unavailable".to_string())
}

fn level_index(level: RiskLevel) -> usize {
    match level {
        RiskLevel::Cautious => 0,
        RiskLevel::Calm => 1,
        RiskLevel::Balanced => 2,
        RiskLevel::Ambitious => 3,
        RiskLevel::Greedy => 4,
    }
}

pub(crate) fn max_leverage(app: &AppHandle) -> u8 {
    app.state::<RiskManager>().limits().max_leverage
}

/// Budget cap in force, % of the declared balance: the level's value, or the
/// user's stricter one.
fn effective_cap_pct(app: &AppHandle, risk: &StrategyRisk) -> f64 {
    let level = budget_cap_pct(level_index(app.state::<RiskManager>().limits().level));
    match risk.budget_cap_pct {
        Some(v) if v.is_finite() && v > 0.0 => v.min(level),
        _ => level,
    }
}

/// `budgetCapReached|<field>|<free>|<limit>`: the refusal carries what is
/// still free under the cap and the cap itself (USDT), so the UI can say how
/// far over the budget is without re-deriving the rule.
fn cap_refusal(field: &str, limit: f64, used: f64) -> String {
    format!("budgetCapReached|{field}|{:.0}|{:.0}", (limit - used).max(0.0).floor(), limit)
}

fn check(cfg: &StrategyConfig, app: &AppHandle) -> Result<(), String> {
    validate::validate(cfg, max_leverage(app)).map_err(|e| e.to_string())
}

pub(crate) fn view(mgr: &StrategyManager, id: &str) -> Result<StrategyBotView, String> {
    mgr.view(id).ok_or_else(|| "botUnknown".to_string())
}

/// The symbol must exist on the exchange's market list (Binance).
pub(crate) async fn check_symbol(app: &AppHandle, cfg: &StrategyConfig) -> Result<(), String> {
    let list = app
        .state::<ExchangeManager>()
        .symbols(&cfg.exchange_id, cfg.market == MarketKind::Futures)
        .await;
    symbol_in(&list, cfg)
}

#[tauri::command]
pub fn strategy_list(mgr: State<StrategyManager>) -> Result<Vec<StrategyBotView>, String> {
    Ok(mgr.views())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyDetail {
    pub view: StrategyBotView,
    pub config: StrategyConfig,
    pub open_orders: Vec<SimOrder>,
}

#[tauri::command]
pub fn strategy_detail(mgr: State<StrategyManager>, id: String) -> Result<StrategyDetail, String> {
    let bot = mgr.bot(&id).ok_or("botUnknown")?;
    Ok(StrategyDetail {
        view: view(&mgr, &id)?,
        config: bot.cfg,
        open_orders: mgr.open_orders(&id),
    })
}

/// Rust-served defaults so the form mirrors no bounds.
pub fn default_config(kind: StrategyKind, exchange_id: String, symbol: String) -> StrategyConfig {
    let (side, params) = match kind {
        // The ladder of dca_long_classic, a template that passed its checks
        // (8 orders, 59.6% coverage). The protections below (drawdown stop,
        // BTC gate, breaker) are the manual form's defaults, so
        // this config is still not that template and carries no preset id.
        StrategyKind::Dca => (Side::Long, presets::dca_long_classic().config.params),
        StrategyKind::Grid => (
            Side::Neutral,
            StrategyParams::Grid(GridParams {
                range: GridRange::Relative {
                    lower_pct: 8.0,
                    upper_pct: 8.0,
                },
                n_grids: 16,
                spacing: Spacing::Geom,
                stop_out_pct: Some(3.0),
                trailing_up: false,
                trail_up_limit: None,
                take_profit_pct: None,
                max_duration_min: None,
            }),
        ),
    };
    StrategyConfig {
        schema_version: CONFIG_SCHEMA_VERSION,
        name: match kind {
            StrategyKind::Dca => "DCA".into(),
            StrategyKind::Grid => "Grid".into(),
        },
        exchange_id,
        market: MarketKind::Futures,
        symbol,
        side,
        budget: 1000.0,
        leverage: 1,
        start: StartCondition::Immediately,
        restart: RestartPolicy::default(),
        max_drawdown_pct: Some(DEFAULT_BOT_DD_STOP_PCT),
        pause_on_btc_break: true,
        // Manual DCA and Grid bots start inside the portfolio breaker.
        portfolio_breaker: true,
        params,
        preset_id: None,
    }
}

#[tauri::command]
pub fn strategy_default(kind: StrategyKind, exchange_id: String, symbol: String) -> Result<StrategyConfig, String> {
    Ok(default_config(kind, exchange_id, symbol))
}

#[tauri::command]
pub async fn strategy_preview(
    app: AppHandle,
    config: StrategyConfig,
    last_price: Option<f64>,
    bot_id: Option<String>,
) -> Result<PreviewDto, String> {
    let price = match last_price.filter(|p| p.is_finite() && *p > 0.0) {
        Some(p) => p,
        None => {
            let st = app
                .state::<ExchangeManager>()
                .status(&config.exchange_id, &config.symbol)
                .await;
            match config.market {
                MarketKind::Futures => st.futures_price,
                MarketKind::Spot => st.spot_price,
            }
            .ok_or("priceUnavailable")?
        }
    };
    let mut dto = validate::preview(&config, price, max_leverage(&app));
    let others: Vec<_> = app.state::<StrategyManager>().bots().into_iter().map(|b| (b.id, b.cfg)).collect();
    if shares_live_symbol(&config, bot_id.as_deref(), &others) {
        dto.warnings.push("symbolSharedLive");
    }
    Ok(dto)
}

/// True when another strategy bot trades the same futures symbol. Paper bots
/// can share it, but real money allows one position per symbol
/// (`liveSymbolBusy`): the user should learn that while setting up, not when
/// switching to live. Binance keeps leverage per symbol, which is why two
/// real bots on one symbol could not each keep their own (3Commas warns about
/// exactly this). `bot_id` is the bot being edited, never counted.
pub(crate) fn shares_live_symbol(
    cfg: &StrategyConfig,
    bot_id: Option<&str>,
    others: &[(String, StrategyConfig)],
) -> bool {
    cfg.market == MarketKind::Futures
        && others.iter().any(|(id, o)| {
            Some(id.as_str()) != bot_id
                && o.market == MarketKind::Futures
                && o.exchange_id == cfg.exchange_id
                && o.symbol.eq_ignore_ascii_case(&cfg.symbol)
        })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationReport {
    pub ok: bool,
    pub error: Option<StrategyError>,
    /// Smallest budget at which every order clears the exchange minimum
    /// (price-free: orders scale with the budget).
    pub min_budget: Option<f64>,
}

#[tauri::command]
pub fn strategy_validate(app: AppHandle, config: StrategyConfig) -> Result<ValidationReport, String> {
    let error = validate::validate(&config, max_leverage(&app)).err();
    Ok(ValidationReport {
        ok: error.is_none(),
        error,
        min_budget: validate::min_budget(&config),
    })
}

/// What the budget field shows under itself: the save floor (every order at
/// least 5 USDT, `validate::min_budget`) and, for a Binance futures bot, the
/// floor the Live switch checks (`go_live_checks`: the smallest order at the
/// symbol's own MIN_NOTIONAL). `symbolRules` says whether that second floor
/// is known: "ok", "unavailable" (the exchange did not answer) or
/// "notApplicable" (spot or another exchange: no Live path).
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BudgetFloor {
    pub min_budget: Option<f64>,
    pub orders: u32,
    pub min_order: f64,
    pub symbol_rules: &'static str,
    pub symbol_min_order: Option<f64>,
    pub live_min_budget: Option<f64>,
}

/// Pure half of `strategy_budget_floor`. `rules` = the symbol's exchange
/// minimum: None when not asked (no Live path), Some(None) when the
/// exchange did not answer, Some(Some(min)) when read.
pub(crate) fn budget_floor(cfg: &StrategyConfig, rules: Option<Option<f64>>) -> BudgetFloor {
    let (symbol_rules, symbol_min_order) = match rules {
        None => ("notApplicable", None),
        Some(None) => ("unavailable", None),
        Some(Some(min)) => ("ok", Some(min)),
    };
    BudgetFloor {
        min_budget: validate::min_budget(cfg),
        orders: validate::order_count(cfg),
        min_order: super::limits::MIN_ORDER_NOTIONAL,
        symbol_rules,
        symbol_min_order,
        live_min_budget: symbol_min_order.and_then(|m| validate::min_budget_at(cfg, m)),
    }
}

/// Binance USDT-M rules already read (public exchangeInfo once per symbol per
/// 15 min, not on every keystroke of a form). Shared by the budget floor and
/// the TradingView bot's symbol minimum. None when Binance did not answer.
pub(crate) async fn binance_rules_cached(app: &AppHandle, symbol: &str) -> Option<crate::exchange::providers::binance_rules::SymbolRules> {
    let symbol = symbol.trim().to_ascii_uppercase();
    let hit = RULES_CACHE.get_or_init(Default::default).lock().ok().and_then(|map| {
        map.get(&symbol)
            .filter(|(at, _)| at.elapsed() < std::time::Duration::from_secs(15 * 60))
            .map(|(_, r)| *r)
    });
    if hit.is_some() {
        return hit;
    }
    let read = app.state::<ExchangeManager>().bn_symbol_rules(&symbol).await.ok();
    if let (Some(r), Ok(mut map)) = (read, RULES_CACHE.get_or_init(Default::default).lock()) {
        map.insert(symbol, (std::time::Instant::now(), r));
    }
    read
}

static RULES_CACHE: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, crate::exchange::providers::binance_rules::SymbolRules)>>,
> = std::sync::OnceLock::new();

#[tauri::command]
pub async fn strategy_budget_floor(app: AppHandle, config: StrategyConfig) -> Result<BudgetFloor, String> {
    let live_path = config.exchange_id == "binance" && config.market == MarketKind::Futures;
    if !live_path {
        return Ok(budget_floor(&config, None));
    }
    let min = binance_rules_cached(&app, &config.symbol).await.map(|r| r.min_notional);
    Ok(budget_floor(&config, Some(min)))
}

/// The preset id stays only while the config still describes that preset's
/// historical simulation: an edit, or a clone that changed a setting, drops
/// it, or the bot detail would name a preset its settings have left.
fn with_provenance(mut config: StrategyConfig) -> StrategyConfig {
    if config.preset_id.is_some() && !presets::keeps_provenance(&config) {
        config.preset_id = None;
    }
    config
}

#[tauri::command]
pub async fn strategy_create(
    app: AppHandle,
    mgr: State<'_, StrategyManager>,
    config: StrategyConfig,
) -> Result<StrategyBotView, String> {
    let now = now_ms();
    let bot = new_stopped_bot(config, max_leverage(&app), mgr.bots().len(), budget_limit(&app, &mgr), now)?;
    check_symbol(&app, &bot.cfg).await?;
    let id = persist_new_bot(&app, &mgr, bot, now)?;
    view(&mgr, &id)
}

/// The budget a new bot may have: the declared balance times the cap in force.
pub(crate) fn budget_limit(app: &AppHandle, mgr: &StrategyManager) -> f64 {
    let cap = effective_cap_pct(app, &mgr.risk());
    app.state::<RiskManager>().balance() * cap / 100.0
}

/// The checks of `strategy_create` that need no exchange, and the new bot:
/// stopped, simulated (a strategy config cannot express real money), with no
/// history. `existing` = bots the desk holds now. Shared with the settings
/// import (app/settings_transfer.rs); the symbol check follows it.
pub(crate) fn new_stopped_bot(
    config: StrategyConfig,
    max_leverage: u8,
    existing: usize,
    budget_limit: f64,
    now: u64,
) -> Result<StrategyBot, String> {
    validate::validate(&config, max_leverage).map_err(|e| e.to_string())?;
    let config = with_provenance(config);
    if existing >= MAX_STRATEGY_BOTS {
        return Err("maxBots".into());
    }
    if config.budget > budget_limit {
        return Err(cap_refusal("budget", budget_limit, 0.0));
    }
    Ok(StrategyBot {
        id: new_bot_id(),
        peak_equity: config.budget,
        cfg: StrategyConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            name: config.name.trim().to_string(),
            ..config
        },
        state: BotRunState::Stopped,
        created_at: now,
        cycles_done: 0,
        realized_quote: 0.0,
        max_dd_quote: 0.0,
        dead_reason: None,
        next_decision_ms: 0,
        chain_cycles: 0,
        start_triggered: false,
        util: Utilisation::default(),
        archived_at: None,
    })
}

/// Saves a new bot and adds it to the desk; returns its id.
pub(crate) fn persist_new_bot(app: &AppHandle, mgr: &StrategyManager, bot: StrategyBot, now: u64) -> Result<String, String> {
    let dir = data_dir(app)?;
    app.state::<StoreManager>()
        .strategy(&dir, |c| db::save_bot(c, &bot, now))?;
    let id = bot.id.clone();
    mgr.put_bot(bot);
    Ok(id)
}

/// The symbol check of `strategy_create` against a list already read
/// (`ExchangeManager::symbols`).
pub(crate) fn symbol_in(list: &[String], cfg: &StrategyConfig) -> Result<(), String> {
    if list.is_empty() {
        return Err("symbolListUnavailable".into());
    }
    if !list.iter().any(|s| s == &cfg.symbol) {
        return Err("symbolUnknown|symbol".into());
    }
    Ok(())
}

/// Fields that may change while a cycle is open (never a reservation).
fn runtime_safe(old: &StrategyConfig, new: &StrategyConfig) -> bool {
    old.exchange_id == new.exchange_id
        && old.market == new.market
        && old.symbol == new.symbol
        && old.side == new.side
        && old.budget == new.budget
        && old.leverage == new.leverage
        && old.params == new.params
}

/// Budget cap on an edit. A bot that already reserves its budget (armed,
/// or holding a cycle) must fit with every other reservation, exactly as
/// `strategy_start` checks it; otherwise raising an armed bot's budget
/// would slip past the aggregate cap.
fn budget_fits(
    mgr: &StrategyManager,
    bot: &StrategyBot,
    new_budget: f64,
    balance: f64,
    cap_pct: f64,
    signal_capital: f64,
) -> Result<(), String> {
    let limit = balance * cap_pct / 100.0;
    if new_budget > limit {
        return Err(cap_refusal("budget", limit, 0.0));
    }
    if driver::is_running(bot.state) || mgr.has_open_cycle(&bot.id) {
        let others = mgr.reserved_budget(Some(&bot.id));
        if others + new_budget > limit {
            return Err(cap_refusal("budget", limit, others));
        }
        if others + new_budget + signal_capital > balance {
            return Err(cap_refusal("budget", balance, others + signal_capital));
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn strategy_update(
    app: AppHandle,
    mgr: State<'_, StrategyManager>,
    id: String,
    config: StrategyConfig,
) -> Result<StrategyBotView, String> {
    let _guard = mgr.op_lock.lock().await;
    let mut bot = mgr.bot(&id).ok_or("botUnknown")?;
    // Real money is on (or still held): the checks made when it was switched
    // on would no longer hold after an edit. Back to paper first.
    if crate::bot::strategy_live::locks_bot(&app, &id) {
        return Err("liveEditLocked".into());
    }
    check(&config, &app)?;
    let config = with_provenance(config);
    if mgr.has_open_cycle(&id) {
        if !runtime_safe(&bot.cfg, &config) {
            return Err("cycleOpenLocked".into());
        }
    } else if driver::is_running(bot.state)
        && (config.symbol != bot.cfg.symbol || config.market != bot.cfg.market || config.side != bot.cfg.side)
    {
        // An armed bot keeps its (symbol, market, side) slot; moving it is a
        // stop, edit, start.
        return Err("stopFirst".into());
    }
    let cap = effective_cap_pct(&app, &mgr.risk());
    let signal_capital: f64 = app
        .state::<BotManager>()
        .positions_snapshot()
        .iter()
        .map(|p| p.capital)
        .sum();
    budget_fits(&mgr, &bot, config.budget, app.state::<RiskManager>().balance(), cap, signal_capital)?;
    if config.symbol != bot.cfg.symbol || config.market != bot.cfg.market {
        check_symbol(&app, &config).await?;
    }
    if config.budget != bot.cfg.budget {
        // Equity = budget + P&L: the peak moves with the budget, or a cut
        // budget reads as a drawdown (and trips the bot's drawdown stop on
        // its next bar).
        bot.peak_equity = if bot.cycles_done == 0 {
            config.budget
        } else {
            bot.peak_equity + (config.budget - bot.cfg.budget)
        };
    }
    let breaker_moved = config.portfolio_breaker != bot.cfg.portfolio_breaker;
    bot.cfg = StrategyConfig {
        schema_version: CONFIG_SCHEMA_VERSION,
        name: config.name.trim().to_string(),
        ..config
    };
    if breaker_moved {
        // The bot's P&L joins or leaves the breaker's P&L; the peak moves
        // with it, so switching never reads as a drawdown or a recovery.
        let pnl = mgr.bot_pnl(&id);
        mgr.shift_peak(if bot.cfg.portfolio_breaker { pnl } else { -pnl });
        engine::save_breaker(&app, &mgr);
    }
    engine::save_bot(&app, &bot);
    mgr.put_bot(bot);
    view(&mgr, &id)
}

#[tauri::command]
pub async fn strategy_start(app: AppHandle, mgr: State<'_, StrategyManager>, id: String) -> Result<StrategyBotView, String> {
    // Under `op_lock`: a bar walk takes the bot out and puts its walked copy
    // back, so an unlocked read-modify-write here could lose the start, or
    // write a pre-walk copy over the walk's realised P&L and cycle count.
    let _guard = mgr.op_lock.lock().await;
    let mut bot = mgr.bot(&id).ok_or("botUnknown")?;
    if bot.state == BotRunState::Dead {
        return Err("botDead".into());
    }
    // The signal bots' daily stop does not hold strategy bots; the portfolio
    // breaker holds the bots inside it.
    if mgr.breaker_holds(&bot) {
        return Err("portfolioDdTripped".into());
    }
    if !app.state::<MembershipManager>().view().active {
        return Err("membershipInactive".into());
    }
    if let Some(code) = crate::bot::strategy_live::start_refusal(&app, &id) {
        return Err(code.into());
    }
    if let Some(code) = crate::app::data_health::trading_blocked(&app) {
        return Err(code);
    }
    check(&bot.cfg, &app)?;
    // One running bot per (symbol, market, side): grids must not cross themselves.
    if mgr.bots().iter().any(|b| {
        b.id != id
            && (driver::is_running(b.state) || mgr.has_open_cycle(&b.id))
            && b.cfg.symbol == bot.cfg.symbol
            && b.cfg.market == bot.cfg.market
            && b.cfg.side == bot.cfg.side
    }) {
        return Err("symbolBusy".into());
    }
    reservation_check(&app, &mgr, &bot)?;
    bot.state = BotRunState::Armed;
    bot.next_decision_ms = now_ms();
    bot.chain_cycles = 0;
    bot.start_triggered = false;
    engine::save_bot(&app, &bot);
    mgr.put_bot(bot);
    mgr.clear_last_note(&id);
    app.state::<BotManager>().ensure_loop(app.clone());
    view(&mgr, &id)
}

/// The budget a running bot reserves must fit the DCA/Grid cap together
/// with every other reservation, and the balance together with the signal
/// bots' capital. Pure: `others` = what the other bots reserve.
pub(super) fn fits_reservation(budget: f64, others: f64, balance: f64, cap_pct: f64, signal_capital: f64) -> Result<(), String> {
    let reserved = others + budget;
    let limit = balance * cap_pct / 100.0;
    if reserved > limit {
        return Err(cap_refusal("", limit, others));
    }
    if reserved + signal_capital > balance {
        return Err(cap_refusal("", balance, others + signal_capital));
    }
    Ok(())
}

fn reservation_check(app: &AppHandle, mgr: &StrategyManager, bot: &StrategyBot) -> Result<(), String> {
    let signal_capital: f64 = app
        .state::<BotManager>()
        .positions_snapshot()
        .iter()
        .map(|p| p.capital)
        .sum();
    fits_reservation(
        bot.cfg.budget,
        mgr.reserved_budget(Some(&bot.id)),
        app.state::<RiskManager>().balance(),
        effective_cap_pct(app, &mgr.risk()),
        signal_capital,
    )
}

/// The Start checks that depend on the risk settings, run again for a bot
/// that was running before a restart: every bound with the level's
/// leverage ceiling (`validate`), then the budget cap and the balance.
/// Err = the refusal code; the bot then stays Stopped.
pub(super) fn resume_refusal(app: &AppHandle, mgr: &StrategyManager, bot: &StrategyBot) -> Result<(), String> {
    // Settings that could not be read stand in as defaults: nothing resumes
    // on them (app/data_health.rs).
    if let Some(code) = crate::app::data_health::trading_blocked(app) {
        return Err(code);
    }
    check(&bot.cfg, app)?;
    reservation_check(app, mgr, bot)
}

/// Stops NEW cycles only; an open cycle keeps being managed.
#[tauri::command]
pub async fn strategy_stop(app: AppHandle, mgr: State<'_, StrategyManager>, id: String) -> Result<StrategyBotView, String> {
    // Same lock as `strategy_start`: a stop pressed during a bar walk must
    // neither be overwritten nor overwrite the walk's results.
    let _guard = mgr.op_lock.lock().await;
    let mut bot = mgr.bot(&id).ok_or("botUnknown")?;
    if bot.state != BotRunState::Dead {
        bot.state = BotRunState::Stopped;
    }
    engine::save_bot(&app, &bot);
    mgr.put_bot(bot);
    view(&mgr, &id)
}

/// Closes the open paper cycle at market (taker + slippage), exit `manual`,
/// and stops the bot. Only "market" exists in paper v1 (no wallet to keep
/// coins in).
#[tauri::command]
pub async fn strategy_close(
    app: AppHandle,
    mgr: State<'_, StrategyManager>,
    id: String,
    mode: String,
) -> Result<StrategyBotView, String> {
    if mode != "market" {
        return Err("closeModeUnsupported".into());
    }
    let _guard = mgr.op_lock.lock().await;
    engine::close_one_locked(&app, &mgr, &id, super::model::ExitReason::Manual, false).await?;
    if let Some(mut b) = mgr.bot(&id) {
        if b.state != BotRunState::Dead {
            b.state = BotRunState::Stopped;
            engine::save_bot(&app, &b);
            mgr.put_bot(b);
        }
    }
    view(&mgr, &id)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseAllReport {
    pub closed: u32,
    /// Cycles left open because no honest price existed (retry later).
    pub unpriced: u32,
}

#[tauri::command]
pub async fn strategy_close_all(app: AppHandle, reason: Option<String>) -> Result<CloseAllReport, String> {
    let reason = reason.unwrap_or_else(|| "manual".into());
    let (closed, unpriced) = engine::force_close_all(&app, &reason).await;
    Ok(CloseAllReport { closed, unpriced })
}

/// Deletes (hides) a paper bot in one step: an open cycle closes at market
/// first (as Close cycle does), a running bot stops. History stays (ledger:
/// filter, never delete). A bot on real money is refused: it goes back to
/// paper first, so no real position is closed from a delete.
#[tauri::command]
pub async fn strategy_archive(app: AppHandle, mgr: State<'_, StrategyManager>, id: String) -> Result<(), String> {
    let _guard = mgr.op_lock.lock().await;
    mgr.bot(&id).ok_or("botUnknown")?;
    if crate::bot::strategy_live::locks_bot(&app, &id) {
        return Err("liveEditLocked".into());
    }
    if mgr.has_open_cycle(&id) {
        engine::close_one_locked(&app, &mgr, &id, super::model::ExitReason::Manual, false).await?;
    }
    let bot = mgr.bot(&id).ok_or("botUnknown")?;
    if mgr.has_open_cycle(&id) {
        return Err("cycleOpenLocked".into());
    }
    let pnl = bot.realized_quote;
    let covered = bot.cfg.portfolio_breaker;
    let mut bot = bot;
    if bot.state != BotRunState::Dead {
        bot.state = BotRunState::Stopped;
    }
    bot.archived_at = Some(now_ms());
    engine::save_bot(&app, &bot);
    mgr.remove_bot(&id);
    if covered {
        mgr.shift_peak(-pnl);
        engine::save_breaker(&app, &mgr);
    }
    Ok(())
}

#[tauri::command]
pub fn strategy_cycles(app: AppHandle, id: String, limit: Option<u32>) -> Result<Vec<db::CycleRow>, String> {
    let dir = data_dir(&app)?;
    app.state::<StoreManager>()
        .strategy(&dir, |c| db::list_cycles(c, Some(&id), limit.unwrap_or(100).min(1000)))
}

#[tauri::command]
pub fn strategy_orders(app: AppHandle, id: String) -> Result<Vec<db::OrderRow>, String> {
    let dir = data_dir(&app)?;
    app.state::<StoreManager>().strategy(&dir, |c| db::list_orders(c, &id))
}

#[tauri::command]
pub fn strategy_fills(app: AppHandle, id: String, limit: Option<u32>) -> Result<Vec<db::FillRow>, String> {
    let dir = data_dir(&app)?;
    app.state::<StoreManager>()
        .strategy(&dir, |c| db::list_fills(c, Some(&id), limit.unwrap_or(200).min(5000)))
}

/// Real recorded series only; an empty array means nothing was recorded.
#[tauri::command]
pub fn strategy_equity(app: AppHandle, id: String, from_ms: Option<u64>) -> Result<Vec<db::EquityRow>, String> {
    let dir = data_dir(&app)?;
    app.state::<StoreManager>()
        .strategy(&dir, |c| db::list_equity(c, &id, from_ms.unwrap_or(0)))
}

/// `scope`: "bot" (with `id`) or "all" (bots in the desk, archived excluded).
#[tauri::command]
pub fn strategy_stats(
    app: AppHandle,
    mgr: State<StrategyManager>,
    scope: String,
    id: Option<String>,
) -> Result<StrategyStats, String> {
    let bots: Vec<StrategyBot> = match scope.as_str() {
        "bot" => {
            let id = id.ok_or("botUnknown")?;
            vec![mgr.bot(&id).ok_or("botUnknown")?]
        }
        "all" => mgr.bots(),
        _ => return Err("scopeInvalid".into()),
    };
    let dir = data_dir(&app)?;
    let mut cycles = Vec::new();
    for b in &bots {
        let rows = app
            .state::<StoreManager>()
            .strategy(&dir, |c| db::list_cycles(c, Some(&b.id), u32::MAX))?;
        cycles.extend(rows.into_iter().filter(|r| r.closed_at.is_some()).map(|r| {
            // The row's own money: realized_quote is gross (cash + fees +
            // funding), so this is the cycle's net P&L exactly. Rebuilding it
            // from pnl_pct_budget x TODAY's budget misstated every cycle
            // closed before a budget edit (allowed on a stopped, flat bot).
            let pnl_quote = r.realized_quote - r.fees_quote - r.funding_quote;
            // The budget the stored % was taken against (that cycle's).
            let budget = match r.pnl_pct_budget {
                Some(p) if p.is_finite() && p.abs() > 1e-9 => pnl_quote * 100.0 / p,
                _ => b.cfg.budget,
            };
            (pnl_quote, budget, r)
        }).map(|(pnl_quote, budget, r)| ClosedCycle {
            pnl_quote,
            budget,
            exit: r.exit_reason.unwrap_or_default(),
            fees: r.fees_quote,
            funding: r.funding_quote,
            so_filled: r.so_filled,
            grid_closing_fills: r.grid_closing_fills,
        }));
    }
    Ok(stats::compute(&bots, &cycles))
}

/// Paper DCA / Grid money for the Dashboard, History and alerts: totals over
/// every closed cycle (deleted bots included) and the newest closed cycles.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyPnl {
    pub totals: db::StrategyPnlTotals,
    pub recent: Vec<db::ClosedCycleRow>,
}

#[tauri::command]
pub fn strategy_pnl(app: AppHandle, limit: Option<u32>) -> Result<StrategyPnl, String> {
    let dir = data_dir(&app)?;
    // Paper money only. In a live build a bot on real money keeps its
    // simulated cycles as the decision book; its real result is reported
    // apart (strategy_live_report), and the two are never summed.
    // An unreadable real-money state excludes nothing here: this is a report,
    // and the bots it would hide are already refused a resume (engine.rs).
    let exclude = if crate::bot::model::LIVE_TRADING_ENABLED {
        crate::bot::strategy_live::stored_live_ids(&app).unwrap_or_default()
    } else {
        Default::default()
    };
    let day = utc_day_start_ms();
    let limit = limit.unwrap_or(50).min(500);
    app.state::<StoreManager>().strategy(&dir, |c| {
        Ok(StrategyPnl {
            totals: db::pnl_totals(c, day, &exclude)?,
            recent: db::recent_closed_cycles(c, limit, &exclude)?,
        })
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPaths {
    pub cycles_path: String,
    pub fills_path: String,
}

#[tauri::command]
pub fn strategy_export_csv(app: AppHandle) -> Result<ExportPaths, String> {
    let dir = data_dir(&app)?;
    let (cycles, fills) = app.state::<StoreManager>().strategy(&dir, db::export_csv)?;
    let cp = dir.join("strategy-cycles-export.csv");
    let fp = dir.join("strategy-fills-export.csv");
    std::fs::write(&cp, cycles).map_err(|_| "csv write failed".to_string())?;
    std::fs::write(&fp, fills).map_err(|_| "csv write failed".to_string())?;
    Ok(ExportPaths {
        cycles_path: cp.to_string_lossy().to_string(),
        fills_path: fp.to_string_lossy().to_string(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyRiskView {
    pub portfolio_dd_pct: f64,
    pub portfolio_dd_default_pct: f64,
    pub budget_cap_pct: f64,
    pub level_budget_cap_pct: f64,
    pub max_leverage: u8,
    pub balance: f64,
    pub reserved_budget: f64,
    pub pnl_quote: f64,
    pub peak_pnl_quote: f64,
    pub tripped: bool,
    pub day_pnl_quote: f64,
    /// Bots inside the portfolio breaker, of `bot_count`, and their budgets
    /// (the base of the breaker's limit).
    pub breaker_bots: u32,
    pub breaker_budget: f64,
    pub bot_count: u32,
}

fn risk_view(app: &AppHandle, mgr: &StrategyManager) -> StrategyRiskView {
    let risk = mgr.risk();
    let (breaker_bots, bot_count) = mgr.breaker_coverage();
    StrategyRiskView {
        portfolio_dd_pct: risk.portfolio_dd_pct,
        portfolio_dd_default_pct: PORTFOLIO_DD_STOP_PCT,
        budget_cap_pct: effective_cap_pct(app, &risk),
        level_budget_cap_pct: budget_cap_pct(level_index(app.state::<RiskManager>().limits().level)),
        max_leverage: max_leverage(app),
        balance: app.state::<RiskManager>().balance(),
        reserved_budget: mgr.reserved_budget(None),
        pnl_quote: mgr.breaker_pnl(),
        peak_pnl_quote: mgr.portfolio_peak(),
        tripped: mgr.portfolio_tripped(),
        day_pnl_quote: mgr.day_pnl_quote(utc_day_start_ms()),
        breaker_bots,
        breaker_budget: mgr.breaker_budget(),
        bot_count,
    }
}

#[tauri::command]
pub fn strategy_risk_get(app: AppHandle, mgr: State<StrategyManager>) -> Result<StrategyRiskView, String> {
    Ok(risk_view(&app, &mgr))
}

/// Tighten-only: the portfolio breaker can only be set at or below its
/// default; the budget cap at or below the level's. `rearm` clears a trip
/// from an earlier UTC day.
#[tauri::command]
pub async fn strategy_risk_set(
    app: AppHandle,
    mgr: State<'_, StrategyManager>,
    portfolio_dd_pct: Option<f64>,
    budget_cap_pct: Option<f64>,
    rearm: Option<bool>,
) -> Result<StrategyRiskView, String> {
    // The breaker is read and tripped inside the tick under `op_lock`.
    let _guard = mgr.op_lock.lock().await;
    let mut risk = mgr.risk();
    if let Some(v) = portfolio_dd_pct {
        if !(v.is_finite() && v > 0.0 && v <= PORTFOLIO_DD_STOP_PCT) {
            return Err("portfolioDdInvalid".into());
        }
        risk.portfolio_dd_pct = v;
    }
    if let Some(v) = budget_cap_pct {
        let level = budget_cap_pct_for(&app);
        if !(v.is_finite() && v > 0.0 && v <= level) {
            return Err("budgetCapInvalid".into());
        }
        risk.budget_cap_pct = Some(v);
    }
    mgr.set_risk_values(risk);
    engine::save_risk(&app, &risk);
    if rearm == Some(true) {
        mgr.rearm_breaker(utc_day_start_ms())?;
        engine::save_breaker(&app, &mgr);
    }
    Ok(risk_view(&app, &mgr))
}

fn budget_cap_pct_for(app: &AppHandle) -> f64 {
    budget_cap_pct(level_index(app.state::<RiskManager>().limits().level))
}

#[tauri::command]
pub fn strategy_notes(mgr: State<StrategyManager>) -> Result<Vec<StrategyNote>, String> {
    Ok(mgr.notes())
}

/// A preset with the smallest budget at which every order of its config
/// clears the exchange minimum (price-free, as `validate::min_budget`).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetView {
    #[serde(flatten)]
    pub preset: Preset,
    pub min_budget: Option<f64>,
    /// The same settings on a wider pair list, where measured.
    pub wide_universe: Option<presets::WideUniverse>,
}

/// PRESET-READY research presets with their historical-simulation numbers.
#[tauri::command]
pub fn strategy_presets() -> Result<Vec<PresetView>, String> {
    Ok(preset_views())
}

fn preset_views() -> Vec<PresetView> {
    presets::all()
        .into_iter()
        .map(|preset| PresetView {
            min_budget: validate::min_budget(&preset.config),
            wide_universe: presets::wide_universe(preset.id),
            preset,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_budget_floor_says_which_minimum_is_known() {
        let cfg = default_config(StrategyKind::Dca, "binance".into(), "BTCUSDT".into());
        let save = validate::min_budget(&cfg);
        let spot = budget_floor(&cfg, None);
        assert_eq!((spot.symbol_rules, spot.min_budget, spot.live_min_budget), ("notApplicable", save, None));
        assert_eq!(spot.min_order, 5.0);
        assert_eq!(spot.orders, validate::order_count(&cfg));
        let down = budget_floor(&cfg, Some(None));
        assert_eq!((down.symbol_rules, down.symbol_min_order, down.live_min_budget), ("unavailable", None, None));
        let btc = budget_floor(&cfg, Some(Some(100.0)));
        assert_eq!(btc.symbol_rules, "ok");
        assert_eq!(btc.live_min_budget, validate::min_budget_at(&cfg, 100.0));
        assert!(btc.live_min_budget.unwrap() > save.unwrap() * 19.0);
        // a 5 USDT symbol (or one with no filter) needs nothing above the save floor
        assert_eq!(budget_floor(&cfg, Some(Some(0.0))).live_min_budget, save);
        let json = serde_json::to_value(&btc).unwrap();
        assert_eq!(json["symbolRules"], "ok");
        assert_eq!(json["liveMinBudget"].as_f64(), btc.live_min_budget);
    }

    #[test]
    fn every_preset_carries_its_minimum_budget_flat_in_the_payload() {
        let views = preset_views();
        assert_eq!(views.len(), presets::all().len());
        for v in &views {
            let min = v.min_budget.unwrap_or_else(|| panic!("{} has no minimum budget", v.preset.id));
            assert_eq!(Some(min), validate::min_budget(&v.preset.config), "{}", v.preset.id);
            let json = serde_json::to_value(v).unwrap();
            assert_eq!(json["id"], v.preset.id, "flattened");
            assert_eq!(json["minBudget"].as_f64(), Some(min));
        }
        let classic = views.iter().find(|v| v.preset.id == "dca_long_classic").unwrap();
        let w = classic.wide_universe.as_ref().expect("classic was run on the top 30");
        assert_eq!(w.top_n, 30);
        assert!(views.iter().filter(|v| v.wide_universe.is_some()).count() == 1, "only measured templates");
    }

    #[test]
    fn a_second_futures_bot_on_a_symbol_is_flagged_but_never_the_edited_bot_itself() {
        let a = default_config(StrategyKind::Dca, "binance".into(), "BTCUSDT".into());
        let others = vec![("bot-a".to_string(), a.clone())];
        // The same bot being edited does not count.
        assert!(!shares_live_symbol(&a, Some("bot-a"), &others));
        // A new bot on the same symbol does.
        assert!(shares_live_symbol(&a, None, &others));
        // Another symbol, a spot bot, or another exchange does not.
        let mut eth = a.clone();
        eth.symbol = "ETHUSDT".into();
        assert!(!shares_live_symbol(&eth, None, &others));
        let mut spot = a.clone();
        spot.market = MarketKind::Spot;
        assert!(!shares_live_symbol(&spot, None, &others));
        let mut other_venue = a.clone();
        other_venue.exchange_id = "bybit".into();
        assert!(!shares_live_symbol(&other_venue, None, &others));
    }

    #[test]
    fn rust_defaults_validate() {
        for kind in [StrategyKind::Dca, StrategyKind::Grid] {
            let c = default_config(kind, "binance".into(), "BTCUSDT".into());
            assert_eq!(validate::validate(&c, 2), Ok(()), "{kind:?}");
        }
    }

    #[test]
    fn an_armed_bot_cannot_raise_its_budget_past_the_aggregate_cap() {
        // balance 1000, cap 20% = 200; two armed bots of 100 reserve 200.
        let mgr = StrategyManager::new();
        let mut a = crate::bot::strategy::driver::tests::sample_bot();
        a.cfg.budget = 100.0;
        let mut b = a.clone();
        b.id = "sb_bbbbbbbbbbbb".into();
        mgr.put_bot(a.clone());
        mgr.put_bot(b);
        assert_eq!(
            budget_fits(&mgr, &a, 200.0, 1000.0, 20.0, 0.0),
            Err("budgetCapReached|budget|100|200".to_string()),
            "200 + 100 reserved > 200"
        );
        assert_eq!(budget_fits(&mgr, &a, 100.0, 1000.0, 20.0, 0.0), Ok(()));
        assert_eq!(
            budget_fits(&mgr, &a, 100.0, 1000.0, 20.0, 850.0),
            Err("budgetCapReached|budget|50|1000".to_string()),
            "signal capital shares the balance"
        );
        // a stopped, flat bot reserves nothing: only its own cap applies
        a.state = BotRunState::Stopped;
        mgr.put_bot(a.clone());
        assert_eq!(budget_fits(&mgr, &a, 150.0, 1000.0, 20.0, 0.0), Ok(()));
        assert!(budget_fits(&mgr, &a, 201.0, 1000.0, 20.0, 0.0).is_err());
    }

    #[test]
    fn default_dca_ladder_is_the_tested_one() {
        let c = default_config(StrategyKind::Dca, "binance".into(), "BTCUSDT".into());
        assert_eq!(c.params, presets::dca_long_classic().config.params);
        let StrategyParams::Dca(p) = &c.params else { unreachable!() };
        assert!(*super::super::dca::deviations(p).last().unwrap() > 50.0);
        // the manual form's protections stay, so no preset id
        assert_eq!(c.max_drawdown_pct, Some(DEFAULT_BOT_DD_STOP_PCT));
        assert!(c.pause_on_btc_break && c.portfolio_breaker);
        assert_eq!(c.preset_id, None);
    }

    #[test]
    fn a_changed_preset_config_loses_its_preset_id() {
        let mut c = presets::dca_long_classic().config;
        c.symbol = "BTCUSDT".into();
        c.name = "Renamed".into();
        assert_eq!(with_provenance(c.clone()).preset_id.as_deref(), Some("dca_long_classic"));
        c.leverage = 2;
        assert_eq!(with_provenance(c).preset_id, None);
    }

    #[test]
    fn the_reservation_check_is_the_start_rule() {
        // balance 1000 at 20%: 200 for every DCA/Grid budget together
        assert_eq!(fits_reservation(200.0, 0.0, 1000.0, 20.0, 0.0), Ok(()));
        assert_eq!(fits_reservation(150.0, 100.0, 1000.0, 20.0, 0.0), Err("budgetCapReached||100|200".to_string()));
        assert_eq!(fits_reservation(100.0, 0.0, 1000.0, 20.0, 950.0), Err("budgetCapReached||50|1000".to_string()));
    }

    #[test]
    fn runtime_edits_never_touch_the_reservation() {
        let a = default_config(StrategyKind::Dca, "binance".into(), "BTCUSDT".into());
        let mut b = a.clone();
        b.name = "renamed".into();
        b.max_drawdown_pct = Some(10.0);
        b.restart.cooldown_min = 5;
        assert!(runtime_safe(&a, &b));
        b.budget = 2000.0;
        assert!(!runtime_safe(&a, &b));
        let mut c = a.clone();
        c.leverage = 2;
        assert!(!runtime_safe(&a, &c));
    }
}
