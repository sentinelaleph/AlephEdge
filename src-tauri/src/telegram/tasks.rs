//! The two background loops (notifier, update poller) and their glue to the
//! real desk. Decisions live in `notify` and `route`; this file only reads
//! state, sends, and commits.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aleph_desk_link::DeskControl;
use aleph_link::{BotSelector, DeskStatus};
use tauri::{AppHandle, Manager};

use super::api::{Update, LONG_POLL_SECS};
use super::notify::{self, DeadBot, Snapshot};
use super::{route, read_token, BotToken, Cmd, Route, TelegramManager, SETTINGS_KEY};
use crate::app::keychain::OsKeychain;
use crate::bot::model::LIVE_TRADING_ENABLED;
use crate::bot::strategy::model::BotRunState;
use crate::bot::strategy::StrategyManager;
use crate::bot::BotManager;
use crate::link::DeskBridge;
use crate::risk::RiskManager;
use crate::store::StoreManager;

const NOTIFY_EVERY: Duration = Duration::from_secs(30);
const IDLE_POLL: Duration = Duration::from_secs(3);
const POLL_RETRY: Duration = Duration::from_secs(10);
const MIN_SEND_GAP_MS: u64 = 1_100;

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Loads the token and the settings, then starts both loops for the life of
/// the process. They idle until the user sets Telegram up.
pub fn spawn(app: &AppHandle) {
    load(app);
    let a = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(NOTIFY_EVERY).await;
            notify_pass(&a).await;
        }
    });
    let a = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            if !poll_once(&a).await {
                tokio::time::sleep(IDLE_POLL).await;
            }
        }
    });
}

fn load(app: &AppHandle) {
    let token = read_token(&OsKeychain);
    let settings = app
        .path()
        .app_data_dir()
        .ok()
        .and_then(|dir| app.state::<StoreManager>().get_meta(&dir, SETTINGS_KEY).ok().flatten())
        .and_then(|body| serde_json::from_str(&body).ok())
        .unwrap_or_default();
    let mgr = app.state::<TelegramManager>();
    let mut g = mgr.lock();
    g.token = token;
    g.settings = settings;
    g.loaded = true;
}

pub(crate) fn save_settings(app: &AppHandle, settings: &super::Settings) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|_| "appDataDirUnavailable".to_string())?;
    let body = serde_json::to_string(settings).map_err(|_| "storeWriteFailed".to_string())?;
    app.state::<StoreManager>().set_meta(&dir, SETTINGS_KEY, &body)
}

/// Real money is in play on the desk (the phone link's rule).
pub(crate) fn desk_live(app: &AppHandle) -> bool {
    LIVE_TRADING_ENABLED && crate::bot::strategy_live::live_exposure(app)
}

/// Everything a pass reads, from the store and the managers.
pub(crate) fn snapshot(app: &AppHandle) -> Result<Snapshot, String> {
    let dir = app.path().app_data_dir().map_err(|_| "appDataDirUnavailable".to_string())?;
    let store = app.state::<StoreManager>();
    let trades = store.list_trades(&dir, 200)?;
    // Paper cycles only, as `strategy_pnl`: a live bot's simulated cycles are
    // its decision book, not the money it made.
    let live_ids = if LIVE_TRADING_ENABLED {
        crate::bot::strategy_live::stored_live_ids(app).unwrap_or_default()
    } else {
        Default::default()
    };
    let cycles = store.strategy(&dir, |c| crate::store::strategy::recent_closed_cycles(c, 100, &live_ids))?;
    let bots = app.state::<BotManager>();
    let strategy = app.state::<StrategyManager>();
    let dead = strategy
        .bots()
        .into_iter()
        .filter(|b| b.state == BotRunState::Dead && b.archived_at.is_none())
        .map(|b| DeadBot {
            live: live_ids.contains(&b.id),
            id: b.id.clone(),
            name: b.cfg.name.clone(),
            kind: b.cfg.kind().as_str().to_string(),
            reason: b.dead_reason.map(|r| r.as_str().to_string()),
        })
        .collect();
    Ok(Snapshot {
        now_ms: now_ms(),
        desk_live: desk_live(app),
        trades,
        cycles,
        skips: bots.status().recent_skips,
        kill_tripped: bots.kill_switch_tripped(),
        daily_loss_pct: app.state::<RiskManager>().effective_daily_loss_pct(),
        breaker_tripped: strategy.portfolio_tripped(),
        breaker_dd_pct: strategy.risk().portfolio_dd_pct,
        dead,
    })
}

