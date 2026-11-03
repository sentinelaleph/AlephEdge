//! One bar's action on one symbol: the key claimed first (never acted on
//! twice), then the plan of spec section 6 (Close first, conflict, flip,
//! already open), each leg checked, through the entry the TradingView bot
//! uses (`user_entry::open`), and the result row written. Also the lookup
//! of real entries whose outcome stayed unknown is in `pending`.

use tauri::{AppHandle, Emitter, Manager};

use crate::bot::engine::user_trade::{self, Level, TradeRequest};
use crate::bot::engine::{live_price, precheck, tv_trade, user_entry};
use crate::bot::model::{BotConfig, BotKind, OpenPosition, PineSettings, TradeMode};
use crate::bot::{BotManager, TRIGGER_EXIT_REASON};
use crate::exchange::ExchangeManager;
use crate::pine::{PineBar, RunVerdict};
use crate::risk::RiskManager;
use crate::signal::model::Direction;
use crate::store::pine::Claim;
use crate::store::StoreManager;

use super::pending::notify;
use super::rules::{self, Fired, STALE};
use super::types::{FiredTrigger, PinePending, PineResult};
use super::{PineManager, RESULT_EVENT};

fn now_ms() -> u64 {
    crate::bot::engine::now_ms()
}

fn side(d: Direction) -> &'static str {
    match d {
        Direction::Long => "long",
        Direction::Short => "short",
    }
}

