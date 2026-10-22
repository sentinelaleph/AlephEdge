//! Tauri command surface for the bot desk.

use tauri::{AppHandle, Manager, State};

use super::live_terms::{self, LiveTerms};
use super::model::{BotConfig, BotDeskStatus, BotKind, OpenPosition, TradeMode, LIVE_TRADING_ENABLED, MAX_BOT_POSITIONS};
use super::BotManager;
use crate::risk::RiskManager;
use crate::signal::model::StreamNoteKind;
use crate::signal::SignalManager;


/// Refuses a config whose capital per position is above the risk level's
/// per-position cap (`balance × max_capital_pct`). The engine skips every
/// signal of such a bot with `capitalCap`, so it must not start at all: it
/// could never open a position.
pub fn check_capital_cap(cfg: &BotConfig, max_capital_quote: f64) -> Result<(), String> {
    if max_capital_quote > 0.0 && cfg.capital > max_capital_quote {
        return Err(format!("botCapitalAboveCap|{}", cap_text(max_capital_quote)));
    }
    Ok(())
}

/// The cap as the error shows it: whole USDT, cents below 1 USDT.
fn cap_text(cap: f64) -> String {
    if cap >= 1.0 {
        format!("{}", cap.floor())
    } else {
        format!("{}", (cap * 100.0).floor() / 100.0)
    }
}

// A new bot's capital (the default, lowered to the per-position cap); defined
// with the bot model so the headless bot farm makes the same new bot.
#[cfg(test)]
pub use super::model::default_capital;

/// Rejects an out-of-bounds risk % (Err reaches the UI as a rejected invoke)
/// rather than clamping what the user typed.
#[tauri::command]
pub fn bot_configure(
    app: AppHandle,
    bots: State<BotManager>,
    config: BotConfig,
) -> Result<BotDeskStatus, String> {
    apply_config(&app, &bots, config, KeepLive::Yes)?;
    Ok(bots.status())
}

/// Whether a saved config keeps the bot's real-money switch.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeepLive {
    /// A settings save: live stays as it was (unless its terms changed).
    Yes,
    /// A settings import: the bot is simulated afterwards, whatever it was.
    No,
}

/// The checks and the save of `bot_configure`, shared with the settings
/// import (app/settings_transfer.rs).
pub(crate) fn apply_config(app: &AppHandle, bots: &BotManager, config: BotConfig, keep: KeepLive) -> Result<(), String> {
    // The TradingView bot: margin per position from its budget, and its
    // passphrase kept as an Argon2id hash only.
    let config = super::engine::tv_trade::normalize_config(
        bots.config_for(config.kind).as_ref(),
        config,
        &super::engine::tv_passphrase::hash,
    )?;
    config.validate()?;
    if config.max_positions > MAX_BOT_POSITIONS {
        return Err(format!("botMaxPositionsRange|1–{MAX_BOT_POSITIONS}"));
    }
    check_capital_cap(&config, app.state::<RiskManager>().state().max_capital_quote)?;
    let exchange = app.state::<crate::exchange::ExchangeManager>();
    if !exchange.is_known(&config.exchange_id) {
        return Err(format!("exchangeUnknown|{}", config.exchange_id));
    }
    if config.kind.uses_futures_market() && !exchange.supports_futures(&config.exchange_id) {
        return Err(format!("exchangeNoFutures|{}", config.exchange_id));
    }
    // The mode and the arm are set only through `bot_set_live` (typed
    // confirmation). A settings save keeps whatever the bot already has, so a
    // stale form can never switch real money on or off by accident.
    let mut config = config;
    let prev = bots.config_for(config.kind);
    let positions = bots.positions_snapshot();
    // The exchange is where the typed LIVE sent real money, and where its
    // real positions are reconciled and closed: it cannot move under them.
    if let Some(refusal) = exchange_change_refusal(prev.as_ref(), &config, &positions) {
        return Err(refusal);
    }
    let holds_real = positions.iter().any(|p| p.bot_kind == config.kind && p.live);
    let (mode, armed) = saved_mode(prev.as_ref(), &config, keep, holds_real);
    (config.mode, config.live) = (mode, armed);
    let disarmed = prev.as_ref().is_some_and(|p| p.live) && !armed;
    if keep == KeepLive::No {
        bots.set_pilot(config.kind, 0);
    }
    let changed = prev.as_ref().is_some_and(|p| settings_changed(p, &config));
    let kind = config.kind;
    bots.store_config(app, config);
    // The typed LIVE confirmed the sizing it showed: with new terms a Live
    // bot stops until LIVE is typed again for them (it never runs on paper).
    if disarmed {
        bots.disarm_and_stop(app, kind);
        bots.push_skip("—", "liveDisarmedStopped", Some(kind.as_str().to_string()));
    }
    // Saved settings apply to signals already refused under the old ones,
    // not only to the next signal.
    if changed {
        bots.clear_settings_verdicts(kind);
    }
    Ok(())
}

