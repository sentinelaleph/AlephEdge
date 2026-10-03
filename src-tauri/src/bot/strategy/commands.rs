//! Tauri command surface for the strategy bots (DCA / Grid, paper only).
//! All commands return `Result<_, String>`; an error string is an i18n code
//! (`strategy.errors.*`), optionally `code|field`.
//!
//! No command here can express a real-money intent: there is no live flag
//! to set, and no command reaches the exchange close commands (real orders).

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
    BotRunState, DcaParams, GridParams, GridRange, MarketKind, RestartPolicy, Side, SimOrder,
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

fn max_leverage(app: &AppHandle) -> u8 {
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

fn check(cfg: &StrategyConfig, app: &AppHandle) -> Result<(), String> {
    validate::validate(cfg, max_leverage(app)).map_err(|e| e.to_string())
}

fn view(mgr: &StrategyManager, id: &str) -> Result<StrategyBotView, String> {
    mgr.view(id).ok_or_else(|| "botUnknown".to_string())
}

/// The symbol must exist on the exchange's market list (Binance).
async fn check_symbol(app: &AppHandle, cfg: &StrategyConfig) -> Result<(), String> {
    let list = app
        .state::<ExchangeManager>()
        .symbols(&cfg.exchange_id, cfg.market == MarketKind::Futures)
        .await;
    if list.is_empty() {
        return Err("symbolListUnavailable".into());
    }
    if !list.iter().any(|s| s == &cfg.symbol) {
        return Err("symbolUnknown|symbol".into());
    }
    Ok(())
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
        StrategyKind::Dca => (
            Side::Long,
            StrategyParams::Dca(DcaParams {
                base_order: None,
                safety_order: None,
                base_weight: Some(1.0),
                safety_weight: Some(1.0),
                max_so: 6,
                so_step_pct: 1.5,
                step_scale: 1.0,
                volume_scale: 1.5,
                tp_pct: 1.5,
                trailing_pct: None,
                sl_pct: None,
                max_duration_min: None,
            }),
        ),
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
    Ok(validate::preview(&config, price, max_leverage(&app)))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationReport {
    pub ok: bool,
    pub error: Option<StrategyError>,
}

#[tauri::command]
pub fn strategy_validate(app: AppHandle, config: StrategyConfig) -> Result<ValidationReport, String> {
    let error = validate::validate(&config, max_leverage(&app)).err();
    Ok(ValidationReport {
        ok: error.is_none(),
        error,
    })
}

#[tauri::command]
pub async fn strategy_create(
    app: AppHandle,
    mgr: State<'_, StrategyManager>,
    config: StrategyConfig,
) -> Result<StrategyBotView, String> {
    check(&config, &app)?;
    if mgr.bots().len() >= MAX_STRATEGY_BOTS {
        return Err("maxBots".into());
    }
    let cap = effective_cap_pct(&app, &mgr.risk());
    if config.budget > app.state::<RiskManager>().balance() * cap / 100.0 {
        return Err("budgetCapReached|budget".into());
    }
    check_symbol(&app, &config).await?;
    let now = now_ms();
    let bot = StrategyBot {
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
    };
    let dir = data_dir(&app)?;
    app.state::<StoreManager>()
        .strategy(&dir, |c| db::save_bot(c, &bot, now))?;
    let id = bot.id.clone();
    mgr.put_bot(bot);
    view(&mgr, &id)
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
        return Err("budgetCapReached|budget".into());
    }
    if driver::is_running(bot.state) || mgr.has_open_cycle(&bot.id) {
        let reserved = mgr.reserved_budget(Some(&bot.id)) + new_budget;
        if reserved > limit || reserved + signal_capital > balance {
            return Err("budgetCapReached|budget".into());
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
    check(&config, &app)?;
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
    // The signal bots' daily stop does not hold strategy bots (owner
    // decision 2026-10-01); the portfolio breaker holds the bots inside it.
    if mgr.breaker_holds(&bot) {
        return Err("portfolioDdTripped".into());
    }
    if !app.state::<MembershipManager>().view().active {
        return Err("membershipInactive".into());
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
    let balance = app.state::<RiskManager>().balance();
    let cap = effective_cap_pct(&app, &mgr.risk());
    let reserved = mgr.reserved_budget(Some(&id)) + bot.cfg.budget;
    if reserved > balance * cap / 100.0 {
        return Err("budgetCapReached".into());
    }
    let signal_capital: f64 = app
        .state::<BotManager>()
        .positions_snapshot()
        .iter()
        .map(|p| p.capital)
        .sum();
    if reserved + signal_capital > balance {
        return Err("budgetCapReached".into());
    }
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

/// Hides a stopped, flat bot. History stays (ledger: filter, never delete).
#[tauri::command]
pub async fn strategy_archive(app: AppHandle, mgr: State<'_, StrategyManager>, id: String) -> Result<(), String> {
    let _guard = mgr.op_lock.lock().await;
    let bot = mgr.bot(&id).ok_or("botUnknown")?;
    if driver::is_running(bot.state) {
        return Err("stopFirst".into());
    }
    if mgr.has_open_cycle(&id) {
        return Err("cycleOpenLocked".into());
    }
    let pnl = bot.realized_quote;
    let covered = bot.cfg.portfolio_breaker;
    let mut bot = bot;
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

/// PRESET-READY research presets with their historical-simulation numbers.
#[tauri::command]
pub fn strategy_presets() -> Result<Vec<Preset>, String> {
    Ok(presets::all())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            Err("budgetCapReached|budget".to_string()),
            "200 + 100 reserved > 200"
        );
        assert_eq!(budget_fits(&mgr, &a, 100.0, 1000.0, 20.0, 0.0), Ok(()));
        assert_eq!(
            budget_fits(&mgr, &a, 100.0, 1000.0, 20.0, 850.0),
            Err("budgetCapReached|budget".to_string()),
            "signal capital shares the balance"
        );
        // a stopped, flat bot reserves nothing: only its own cap applies
        a.state = BotRunState::Stopped;
        mgr.put_bot(a.clone());
        assert_eq!(budget_fits(&mgr, &a, 150.0, 1000.0, 20.0, 0.0), Ok(()));
        assert!(budget_fits(&mgr, &a, 201.0, 1000.0, 20.0, 0.0).is_err());
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