/// What one open leg left on the row.
#[derive(Default)]
struct Leg {
    counted: bool,
    pending: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn act(app: &AppHandle, cfg: &BotConfig, p: &PineSettings, symbol: &str, close_ms: u64, bar: PineBar, v: &RunVerdict, fired: &Fired) {
    let mgr = app.state::<PineManager>();
    let bots = app.state::<BotManager>();
    let key = rules::event_key(&p.script_hash, symbol, close_ms);
    let mut row = PineResult {
        key: key.clone(),
        symbol: symbol.to_string(),
        script_hash: p.script_hash.clone(),
        timeframe: p.timeframe.as_str().to_string(),
        bar_time: bar.t,
        bar_close_ms: close_ms,
        bar_close: bar.c,
        handled_at: now_ms(),
        triggers: fired.ids.iter().map(|(id, a)| FiredTrigger { id: id.clone(), title: mgr.title(&p.script_hash, id), action: *a }).collect(),
        outcome: "pineRunning".to_string(),
        bot_name: p.name.clone(),
        mode: if cfg.mode == TradeMode::Live { "live" } else { "paper" }.to_string(),
        ..Default::default()
    };
    let Ok(dir) = app.path().app_data_dir() else {
        return mgr.note(symbol, "appDataDirUnavailable");
    };
    let store = app.state::<StoreManager>();
    let body = serde_json::to_string(&row).unwrap_or_default();
    let claim = Claim { key: &key, script_hash: &p.script_hash, symbol, bar_close_ms: close_ms, now_ms: now_ms(), body: &body };
    match store.pine_claim(&dir, &claim) {
        Ok(true) => {}
        Ok(false) => return, // acted on already (a restart or a retry)
        Err(e) => {
            // Never an order without its key on disk.
            mgr.note(symbol, "storeWriteFailed");
            return bots.push_skip(symbol, "storeWriteFailed", Some(e));
        }
    }
    let mut leg = Leg::default();
    row.outcome = match run_plan(app, cfg, p, &mut row, v, fired, &mut leg).await {
        Ok(code) | Err(code) => code,
    };
    row.handled_at = now_ms();
    save(app, &row, leg.counted, leg.pending.as_deref());
    notify(app, cfg.mode == TradeMode::Live, &row).await;
}

pub(super) fn save(app: &AppHandle, row: &PineResult, counted: bool, pending: Option<&str>) {
    let saved = app.path().app_data_dir().map_err(|_| "appDataDirUnavailable".to_string()).and_then(|dir| {
        let body = serde_json::to_string(row).map_err(|_| "storeWriteFailed".to_string())?;
        app.state::<StoreManager>().pine_update(&dir, &row.key, &body, counted, pending)
    });
    if let Err(e) = saved {
        app.state::<BotManager>().push_skip(&row.symbol, "storeWriteFailed", Some(e));
    }
    let _ = app.emit(RESULT_EVENT, &row.key);
}

/// The Pine bot's own position on `symbol`.
pub fn own_position(book: &[OpenPosition], symbol: &str) -> Option<OpenPosition> {
    book.iter().find(|p| p.bot_kind == BotKind::Pine && p.symbol.eq_ignore_ascii_case(symbol)).cloned()
}

#[allow(clippy::too_many_arguments)]
async fn run_plan(
    app: &AppHandle,
    cfg: &BotConfig,
    p: &PineSettings,
    row: &mut PineResult,
    v: &RunVerdict,
    fired: &Fired,
    leg: &mut Leg,
) -> Result<String, String> {
    if rules::stale(now_ms(), row.bar_close_ms, p.stale_s) {
        return Err(STALE.to_string());
    }
    let own = own_position(&app.state::<BotManager>().positions_snapshot(), &row.symbol);
    let plan = rules::plan(fired, own.as_ref().map(|o| o.direction));
    row.side = plan.open.map(|d| side(d).to_string());
    if plan.close {
        let pos = own.clone().ok_or_else(|| rules::NOTHING_TO_CLOSE.to_string())?;
        row.position_id = Some(pos.signal_id.clone());
        if let Err(code) = crate::bot::close_position_as(app.clone(), pos.signal_id.clone(), BotKind::Pine, TRIGGER_EXIT_REASON).await {
            return Err(if plan.open.is_some() { format!("pineFlipCloseFailed|{code}") } else { code });
        }
    }
    let Some(direction) = plan.open else {
        return match (plan.close, plan.note) {
            (true, note) => {
                row.note = note.map(str::to_string);
                Ok(crate::tvlink::message::CLOSED.to_string())
            }
            (false, Some(note)) => Err(note.to_string()),
            (false, None) => Err(rules::NOTHING_TO_CLOSE.to_string()),
        };
    };
    let flipped = own.as_ref().is_some_and(|o| o.direction != direction);
    match open_leg(app, cfg, p, row, v, direction, leg).await {
        Ok(()) if flipped => Ok(crate::tvlink::message::FLIPPED.to_string()),
        Ok(()) => Ok(crate::tvlink::message::OPENED.to_string()),
        Err(code) if plan.close && !code.starts_with(crate::tvlink::tasks::PENDING) => Err(format!("pineFlipOpenRefused|{code}")),
        Err(code) => Err(code),
    }
}

/// The bot's limits, one exchange position per symbol on Live, the price
/// guard, the symbol minimum and the exits, then the shared entry.
async fn open_leg(
    app: &AppHandle,
    cfg: &BotConfig,
    p: &PineSettings,
    row: &mut PineResult,
    v: &RunVerdict,
    direction: Direction,
    leg: &mut Leg,
) -> Result<(), String> {
    let bots = app.state::<BotManager>();
    let symbol = row.symbol.clone();
    let live_mode = cfg.mode == TradeMode::Live;
    if live_mode {
        if let Some(code) = p.live_refusal() {
            return Err(code.to_string()); // D4, again at the order
        }
    }
    let size = p.max_size_usdt;
    let leverage = precheck::effective_leverage(cfg, &app.state::<RiskManager>().limits());
    let book = bots.positions_snapshot();
    let mine: Vec<&OpenPosition> = book.iter().filter(|x| x.bot_kind == BotKind::Pine).collect();
    let dir = app.path().app_data_dir().map_err(|_| "appDataDirUnavailable".to_string())?;
    let day = crate::tvlink::guard::utc_day(now_ms());
    let opens_today = app.state::<StoreManager>().pine_opens_since(&dir, day)?;
    let usage = rules::Usage { open: mine.len(), margin_usdt: mine.iter().map(|x| x.capital).sum(), opens_today };
    rules::limit_refusal(cfg, p, size, leverage, usage)?;
    if live_mode {
        if let Some(k) = tv_trade::other_holder(&book, &symbol).filter(|k| *k != BotKind::Pine) {
            return Err(format!("pineSymbolHeldBy|{}", k.as_str()));
        }
        if crate::bot::strategy_live::owns_symbol(app, &symbol) {
            return Err("pineSymbolHeldBy|strategy".to_string());
        }
    }
    let exchange = app.state::<ExchangeManager>();
    let live = live_price(&exchange, &cfg.exchange_id, &symbol, BotKind::Pine)
        .await
        .filter(|x| x.is_finite() && *x > 0.0)
        .ok_or_else(|| "priceUnavailable".to_string())?;
    rules::price_refusal(row.bar_close, live, p.price_guard_pct)?;
    if cfg.exchange_id == "binance" {
        let rules_ = crate::bot::strategy::commands::binance_rules_cached(app, &symbol).await;
        let min = rules_.and_then(|r| tv_trade::symbol_min_usdt(r.lot.min_qty, r.lot.step, r.min_notional, live));
        tv_trade::symbol_min_refusal(&symbol, size, p.max_size_usdt, min).map_err(|e| e.replacen(tv_trade::BELOW_SYMBOL_MIN, "pineBelowSymbolMinimum", 1))?;
    }
    let (stop, target) = rules::exits(p, &v.numeric_inputs)?;
    (row.stop_pct, row.target_pct) = (Some(stop), Some(target));
    let levels = (Level::Pct(target), Level::Pct(stop));
    let (take_profit, stop_loss) = user_trade::resolve_levels(direction, live, levels.0, levels.1);
    let req = TradeRequest {
        kind: BotKind::Pine,
        symbol: symbol.clone(),
        direction,
        notional_usdt: Some(size),
        take_profit,
        stop_loss,
        mode: cfg.mode,
        confirmation: None,
        confirmed_margin_usdt: None,
    };
    match user_entry::open(app, BotKind::Pine, &req, &row.key, levels).await {
        Ok(open) => {
            leg.counted = true;
            fill_row(row, &open.pos, open.requested_qty);
            Ok(())
        }
        Err(r) if r.outcome_unknown => {
            // Sent, no verdict: never sent again, looked up by client id.
            leg.counted = true;
            if let Some(planned) = r.planned {
                (row.requested_qty, row.position_id) = (Some(planned.qty), Some(planned.signal_id.clone()));
                let pend = PinePending { planned, tp: levels.0, sl: levels.1, sent_at: now_ms() };
                leg.pending = serde_json::to_string(&pend).ok();
            }
            Err(format!("{}|{}", crate::tvlink::tasks::PENDING, r.code))
        }
        Err(r) => Err(r.code),
    }
}

pub(super) fn fill_row(row: &mut PineResult, pos: &OpenPosition, requested_qty: f64) {
    row.position_id = Some(pos.signal_id.clone());
    (row.requested_qty, row.filled_qty, row.fill_price) = (Some(requested_qty), Some(pos.qty), Some(pos.entry));
    row.slippage_pct = rules::slippage_pct(pos.direction, pos.entry, row.bar_close);
}