/// The (mode, arm) a saved config keeps (pure). A settings save keeps the
/// mode; it keeps the arm unless the typed LIVE's terms changed. A settings
/// import (`KeepLive::No`) makes the bot Paper and disarmed, unless it still
/// holds real positions: then it stays a Live bot, disarmed, until they close.
pub fn saved_mode(prev: Option<&BotConfig>, next: &BotConfig, keep: KeepLive, holds_real: bool) -> (TradeMode, bool) {
    let Some(prev) = prev else {
        return (if holds_real { TradeMode::Live } else { TradeMode::Paper }, false);
    };
    match keep {
        KeepLive::Yes => (prev.mode, prev.live && !live_terms_changed(prev, next)),
        KeepLive::No if holds_real => (TradeMode::Live, false),
        KeepLive::No => (TradeMode::Paper, false),
    }
}

/// Any setting other than the real-money mode and arm differs. Comparing the
/// whole config errs toward re-judging: a re-judged signal reaches the same
/// verdict again and the skip log keeps one row per verdict.
pub fn settings_changed(prev: &BotConfig, next: &BotConfig) -> bool {
    let plain = |c: &BotConfig| {
        let mut c = c.clone();
        c.live = false;
        c.mode = TradeMode::Paper;
        serde_json::to_value(c).ok()
    };
    plain(prev) != plain(next)
}

/// What the typed LIVE confirmed: capital, leverage, position cap, the
/// exchange the orders go to and, for the TradingView bot, its budget and
/// max size per alert (its margin at risk and largest position,
/// `live_terms::signal_terms`).
pub fn live_terms_changed(prev: &BotConfig, next: &BotConfig) -> bool {
    let tv_money = |c: &BotConfig| c.tv.as_ref().map(|t| (t.budget_usdt, t.max_size_usdt));
    prev.capital != next.capital
        || prev.leverage != next.leverage
        || prev.max_positions != next.max_positions
        || prev.exchange_id != next.exchange_id
        || tv_money(prev) != tv_money(next)
}

/// Why a settings save may not change the bot's exchange, or None. A bot
/// holding real positions keeps its exchange until they are closed (they
/// would otherwise be reconciled against another venue's account and booked
/// as closed while still open); a bot switched to real money goes back to
/// simulated first (LIVE was confirmed for the venue it named).
pub fn exchange_change_refusal(prev: Option<&BotConfig>, next: &BotConfig, positions: &[OpenPosition]) -> Option<String> {
    let prev = prev?;
    if prev.exchange_id == next.exchange_id {
        return None;
    }
    if let Some(p) = positions.iter().find(|p| p.bot_kind == next.kind && p.live) {
        return Some(format!("botHasLivePositions|{}", p.exchange_id));
    }
    if prev.live || prev.mode == TradeMode::Live {
        return Some(format!("liveExchangeLocked|{}", prev.exchange_id));
    }
    None
}

/// Why real money may not be switched ON for `kind` on `exchange_id`, or
/// None. Pure (the order path and the sandbox are passed in); the key check
/// needs the vault and stays in the command. `LIVE_TRADING_ENABLED` is true
/// in the desktop's one build; the parameter keeps the rule testable.
pub fn live_enable_refusal(
    live_build: bool,
    kind: BotKind,
    exchange_id: &str,
    confirmation: &str,
    sandbox: bool,
) -> Option<String> {
    if !live_build {
        return Some("liveBuildDisabled".to_string());
    }
    if !kind.live_capable() || !crate::exchange::has_order_path(exchange_id) {
        return Some("liveFuturesBinanceOnly".to_string());
    }
    // Only a venue whose order path passed its test-network dry run.
    if let Err(code) = crate::exchange::live_venue_check_in(exchange_id, sandbox) {
        return Some(code);
    }
    if confirmation.trim() != LIVE_CONFIRMATION {
        return Some(format!("liveConfirmRequired|{LIVE_CONFIRMATION}"));
    }
    None
}

