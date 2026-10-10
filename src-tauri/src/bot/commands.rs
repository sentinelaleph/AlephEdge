//! Tauri command surface for the bot desk.

use tauri::{AppHandle, Manager, State};

use super::model::{BotConfig, BotDeskStatus, BotKind, OpenPosition, LIVE_TRADING_ENABLED, MAX_BOT_POSITIONS};
use super::BotManager;
use crate::risk::RiskManager;
use crate::signal::model::StreamNoteKind;
use crate::signal::SignalManager;

/// Capital a NEW bot starts with when the risk level allows it.
const DEFAULT_CAPITAL: f64 = 100.0;

/// Refuses a config whose capital per position is above the risk level's
/// per-position cap (`balance × max_capital_pct`). The engine skips every
/// signal of such a bot with `capitalCap`, so it must not start at all:
/// before this check a fresh install (Cautious, 1000 USDT, cap 20) started a
/// 100 USDT bot that could never open a position.
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

/// A new bot's capital: the default, lowered to the per-position cap of the
/// current risk level and balance so the defaults can actually trade.
pub fn default_capital(max_capital_quote: f64) -> f64 {
    if !(max_capital_quote.is_finite() && max_capital_quote > 0.0) {
        return DEFAULT_CAPITAL;
    }
    let cap = if max_capital_quote >= 1.0 {
        max_capital_quote.floor()
    } else {
        (max_capital_quote * 100.0).floor() / 100.0
    };
    DEFAULT_CAPITAL.min(cap)
}

/// Rejects an out-of-bounds risk % (Err reaches the UI as a rejected invoke)
/// rather than clamping what the user typed.
#[tauri::command]
pub fn bot_configure(
    app: AppHandle,
    bots: State<BotManager>,
    config: BotConfig,
) -> Result<BotDeskStatus, String> {
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
    // `live` is set only through `bot_set_live` (typed confirmation). A
    // settings save keeps whatever the bot already has, so a stale form can
    // never switch real money on or off by accident.
    let mut config = config;
    let prev = bots.config_for(config.kind);
    // The exchange is where the typed LIVE sent real money, and where its
    // real positions are reconciled and closed: it cannot move under them.
    if let Some(refusal) = exchange_change_refusal(prev.as_ref(), &config, &bots.positions_snapshot()) {
        return Err(refusal);
    }
    config.live = prev.as_ref().is_some_and(|c| c.live);
    // The typed LIVE confirmed real money at the sizing it showed. A change
    // to capital, leverage or the position cap switches live off; the user
    // confirms the new sizing with LIVE again.
    if prev.as_ref().is_some_and(|p| p.live && live_terms_changed(p, &config)) {
        config.live = false;
    }
    let changed = prev.as_ref().is_some_and(|p| settings_changed(p, &config));
    bots.save_config(&app, &config);
    let kind = config.kind;
    bots.configure(config);
    // Saved settings apply to signals already refused under the old ones,
    // not only to the next signal (until now only a restart did that).
    if changed {
        bots.clear_settings_verdicts(kind);
    }
    Ok(bots.status())
}

/// Any setting other than the real-money flag differs. Comparing the whole
/// config errs toward re-judging: a re-judged signal reaches the same
/// verdict again and the skip log keeps one row per verdict.
pub fn settings_changed(prev: &BotConfig, next: &BotConfig) -> bool {
    let plain = |c: &BotConfig| {
        let mut c = c.clone();
        c.live = false;
        serde_json::to_value(c).ok()
    };
    plain(prev) != plain(next)
}

/// What the typed LIVE confirmed: capital, leverage, position cap, and the
/// exchange the orders go to.
pub fn live_terms_changed(prev: &BotConfig, next: &BotConfig) -> bool {
    prev.capital != next.capital
        || prev.leverage != next.leverage
        || prev.max_positions != next.max_positions
        || prev.exchange_id != next.exchange_id
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
    if prev.live {
        return Some(format!("liveExchangeLocked|{}", prev.exchange_id));
    }
    None
}

/// Why real money may not be switched ON for `kind` on `exchange_id`, or
/// None. Pure (the build switch and the sandbox are passed in); the key
/// check needs the vault and stays in the command.
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
    if kind != BotKind::Futures || !crate::exchange::has_order_path(exchange_id) {
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

/// Switches one bot's real-money trading on or off. On needs: a live build,
/// the Futures bot on a venue whose order path passed its dry run (Binance),
/// a Trade key in the unlocked vault, and the typed confirmation. Off always
/// works. Never persisted as on: a restart comes back simulated (see
/// `BotManager::restore_configs`).
#[tauri::command]
pub fn bot_set_live(
    app: AppHandle,
    bots: State<BotManager>,
    kind: BotKind,
    enabled: bool,
    confirmation: String,
) -> Result<BotDeskStatus, String> {
    let mut cfg = bots
        .config_for(kind)
        .ok_or_else(|| "botSaveFirst".to_string())?;
    if enabled {
        let sandbox = crate::exchange::venue::ccxt::sandbox_from_env();
        if let Some(refusal) = live_enable_refusal(LIVE_TRADING_ENABLED, kind, &cfg.exchange_id, &confirmation, sandbox) {
            return Err(refusal);
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
    }
    cfg.live = enabled;
    bots.set_pilot(kind, if enabled { super::model::PILOT_TRADES } else { 0 });
    bots.configure(cfg);
    Ok(bots.status())
}

/// Ends the pilot of a live bot: entries go back to full size.
#[tauri::command]
pub fn bot_end_pilot(bots: State<BotManager>, kind: BotKind) -> BotDeskStatus {
    bots.set_pilot(kind, 0);
    bots.status()
}

/// Defaults for a NEW bot (risk sizing, 1% per trade), so the UI never
/// re-declares them. Saved configs keep whatever they carry. The capital is
/// fitted under the current per-position cap (see `default_capital`).
#[tauri::command]
pub fn bot_default_config(risk: State<RiskManager>, kind: BotKind, exchange_id: String) -> BotConfig {
    let mut cfg = BotConfig::new_default(kind, &exchange_id);
    cfg.capital = default_capital(risk.state().max_capital_quote);
    cfg
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
/// "skipped trade" (PRD §5.4: no silent skips) regardless of whether a bot loop
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

#[tauri::command]
pub fn bot_status(bots: State<BotManager>, signals: State<SignalManager>) -> BotDeskStatus {
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
    bots.status()
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
    // position. The default bot used to ask for 100 and skip every signal.
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
