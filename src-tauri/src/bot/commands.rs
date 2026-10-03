//! Tauri command surface for the bot desk.

use tauri::{AppHandle, Manager, State};

use super::model::{BotConfig, BotDeskStatus, BotKind, LIVE_TRADING_ENABLED, MAX_BOT_POSITIONS};
use super::BotManager;
use crate::signal::model::StreamNoteKind;
use crate::signal::SignalManager;

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
    config.live = bots.config_for(config.kind).is_some_and(|c| c.live);
    bots.save_config(&app, &config);
    bots.configure(config);
    Ok(bots.status())
}

/// The exact text the user types to switch a bot to real money.
pub const LIVE_CONFIRMATION: &str = "LIVE";

/// Switches one bot's real-money trading on or off. On needs: a live build,
/// the Futures bot on Binance, a Trade key in the unlocked vault, and the
/// typed confirmation. Off always works. Never persisted as on: a restart
/// comes back simulated (see `BotManager::restore_configs`).
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
        if !LIVE_TRADING_ENABLED {
            return Err("liveBuildDisabled".to_string());
        }
        if kind != BotKind::Futures || cfg.exchange_id != "binance" {
            return Err("liveFuturesBinanceOnly".to_string());
        }
        if confirmation.trim() != LIVE_CONFIRMATION {
            return Err(format!("liveConfirmRequired|{LIVE_CONFIRMATION}"));
        }
        // A verified trade-only key: a key stored before verification was
        // mandatory (Unknown) or one that can withdraw never goes live.
        match app
            .state::<crate::vault::VaultManager>()
            .credential("binance")
            .map(|c| c.permission)
        {
            Some(crate::vault::model::CredentialPermission::TradeOnly) => {}
            Some(_) => return Err("liveNeedsVerifiedKey".to_string()),
            None => return Err("liveNeedsTradeKey".to_string()),
        }
    }
    cfg.live = enabled;
    bots.configure(cfg);
    Ok(bots.status())
}

/// Defaults for a NEW bot (risk sizing, 1% per trade), so the UI never
/// re-declares them. Saved configs keep whatever they carry.
#[tauri::command]
pub fn bot_default_config(kind: BotKind, exchange_id: String) -> BotConfig {
    BotConfig::new_default(kind, &exchange_id)
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
pub fn bot_stop(bots: State<BotManager>, kind: BotKind) -> BotDeskStatus {
    bots.stop(kind);
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