/// The exact text the user types to switch a bot to real money.
pub const LIVE_CONFIRMATION: &str = "LIVE";

/// Why a bot whose book holds paper positions may not take real money, or
/// None (pure): paper and real positions never share a bot.
pub fn paper_book_refusal(kind: BotKind, positions: &[OpenPosition]) -> Option<&'static str> {
    positions.iter().any(|p| p.bot_kind == kind && !p.live).then_some("liveHasPaperPositions")
}

/// What switching real money OFF leaves the bot as (pure): it goes back to
/// Paper only once it holds no real position, else it stays a Live bot,
/// disarmed and stopped, until they close (the book keeps managing them).
pub fn off_mode(holds_real: bool) -> TradeMode {
    if holds_real {
        TradeMode::Live
    } else {
        TradeMode::Paper
    }
}

/// The money the typed LIVE would put at risk on `kind`'s saved settings.
#[tauri::command]
pub fn bot_live_terms(bots: State<BotManager>, risk: State<RiskManager>, kind: BotKind) -> Result<LiveTerms, String> {
    let cfg = bots.config_for(kind).ok_or_else(|| "botSaveFirst".to_string())?;
    Ok(live_terms::signal_terms(&cfg, &risk.limits()))
}

/// Real money for one signal bot: the "Go live" of a Paper bot and the
/// per-session arm of a Live bot share this one gate. On needs: the Futures
/// bot on a venue whose order path passed its dry run, a verified trade-only
/// key in the unlocked vault, readable risk settings and store, no paper
/// position in the bot's book, the typed confirmation and the margin at risk
/// it showed (`at_risk_usdt`, re-checked against the settings in force). On
/// makes the bot Live (stored) and armed with its pilot (a new one when it
/// was Paper, the saved count on a re-arm: `pilot_at_arm`). The arm is never
/// persisted: a restart brings the Live bot back stopped (see
/// `BotManager::restore_configs`). Off always works: the bot is disarmed and
/// stopped, and becomes Paper once it holds no real position.
#[tauri::command]
pub fn bot_set_live(
    app: AppHandle,
    bots: State<BotManager>,
    kind: BotKind,
    enabled: bool,
    confirmation: String,
    at_risk_usdt: Option<f64>,
) -> Result<BotDeskStatus, String> {
    let mut cfg = bots
        .config_for(kind)
        .ok_or_else(|| "botSaveFirst".to_string())?;
    let positions = bots.positions_snapshot();
    if !enabled {
        let holds_real = positions.iter().any(|p| p.bot_kind == kind && p.live);
        bots.disarm_and_stop(&app, kind);
        bots.set_pilot(kind, 0);
        cfg.live = false;
        cfg.mode = off_mode(holds_real);
        bots.store_config(&app, cfg);
        return Ok(bots.status());
    }
    let sandbox = crate::exchange::venue::ccxt::sandbox_from_env();
    if let Some(refusal) = live_enable_refusal(LIVE_TRADING_ENABLED, kind, &cfg.exchange_id, &confirmation, sandbox) {
        return Err(refusal);
    }
    if let Some(code) = crate::app::data_health::trading_blocked(&app) {
        return Err(code);
    }
    if let Some(code) = paper_book_refusal(kind, &positions) {
        return Err(code.to_string());
    }
    // A verified trade-only key: a key stored before verification was
    // mandatory (Unknown) or one that can withdraw never goes live.
    match app
        .state::<crate::vault::VaultManager>()
        .credential(&cfg.exchange_id)
        .map(|c| c.permission)
    {
        Some(crate::vault::model::CredentialPermission::TradeOnly) => {}
        Some(_) => return Err("liveNeedsVerifiedKey".to_string()),
        None => return Err("liveNeedsTradeKey".to_string()),
    }
    let terms = live_terms::signal_terms(&cfg, &app.state::<RiskManager>().limits());
    live_terms::confirmed(&terms, at_risk_usdt)?;
    // A Paper bot going Live starts a new pilot; a Live bot's re-arm (after
    // a restart, "Stop all live") continues the saved one.
    let fresh = cfg.mode == TradeMode::Paper;
    cfg.live = true;
    cfg.mode = TradeMode::Live;
    bots.set_pilot(kind, super::pilot_at_arm(bots.saved_pilot(&app, kind), fresh));
    bots.save_pilot_left(&app, kind);
    bots.set_armed_terms(kind, terms);
    bots.store_config(&app, cfg);
    Ok(bots.status())
}

