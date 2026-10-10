//! "Take this signal now" (owner, 10 Oct): the user opens one buffered
//! signal on a signal bot by hand, from Execute on the Signals page.
//!
//! Paper only. The entry runs the loop's own pipeline (`entry::prepare_entry`
//! in `EntryMode::Manual`): the bot's signal filters are skipped (the user
//! decided), every risk check stays. The fill is the live price whenever it
//! lies strictly between the stop and the bot's target, also after the
//! signal's entry has passed (owner: Execute comes long after publication). A configured bot takes it whether it
//! runs or not; the engine loop is started so the position is managed.
//! Desktop only: the paper runner never compiles this file.

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::signal::model::Signal;
use crate::signal::SignalManager;

use super::super::model::{BotConfig, BotKind, OpenPosition, FUTURES_FEE_RATE, SPOT_FEE_RATE};
use super::super::BotManager;
use super::entry::{self, EntryMode, Prepared};
use super::{book, entry_rules, now_ms, pnl, take_profit};

/// Refusal: the bot trades real money. Manual entries are paper only.
pub const PAPER_ONLY: &str = "manualTakePaperOnly";
/// Refusal: the signal is not in the buffer (never received, or aged out).
pub const SIGNAL_NOT_FOUND: &str = "signalNotFound";
/// Below this reward:risk at the fill the preview warns: the trade has
/// moved and most of the published reward is gone.
pub const LOW_RR_AT_FILL: f64 = 0.5;
/// Feed note on a manual open.
pub const FEED_KEY: &str = "manualOpened";

/// What opening the signal now would do, from the paper fill model.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TakePreview {
    pub bot_kind: BotKind,
    pub signal_id: String,
    pub symbol: String,
    pub direction: String,
    /// Paper fill: the live price (a manual entry fills at market inside
    /// the stop–target band, also after the signal's entry has passed).
    pub entry: f64,
    /// The signal's published entry.
    pub published_entry: f64,
    pub stop: f64,
    /// |target − fill| / |fill − stop|.
    pub rr_at_fill: f64,
    /// The same exit target measured from the published entry.
    pub rr_published: f64,
    /// `rr_at_fill` below `LOW_RR_AT_FILL`.
    pub rr_low: bool,
    /// The bot's take-profit target resolved at this fill.
    pub target: f64,
    pub tp_target: String,
    pub notional_usdt: f64,
    pub capital: f64,
    pub effective_leverage: f64,
    pub risk_capped: bool,
    /// NET, fees included; negative.
    pub loss_at_stop_usdt: f64,
    /// NET, fees included.
    pub gain_at_target_usdt: f64,
    /// Both taker fees (open and close) on the notional.
    pub fees_usdt: f64,
}

/// Checks that need no network, in order: a saved config, paper money,
/// the daily stop, a valid config, the signal in the buffer, and no
/// position of this bot on the signal or its symbol.
pub fn refusal(
    cfg: Option<&BotConfig>,
    tripped: bool,
    sig: Option<&Signal>,
    holds_signal_or_symbol: bool,
) -> Result<(), String> {
    let Some(cfg) = cfg else {
        return Err("botNotConfigured".to_string());
    };
    if cfg.live {
        return Err(PAPER_ONLY.to_string());
    }
    if tripped {
        return Err("botDailyLossTripped".to_string());
    }
    cfg.validate()?;
    if sig.is_none() {
        return Err(SIGNAL_NOT_FOUND.to_string());
    }
    if holds_signal_or_symbol {
        return Err("marketHeld".to_string());
    }
    Ok(())
}

/// A refusal as the command returns it: `key` or `key|detail`.
pub fn skip_code(skip: &super::precheck::Skip) -> String {
    match &skip.detail {
        Some(d) => format!("{}|{d}", skip.key),
        None => skip.key.to_string(),
    }
}

/// Loss at the stop, gain at the target and both fees for `pos`, in USDT,
/// on the book's own NET PnL formula.
pub fn outcome_usdt(pos: &OpenPosition) -> (f64, f64, f64) {
    let at = |price: f64| pos.capital * pnl::unrealized_net_pnl_pct(pos, price) / 100.0;
    let fee_rate = if pos.bot_kind.uses_futures_market() { FUTURES_FEE_RATE } else { SPOT_FEE_RATE };
    let notional = pos.capital * super::sizing::leverage_of(pos);
    (at(pos.sl), at(pos.tp), 2.0 * fee_rate * notional)
}