/// Sets the cursors to "now" (no backlog) and starts a new generation.
pub(crate) fn reset_cursors(app: &AppHandle, inner: &mut super::Inner) -> Result<(), String> {
    let snap = snapshot(app)?;
    let (cursors, watch) = notify::reset(&snap);
    inner.settings.cursors = cursors;
    inner.watch = watch;
    inner.generation += 1;
    Ok(())
}

pub(crate) fn set_error(app: &AppHandle, code: &str, from_poll: bool) {
    app.state::<TelegramManager>().lock().last_error = Some((code.to_string(), from_poll));
}

/// One message, at most one a second.
pub(crate) async fn send(app: &AppHandle, token: &BotToken, chat: i64, text: &str) -> Result<(), String> {
    let mgr = app.state::<TelegramManager>();
    let mut last = mgr.send_gate.lock().await;
    let wait = (*last + MIN_SEND_GAP_MS).saturating_sub(now_ms());
    if wait > 0 {
        tokio::time::sleep(Duration::from_millis(wait)).await;
    }
    let r = mgr.api.send_message(token, chat, text).await;
    *last = now_ms();
    drop(last);
    let mut g = mgr.lock();
    match &r {
        Ok(()) => {
            g.last_sent_at = Some(now_ms());
            if g.last_error.as_ref().is_some_and(|(_, poll)| !poll) {
                g.last_error = None;
            }
        }
        Err(e) => g.last_error = Some((e.clone(), false)),
    }
    r
}

async fn notify_pass(app: &AppHandle) {
    let mgr = app.state::<TelegramManager>();
    let Some((token, chat)) = mgr.lock().active() else {
        return;
    };
    let snap = match snapshot(app) {
        Ok(s) => s,
        Err(e) => return set_error(app, &e, false),
    };
    let (events, cursors, watch, categories, generation) = {
        let g = mgr.lock();
        let (events, cursors, watch) = notify::pass(&snap, &g.settings.cursors, &g.watch);
        (events, cursors, watch, g.settings.categories, g.generation)
    };
    let wanted: Vec<_> = events.into_iter().filter(|e| categories.allows(&e.cats)).collect();
    let mut first_failed = false;
    for (i, text) in notify::compose(&wanted).iter().enumerate() {
        if send(app, &token, chat, text).await.is_err() {
            // Nothing went out: keep the cursors and try again next pass.
            // A later failure keeps what was sent from going out twice.
            first_failed = i == 0;
            break;
        }
    }
    if first_failed {
        return;
    }
    let mut g = mgr.lock();
    if g.generation != generation || g.active().is_none() {
        return;
    }
    let changed = g.settings.cursors != cursors;
    g.settings.cursors = cursors;
    g.watch = watch;
    if changed {
        let settings = g.settings.clone();
        drop(g);
        if let Err(e) = save_settings(app, &settings) {
            set_error(app, &e, false);
        }
    }
}

/// One `getUpdates` round. `false` when there was nothing to poll for.
async fn poll_once(app: &AppHandle) -> bool {
    let mgr = app.state::<TelegramManager>();
    let (token, offset) = {
        let g = mgr.lock();
        if !g.should_poll(now_ms()) {
            return false;
        }
        (g.token.clone().expect("should_poll needs a token"), g.settings.update_offset)
    };
    let updates = match mgr.api.get_updates(&token, offset, LONG_POLL_SECS).await {
        Ok(u) => u,
        Err(e) => {
            set_error(app, &e, true);
            tokio::time::sleep(POLL_RETRY).await;
            return true;
        }
    };
    {
        let mut g = mgr.lock();
        if g.last_error.as_ref().is_some_and(|(_, poll)| *poll) {
            g.last_error = None;
        }
    }
    let mut next = offset;
    for u in &updates {
        next = next.max(u.update_id + 1);
        handle(app, &token, u).await;
    }
    if next != offset {
        let settings = {
            let mut g = mgr.lock();
            // The token was replaced or removed meanwhile: the offset belongs
            // to the old bot.
            if g.token.as_ref() != Some(&token) {
                return true;
            }
            g.settings.update_offset = next;
            g.settings.clone()
        };
        if let Err(e) = save_settings(app, &settings) {
            set_error(app, &e, false);
        }
    }
    true
}