/// The real positions of `kind` that "Close positions and switch" closes
/// (pure): that bot's own live rows only, never another bot's, never a
/// paper row.
pub fn own_live_rows(kind: BotKind, positions: &[OpenPosition]) -> Vec<String> {
    positions.iter().filter(|p| p.bot_kind == kind && p.live).map(|p| p.signal_id.clone()).collect()
}

/// The verdict after the closes ran (pure): Paper only once none of the
/// bot's real positions is left; otherwise it stays a disarmed Live bot and
/// says how many are still open (`closeNotConfirmed|n`).
pub fn close_and_paper_verdict(left: usize) -> Result<TradeMode, String> {
    if left == 0 {
        Ok(TradeMode::Paper)
    } else {
        Err(format!("closeNotConfirmed|{left}"))
    }
}

/// "Close positions and switch": the Live bot is disarmed and stopped first
/// (no new entry while it closes), each of ITS real positions is closed
/// through the manual close path (`book::close_now`: the close claim, a
/// reduce-only market order sized from the exchange's own position, the
/// book updated only on a confirmed fill), and only once none is left the
/// bot becomes Paper. A close the exchange did not confirm leaves it a
/// disarmed Live bot (what "Switch later" does) and returns
/// `closeNotConfirmed|n`. Other bots' positions are never touched.
#[tauri::command]
pub async fn bot_close_live_and_paper(app: AppHandle, kind: BotKind) -> Result<BotDeskStatus, String> {
    let bots = app.state::<BotManager>();
    let mut cfg = bots.config_for(kind).ok_or_else(|| "botSaveFirst".to_string())?;
    bots.disarm_and_stop(&app, kind);
    bots.set_pilot(kind, 0);
    for id in own_live_rows(kind, &bots.positions_snapshot()) {
        super::engine::book::close_now(&app, &id, kind, MANUAL_EXIT_REASON).await;
    }
    let left = own_live_rows(kind, &bots.positions_snapshot()).len();
    let verdict = close_and_paper_verdict(left);
    cfg.live = false;
    cfg.mode = verdict.clone().unwrap_or(TradeMode::Live);
    bots.store_config(&app, cfg);
    verdict.map(|_| bots.status())
}

/// What "Stop all live" did.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStopReport {
    /// Live signal bots that were running and stopped (every one disarmed).
    pub signal_stopped: u32,
    /// Live DCA / Grid bots that were running and stopped (every one disarmed).
    pub strategy_stopped: u32,
}

/// The header's "Stop all live": every Live bot stops taking new entries or
/// cycles and is disarmed (a new start needs the typed LIVE). Open real
/// positions and cycles stay managed under their exchange stops; closing
/// them is the Exchange tab's Close all.
#[tauri::command]
pub async fn live_stop_all(app: AppHandle) -> Result<LiveStopReport, String> {
    let bots = app.state::<BotManager>();
    let mut signal_stopped = 0;
    for kind in super::ALL_KINDS {
        if bots.config_for(kind).is_some_and(|c| c.live || c.mode == TradeMode::Live) {
            if bots.disarm_and_stop(&app, kind) {
                signal_stopped += 1;
            }
            bots.set_pilot(kind, 0);
        }
    }
    let strategy_stopped = super::strategy_live::stop_all_live(&app).await;
    Ok(LiveStopReport { signal_stopped, strategy_stopped })
}

/// Ends the pilot of a live bot: entries go back to full size.
#[tauri::command]
pub fn bot_end_pilot(app: AppHandle, bots: State<BotManager>, kind: BotKind) -> BotDeskStatus {
    bots.set_pilot(kind, 0);
    bots.save_pilot_left(&app, kind);
    bots.status()
}

/// Defaults for a NEW bot (risk sizing), so the UI never re-declares them.
/// Saved configs keep whatever they carry. The capital is fitted under the
/// current per-position cap (`default_capital`) and the risk % raised so a
/// typical stop still clears the exchange minimum (`default_risk_pct_for`).
#[tauri::command]
pub fn bot_default_config(risk: State<RiskManager>, kind: BotKind, exchange_id: String) -> BotConfig {
    BotConfig::new_tradable_for_desk(kind, &exchange_id, risk.state().max_capital_quote)
}