/// Reward:risk of `entry` against `target` and `stop` (0 when the risk is 0).
pub fn reward_risk(entry: f64, target: f64, stop: f64) -> f64 {
    let risk = (entry - stop).abs();
    if risk > 0.0 && risk.is_finite() {
        (target - entry).abs() / risk
    } else {
        0.0
    }
}

/// The preview of `pos`; `published_target` is the bot's target resolved at
/// the signal's own entry (it differs from `pos.tp` for a custom %).
pub fn preview_of(pos: &OpenPosition, published_target: f64) -> TakePreview {
    let (loss, gain, fees) = outcome_usdt(pos);
    let rr_at_fill = reward_risk(pos.entry, pos.tp, pos.sl);
    TakePreview {
        bot_kind: pos.bot_kind,
        signal_id: pos.signal_id.clone(),
        symbol: pos.symbol.clone(),
        direction: pos.direction.clone(),
        entry: pos.entry,
        published_entry: pos.signal_entry,
        stop: pos.sl,
        rr_at_fill,
        rr_published: reward_risk(pos.signal_entry, published_target, pos.sl),
        rr_low: rr_at_fill < LOW_RR_AT_FILL,
        target: pos.tp,
        tp_target: pos.tp_target.clone(),
        notional_usdt: pos.notional_usdt,
        capital: pos.capital,
        effective_leverage: super::sizing::leverage_of(pos),
        risk_capped: pos.risk_capped,
        loss_at_stop_usdt: loss,
        gain_at_target_usdt: gain,
        fees_usdt: fees,
    }
}

/// The paper position a manual entry would open now with the bot's target
/// resolved at the published entry, or the refusal code.
async fn plan(app: &AppHandle, kind: BotKind, signal_id: &str) -> Result<(OpenPosition, f64), String> {
    let bots = app.state::<BotManager>();
    let cfg = bots.config_for(kind);
    let sig = app
        .state::<SignalManager>()
        .recent()
        .into_iter()
        .find(|s| s.id == signal_id);
    let held = sig
        .as_ref()
        .is_some_and(|s| bots.has_position(&s.id, kind) || bots.holds_symbol(kind, &s.symbol));
    refusal(cfg.as_ref(), bots.kill_switch_tripped(), sig.as_ref(), held)?;
    let (Some(cfg), Some(sig)) = (cfg, sig) else {
        return Err(SIGNAL_NOT_FOUND.to_string());
    };
    let now = now_ms();
    let Prepared { plan, ctx } = entry::prepare_entry(app, &cfg, &sig, bots.btc_regime(), EntryMode::Manual, held, now)
        .await
        .map_err(|s| skip_code(&s))?
        // A manual entry never waits (`entry_rules::plan_entry_in`).
        .ok_or_else(|| "priceUnavailable".to_string())?;
    let mut pos = entry_rules::finish_position(&cfg, &sig, plan.price, &plan.tp, &plan, now, &ctx);
    pos.manual = true;
    let published = take_profit::resolve_tp(&sig, plan.target, sig.entry).map_or(pos.tp, |r| r.tp);
    Ok((pos, published))
}

pub async fn preview(app: &AppHandle, kind: BotKind, signal_id: &str) -> Result<TakePreview, String> {
    plan(app, kind, signal_id).await.map(|(p, published)| preview_of(&p, published))
}

/// Opens the signal on `kind`'s paper book now. The book is checked again
/// at the moment of the open: a double click, or the bot's loop taking the
/// same signal meanwhile, opens nothing twice.
pub async fn take(app: &AppHandle, kind: BotKind, signal_id: &str) -> Result<OpenPosition, String> {
    let (pos, _) = plan(app, kind, signal_id).await?;
    let bots = app.state::<BotManager>();
    if !book::open_position_if_free(app, pos.clone()) {
        return Err("marketHeld".to_string());
    }
    bots.judge(kind, &pos.signal_id);
    bots.push_skip(&pos.symbol, FEED_KEY, None);
    bots.ensure_loop(app.clone());
    Ok(pos)
}