async fn handle(app: &AppHandle, token: &BotToken, u: &Update) {
    let Some(msg) = &u.message else {
        return;
    };
    let mgr = app.state::<TelegramManager>();
    let r = {
        let g = mgr.lock();
        // Read with a token that was replaced or removed meanwhile.
        if g.token.as_ref() != Some(token) {
            return;
        }
        route(msg, g.settings.chat_id, g.link.as_ref(), g.settings.commands_enabled, now_ms())
    };
    match r {
        Route::Ignore => {}
        Route::Link { chat_id, label } => {
            let settings = {
                let mut g = mgr.lock();
                g.settings.chat_id = Some(chat_id);
                g.settings.chat_label = label;
                g.settings.enabled = true;
                g.link = None;
                if let Err(e) = reset_cursors(app, &mut g) {
                    g.last_error = Some((e, false));
                }
                g.settings.clone()
            };
            if let Err(e) = save_settings(app, &settings) {
                set_error(app, &e, false);
            }
            let text = format!(
                "{}\nLinked to this chat. Notifications are on. Commands are {}.",
                notify::header(desk_live(app)),
                if settings.commands_enabled { "on" } else { "off" }
            );
            let _ = send(app, token, chat_id, &text).await;
        }
        Route::CommandsOff { chat_id } => {
            let text = format!(
                "{}\nCommands are off. Turn them on in Settings, Notifications, Telegram.",
                notify::header(desk_live(app))
            );
            let _ = send(app, token, chat_id, &text).await;
        }
        Route::Command { chat_id, cmd } => {
            let text = run_command(app, cmd).await;
            let _ = send(app, token, chat_id, &text).await;
        }
    }
}

/// `/status` and `/stop` through the phone link's own desk bridge.
async fn run_command(app: &AppHandle, cmd: Cmd) -> String {
    let live = desk_live(app);
    match cmd {
        Cmd::Status => status_text(live, &DeskBridge::new(app.clone()).status()),
        Cmd::Stop => {
            let mut bridge = DeskBridge::new(app.clone());
            match bridge.stop_opening(BotSelector::All).await {
                Ok(ack) | Err(ack) => {
                    let open = ack.params.get("open").copied();
                    stop_text(live, ack.code == aleph_link::ack_code::STOPPED, open)
                }
            }
        }
        Cmd::Help => help_text(live),
    }
}

pub(crate) fn status_text(live: bool, st: &DeskStatus) -> String {
    let bots = if st.bots_running.is_empty() {
        "0".to_string()
    } else {
        format!("{} ({})", st.bots_running.len(), st.bots_running.join(", "))
    };
    format!(
        "{}\nBots running: {bots}\nOpen positions: {}\nToday net: {:+.2}%\nDaily loss stop: {}",
        notify::header(live),
        st.open_positions,
        st.today_net_pct,
        if st.kill_switch_tripped { "tripped" } else { "not tripped" }
    )
}

pub(crate) fn stop_text(live: bool, stopped: bool, open: Option<i64>) -> String {
    if !stopped {
        return format!("{}\nStop failed. Check the app.", notify::header(live));
    }
    let open = open.map(|n| n.to_string()).unwrap_or_else(|| "unknown".into());
    format!(
        "{}\nNew entries stopped on every bot (signal, DCA, Grid). Nothing was closed. Open positions still managed: {open}.",
        notify::header(live)
    )
}

pub(crate) fn help_text(live: bool) -> String {
    format!(
        "{}\n/status: bots running, open positions, today's net.\n/stop: stop new entries on every bot. Open positions stay open.",
        notify::header(live)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reply_states_the_desk() {
        let st = DeskStatus {
            bots_running: vec!["futures".into(), "dca".into()],
            open_positions: 3,
            today_net_pct: 0.4249,
            skips_today: vec![],
            exposure_pct: 0.0,
            effective_bets: 0.0,
            kill_switch_tripped: false,
        };
        assert_eq!(
            status_text(false, &st),
            "Aleph Edge · Paper\nBots running: 2 (futures, dca)\nOpen positions: 3\nToday net: +0.42%\nDaily loss stop: not tripped"
        );
        let idle = DeskStatus { bots_running: vec![], kill_switch_tripped: true, today_net_pct: -1.0, ..st };
        let t = status_text(true, &idle);
        assert!(t.starts_with("Aleph Edge · LIVE\nBots running: 0\n") && t.ends_with("Today net: -1.00%\nDaily loss stop: tripped"), "{t}");
    }

    #[test]
    fn stop_reply_says_nothing_was_closed() {
        let t = stop_text(false, true, Some(2));
        assert!(t.contains("Nothing was closed") && t.ends_with("still managed: 2."), "{t}");
        assert!(stop_text(true, false, None).ends_with("Stop failed. Check the app."));
        assert!(help_text(false).contains("/status") && help_text(false).contains("/stop"));
    }
}