#[tauri::command]
pub fn bot_start(
    app: AppHandle,
    bots: State<BotManager>,
    kind: BotKind,
) -> Result<BotDeskStatus, String> {
    bots.start(app.clone(), kind)?;
    Ok(bots.status())
}

#[tauri::command]
pub fn bot_stop(app: AppHandle, bots: State<BotManager>, kind: BotKind) -> BotDeskStatus {
    bots.stop(&app, kind);
    bots.status()
}

/// Polled by the desk. Before snapshotting, drains any fresh Sentinel vetoes
/// into the visible skip feed — so a server-side invalidation shows up as a
/// "skipped trade" (no silent skips) regardless of whether a bot loop
/// is currently ticking.
/// The exit reason a user-initiated close is recorded with. The same string
/// `strategy_close` uses, so `pnl.exit.manual` / `strategy.exit.manual` label it.
pub const MANUAL_EXIT_REASON: &str = "manual";

/// The verdict of a manual close from the book's state before and after the
/// shared close path ran: a position that was never there is
/// `positionNotFound`; one still in the book afterwards (no live price, a
/// close claim held elsewhere, a live close the exchange did not confirm) is
/// `closeNotConfirmed`. Success only once it is gone.
fn manual_close_verdict(found_before: bool, still_open_after: bool) -> Result<(), String> {
    if !found_before {
        return Err("positionNotFound".to_string());
    }
    if still_open_after {
        return Err("closeNotConfirmed".to_string());
    }
    Ok(())
}

/// Closes one open signal-bot position now, at the live quote, exit reason
/// "manual". Runs the engine's own close path (`book::close_now`): the close
/// claim, paper booking at the live price, and for a LIVE position a real
/// reduce-only market order sized from the exchange's own position. The UI
/// asks for a typed confirmation before a live close.
#[tauri::command]
pub async fn bot_close_position(
    app: AppHandle,
    signal_id: String,
    kind: BotKind,
) -> Result<BotDeskStatus, String> {
    let is_open = |app: &AppHandle| {
        app.state::<BotManager>()
            .positions_snapshot()
            .iter()
            .any(|p| p.signal_id == signal_id && p.bot_kind == kind)
    };
    let found = is_open(&app);
    if found {
        super::engine::book::close_now(&app, &signal_id, kind, MANUAL_EXIT_REASON).await;
    }
    manual_close_verdict(found, is_open(&app))?;
    Ok(app.state::<BotManager>().status())
}

/// Opens one buffered signal on `kind`'s paper book now, by hand (Execute
/// on the Signals page). The bot's own signal filters are skipped, its risk
/// limits are not (see engine/manual.rs). Refused for a bot on real money
/// (`manualTakePaperOnly`); every other refusal is a skip key, `key|detail`.
#[tauri::command]
pub async fn bot_take_signal(app: AppHandle, kind: BotKind, signal_id: String) -> Result<OpenPosition, String> {
    super::engine::manual::take(&app, kind, &signal_id).await
}

/// What `bot_take_signal` would open now: size, loss at the stop and gain at
/// the target with both fees (paper fill model), or the same refusal.
#[tauri::command]
pub async fn bot_preview_take(
    app: AppHandle,
    kind: BotKind,
    signal_id: String,
) -> Result<super::engine::manual::TakePreview, String> {
    super::engine::manual::preview(&app, kind, &signal_id).await
}

/// "New trade": opens ONE paper position on the user's own symbol, side,
/// size, take-profit and stop, at the live price now, through the same entry
/// pipeline as `bot_take_signal` (see engine/user_trade.rs). Paper only
/// (`manualTakePaperOnly`); every other refusal is a code, `key|detail`.
#[tauri::command]
pub async fn bot_open_trade(
    app: AppHandle,
    request: super::engine::user_trade::TradeRequest,
) -> Result<OpenPosition, String> {
    super::engine::user_trade::open(&app, &request).await
}

/// What `bot_open_trade` would open now: fill, quantity, risk in USDT and %
/// of the bot's capital, reward:risk, liquidation price, fees; or the same
/// refusal.
#[tauri::command]
pub async fn bot_preview_trade(
    app: AppHandle,
    request: super::engine::user_trade::TradeRequest,
) -> Result<super::engine::user_trade::TradePreview, String> {
    super::engine::user_trade::preview(&app, &request).await
}

#[tauri::command]
pub fn bot_status(app: AppHandle, bots: State<BotManager>, signals: State<SignalManager>) -> BotDeskStatus {
    for veto in signals.drain_pending_vetoes() {
        let key = match veto.kind {
            StreamNoteKind::Veto => "sentinelVeto",
            StreamNoteKind::Superseded => "superseded",
            StreamNoteKind::ParseError => "signalParseError",
            StreamNoteKind::RegimeVeto => "vetoedByRegime",
            StreamNoteKind::Resolved => "signalResolved",
        };
        let detail = (!veto.reason.is_empty()).then_some(veto.reason);
        bots.push_skip(&veto.symbol, key, detail);
    }
    let mut status = bots.status();
    status.trading_blocked = crate::app::data_health::trading_blocked(&app);
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sizing_or_exchange_change_needs_live_confirmed_again() {
        let a = BotConfig::new_default(BotKind::Futures, "binance");
        let mut b = a.clone();
        assert!(!live_terms_changed(&a, &b));
        b.min_confidence = Some(0.9);
        assert!(!live_terms_changed(&a, &b), "filters are not sizing");
        b.leverage = a.leverage + 1;
        assert!(live_terms_changed(&a, &b));
        let mut c = a.clone();
        c.capital = a.capital * 2.0;
        assert!(live_terms_changed(&a, &c));
        let mut d = a.clone();
        d.max_positions = a.max_positions + 1;
        assert!(live_terms_changed(&a, &d));
        let mut e = a.clone();
        e.exchange_id = "bybit".into();
        assert!(live_terms_changed(&a, &e), "LIVE named the exchange");
        // The TradingView bot: its budget and max size are the money LIVE showed.
        let tv = BotConfig::new_default(BotKind::Tradingview, "binance");
        let mut f = tv.clone();
        f.tv.as_mut().unwrap().max_size_usdt += 10.0;
        assert!(live_terms_changed(&tv, &f));
        let mut g = tv.clone();
        g.tv.as_mut().unwrap().budget_usdt += 10.0;
        assert!(live_terms_changed(&tv, &g));
        let mut h = tv.clone();
        h.tv.as_mut().unwrap().stale_s += 10;
        assert!(!live_terms_changed(&tv, &h), "a guard is not money");
    }

    fn live_position(kind: BotKind, exchange: &str) -> OpenPosition {
        let mut p = crate::bot::engine::test_fixtures::position(
            &crate::bot::engine::test_fixtures::signal("long", 100.0, 106.0, 93.0),
            None,
        );
        (p.bot_kind, p.exchange_id, p.live) = (kind, exchange.into(), true);
        p
    }

    #[test]
    fn the_exchange_cannot_move_under_real_money() {
        let paper = BotConfig::new_default(BotKind::Futures, "binance");
        let mut live = paper.clone();
        live.live = true;
        let mut to_bybit = paper.clone();
        to_bybit.exchange_id = "bybit".into();

        // A paper bot without real positions may move.
        assert_eq!(exchange_change_refusal(Some(&paper), &to_bybit, &[]), None);
        assert_eq!(exchange_change_refusal(None, &to_bybit, &[]), None, "a new bot");
        // Switched to real money: back to simulated first.
        assert_eq!(
            exchange_change_refusal(Some(&live), &to_bybit, &[]),
            Some("liveExchangeLocked|binance".to_string())
        );
        // Real positions held (live switched off since): close them first.
        let held = [live_position(BotKind::Futures, "binance")];
        assert_eq!(
            exchange_change_refusal(Some(&paper), &to_bybit, &held),
            Some("botHasLivePositions|binance".to_string())
        );
        // Another bot's real position, a paper position, or no exchange
        // change at all do not block.
        let other = [live_position(BotKind::Spot, "binance")];
        assert_eq!(exchange_change_refusal(Some(&paper), &to_bybit, &other), None);
        let mut paper_pos = live_position(BotKind::Futures, "binance");
        paper_pos.live = false;
        assert_eq!(exchange_change_refusal(Some(&paper), &to_bybit, &[paper_pos]), None);
        assert_eq!(exchange_change_refusal(Some(&live), &live, &held), None, "same exchange");
    }

    #[test]
    fn real_money_switches_on_only_on_a_dry_run_venue() {
        let on = |exchange: &str, sandbox: bool| live_enable_refusal(true, BotKind::Futures, exchange, "LIVE", sandbox);
        assert_eq!(on("binance", false), None);
        assert_eq!(on("bybit", false), Some("liveVenueNotDryRun|bybit".to_string()));
        assert_eq!(on("okx", false), Some("liveVenueNotDryRun|okx".to_string()));
        // Bitget's order path was removed (no key verifier, no dry run).
        assert_eq!(on("bitget", false), Some("liveFuturesBinanceOnly".to_string()));
        assert_eq!(on("bitget", true), Some("liveFuturesBinanceOnly".to_string()));
        // The sandbox dry run may switch them on (orders go to test networks).
        assert_eq!(on("bybit", true), None);
        assert_eq!(on("okx", true), None);
        // The rest of the chain is unchanged.
        assert_eq!(on("mexc", true), Some("liveFuturesBinanceOnly".to_string()));
        assert_eq!(
            live_enable_refusal(true, BotKind::Spot, "binance", "LIVE", false),
            Some("liveFuturesBinanceOnly".to_string())
        );
        assert_eq!(
            live_enable_refusal(true, BotKind::Futures, "binance", "live", false),
            Some("liveConfirmRequired|LIVE".to_string())
        );
        assert_eq!(
            live_enable_refusal(false, BotKind::Futures, "binance", "LIVE", false),
            Some("liveBuildDisabled".to_string())
        );
    }

    // A fresh install is Cautious with a 1000 USDT balance: 20 USDT per
    // position, so the default bot must not ask for more.
    #[test]
    fn the_default_bot_fits_the_default_risk_setting() {
        use crate::bot::engine::precheck::{judge_budget, Budget};
        use crate::risk::model::{RiskConfig, RiskState};
        let risk = RiskConfig::default();
        let cap = RiskState::from_config(&risk).max_capital_quote;
        assert_eq!(cap, 20.0);
        let mut cfg = BotConfig::new_default(BotKind::Futures, "binance");
        let budget = Budget { global_open: 0, bot_open: 0, balance: risk.balance, notional: 23.7 };
        let limits = risk.level.limits();
        assert_eq!(judge_budget(&limits, &cfg, &budget).unwrap_err().key, "capitalCap", "the old default");
        assert_eq!(check_capital_cap(&cfg, cap), Err("botCapitalAboveCap|20".to_string()));
        cfg.capital = default_capital(cap);
        assert_eq!(cfg.capital, 20.0);
        assert!(judge_budget(&limits, &cfg, &budget).is_ok());
        assert!(check_capital_cap(&cfg, cap).is_ok(), "at the cap is allowed");
    }

    // 0.3.17: the fresh-install new bot (20 USDT at Cautious / 1000) sized
    // every 7% stop at ~2.8 USDT, under the 5 USDT minimum, and only skipped.
    #[test]
    fn the_default_new_bot_can_place_an_order_on_a_typical_stop() {
        use crate::bot::engine::sizing::size_position;
        use crate::bot::model::{default_risk_pct_for, EXCHANGE_MIN_ORDER_USDT};
        use crate::risk::model::{RiskConfig, RiskState};
        let risk = RiskConfig::default();
        let cap = RiskState::from_config(&risk).max_capital_quote;
        for kind in [BotKind::Futures, BotKind::Spot] {
            let old = BotConfig::new_for_desk(kind, "binance", cap);
            let new = BotConfig::new_tradable_for_desk(kind, "binance", cap);
            assert_eq!((old.capital, new.capital), (20.0, 20.0), "capital stays at the cap");
            assert_eq!(old.risk_per_trade_pct, 1.0, "the farm's builder is unchanged");
            assert_eq!(new.risk_per_trade_pct, 2.6);
            assert!(new.validate().is_ok());
            assert!(check_capital_cap(&new, cap).is_ok());
            let lev = if kind == BotKind::Spot { 1 } else { 2 };
            // Entry 100: stops at 7% (median of the 17 Oct pending signals) and 10%.
            let at = |c: &BotConfig, sl: f64| size_position(c, lev, 100.0, sl).unwrap().notional;
            assert!(at(&old, 93.0) < EXCHANGE_MIN_ORDER_USDT, "1% at 20 USDT: {}", at(&old, 93.0));
            assert!(at(&new, 93.0) >= EXCHANGE_MIN_ORDER_USDT, "7% stop: {}", at(&new, 93.0));
            assert!(at(&new, 90.0) >= EXCHANGE_MIN_ORDER_USDT, "10% stop: {}", at(&new, 90.0));
            // At risk: 2.6% of 20 = 0.52 USDT = 0.052% of the 1000 USDT balance.
            assert!((new.capital * new.risk_per_trade_pct / 100.0 - 0.52).abs() < 1e-9);
        }
        assert_eq!(default_risk_pct_for(100.0), 1.0, "a 100 USDT bot keeps 1%");
        assert_eq!(default_risk_pct_for(10.0), 5.0, "capped at the 5% ceiling");
        assert_eq!(default_risk_pct_for(2.0), 5.0);
        assert_eq!(default_risk_pct_for(0.0), 1.0);
        assert_eq!(default_risk_pct_for(f64::NAN), 1.0);
    }

    #[test]
    fn default_capital_keeps_100_when_the_cap_allows_it() {
        assert_eq!(default_capital(200.0), 100.0);
        assert_eq!(default_capital(60.4), 60.0);
        assert_eq!(default_capital(0.456), 0.45);
        assert_eq!(default_capital(0.0), 100.0, "no cap known");
        assert_eq!(cap_text(0.456), "0.45");
        assert_eq!(cap_text(60.4), "60");
    }

    #[test]
    fn only_a_real_settings_change_rejudges_pending_signals() {
        let a = BotConfig::new_default(BotKind::Futures, "binance");
        let mut b = a.clone();
        b.live = true;
        assert!(!settings_changed(&a, &b), "the live flag is not a setting");
        b.direction = Some("all".into());
        assert!(settings_changed(&a, &b));
        let mut c = a.clone();
        c.max_signal_age_min = 0;
        assert!(settings_changed(&a, &c));
    }

    #[test]
    fn close_and_switch_closes_only_this_bots_real_positions() {
        let mut tv_live = live_position(BotKind::Tradingview, "binance");
        tv_live.signal_id = "manual-tv-a".into();
        let mut tv_paper = live_position(BotKind::Tradingview, "binance");
        (tv_paper.signal_id, tv_paper.live) = ("manual-tv-b".into(), false);
        let mut fut_live = live_position(BotKind::Futures, "binance");
        fut_live.signal_id = "sig-1".into();
        let book = vec![fut_live.clone(), tv_live, tv_paper, fut_live];
        assert_eq!(own_live_rows(BotKind::Tradingview, &book), vec!["manual-tv-a".to_string()]);
        assert_eq!(own_live_rows(BotKind::Futures, &book).len(), 2, "the Futures bot's own two");
        assert!(own_live_rows(BotKind::Spot, &book).is_empty());
        // Paper only once every one closed; else it stays Live, disarmed.
        assert_eq!(close_and_paper_verdict(0), Ok(TradeMode::Paper));
        assert_eq!(close_and_paper_verdict(1), Err("closeNotConfirmed|1".to_string()));
        // The command closes through the shared close path, one row at a time.
        let src = include_str!("commands.rs");
        let body = &src[src.find("pub async fn bot_close_live_and_paper").unwrap()..];
        let body = &body[..body.find("\n}").unwrap()];
        assert!(body.contains("disarm_and_stop(&app, kind)"));
        assert!(body.contains("own_live_rows(kind, &bots.positions_snapshot())"));
        assert!(body.contains("book::close_now(&app, &id, kind, MANUAL_EXIT_REASON)"));
        assert!(!body.contains("close_all"), "never a close-all across bots");
    }

    #[test]
    fn manual_reason_is_the_labelled_one() {
        assert_eq!(MANUAL_EXIT_REASON, "manual");
    }

    #[test]
    fn missing_position_is_not_found() {
        assert_eq!(manual_close_verdict(false, false), Err("positionNotFound".to_string()));
        // Not found wins even if a same-id row appeared meanwhile.
        assert_eq!(manual_close_verdict(false, true), Err("positionNotFound".to_string()));
    }

    #[test]
    fn still_open_after_the_close_is_not_confirmed() {
        assert_eq!(manual_close_verdict(true, true), Err("closeNotConfirmed".to_string()));
    }

    #[test]
    fn gone_after_the_close_is_success() {
        assert_eq!(manual_close_verdict(true, false), Ok(()));
    }
}
