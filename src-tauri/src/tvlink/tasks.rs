//! The read loop and the glue to the desk. Decisions live in `message`;
//! this file reads, runs the New trade / close paths, and records.

use std::time::Duration;

use tauri::{AppHandle, Manager};

use super::api::{with_session, Event, PAGE, SIGNED_OUT};
use super::message::{self, close_target, decide, needs_price, resolve_levels, Alert, Decision, Partial};
use super::{load_stored, save_stored, AlertResult, TvManager};
use crate::bot::engine::user_trade::{self, TradeRequest};
use crate::bot::BotManager;
use crate::membership::MembershipManager;
use crate::store::StoreManager;
use crate::telegram::notify::Category;

/// Between reads while on.
const POLL: Duration = Duration::from_secs(5);
/// After a failed read.
const BACKOFF: Duration = Duration::from_secs(30);
/// A full page means more are waiting.
const NEXT_PAGE: Duration = Duration::from_millis(250);

fn now_ms() -> u64 {
    crate::bot::engine::now_ms()
}

/// Loads the stored state, then reads for the life of the process. Idle
/// until the user turns "Receive alerts" on, and while signed out.
pub fn spawn(app: &AppHandle) {
    if let Ok(dir) = app.path().app_data_dir() {
        let stored = load_stored(&app.state::<StoreManager>(), &dir);
        app.state::<TvManager>().lock().stored = stored;
    }
    let a = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let wait = pass(&a).await;
            tokio::time::sleep(wait).await;
        }
    });
}

pub(crate) fn save(app: &AppHandle) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|_| "appDataDirUnavailable".to_string())?;
    let stored = app.state::<TvManager>().lock().stored.clone();
    save_stored(&app.state::<StoreManager>(), &dir, &stored)
}

/// One read and the handling of what it returned; the wait before the next.
async fn pass(app: &AppHandle) -> Duration {
    let mgr = app.state::<TvManager>();
    let (active, cursor) = {
        let g = mgr.lock();
        (g.stored.active(), g.stored.cursor.clone())
    };
    if !active {
        return POLL;
    }
    let membership = app.state::<MembershipManager>();
    let base = membership.base_url();
    let read = with_session(membership.inner(), |token| {
        let (api, base, after) = (&mgr.api, base.clone(), cursor.clone());
        async move { api.events(&base, &token, after.as_deref()).await }
    })
    .await;
    let events = match read {
        Ok(e) => e,
        // Signed out: the feature is off, not failing.
        Err(e) if e.code() == SIGNED_OUT => return POLL,
        Err(e) => {
            mgr.lock().last_error = Some(e.code());
            return BACKOFF;
        }
    };
    {
        let mut g = mgr.lock();
        g.last_error = None;
        g.last_poll_at = Some(now_ms());
    }
    let full = events.len() as u32 >= PAGE;
    for ev in events {
        handle(app, ev).await;
    }
    if full {
        NEXT_PAGE
    } else {
        POLL
    }
}

/// One event: the cursor moves (and is saved) BEFORE anything runs, so a
/// crash mid-trade never runs the same alert twice after a restart.
async fn handle(app: &AppHandle, ev: Event) {
    let mgr = app.state::<TvManager>();
    let execute = {
        let mut g = mgr.lock();
        if g.stored.seen(&ev.id) {
            return;
        }
        g.stored.cursor = Some(ev.id.clone());
        g.stored.execute
    };
    if let Err(e) = save(app) {
        mgr.lock().last_error = Some(e);
    }
    let decision = decide(&ev.body, ev.received_at, execute, now_ms());
    let (outcome, seen, position_id) = run(app, decision).await;
    let result = AlertResult {
        event_id: ev.id,
        received_at: ev.received_at,
        handled_at: now_ms(),
        action: seen.action.map(|a| a.as_str().to_string()),
        symbol: seen.symbol,
        bot: seen.bot.map(|b| b.as_str().to_string()),
        outcome,
        position_id,
    };
    mgr.lock().stored.record(result.clone());
    if let Err(e) = save(app) {
        mgr.lock().last_error = Some(e);
    }
    // Execute on and nothing opened or closed: one line to Telegram (bot
    // error). An open adds nothing (no "opened" category); a close is
    // already sent as a closed deal from the trade it books.
    if execute && !matches!(result.outcome.as_str(), message::OPENED | message::CLOSED | message::RECEIVED) {
        let text = refusal_line(&result);
        crate::telegram::send_line(app, Category::BotError, &text).await;
    }
}

/// "TradingView alert not executed: buy BTCUSDT futures · tvExpired".
pub(crate) fn refusal_line(r: &AlertResult) -> String {
    let what: Vec<&str> = [r.action.as_deref(), r.symbol.as_deref(), r.bot.as_deref()].into_iter().flatten().collect();
    let what = if what.is_empty() { "unreadable message".to_string() } else { what.join(" ") };
    format!("TradingView alert not executed: {what} · {}", r.outcome)
}

fn seen_of(a: &Alert) -> Partial {
    Partial { action: Some(a.action), symbol: Some(a.symbol.clone()), bot: Some(a.bot) }
}

/// Runs a decision: (outcome code, what was read, position id).
async fn run(app: &AppHandle, d: Decision) -> (String, Partial, Option<String>) {
    match d {
        Decision::Record { code, seen } => (code, seen, None),
        Decision::Open(alert) => {
            let seen = seen_of(&alert);
            match open(app, &alert).await {
                Ok(id) => (message::OPENED.to_string(), seen, Some(id)),
                Err(code) => (code, seen, None),
            }
        }
        Decision::Close(alert) => {
            let seen = seen_of(&alert);
            let rows = app.state::<BotManager>().positions_snapshot();
            let target = close_target(rows.iter().map(|p| (p.signal_id.as_str(), p.bot_kind, p.symbol.as_str())), alert.bot, &alert.symbol);
            let Some(id) = target else {
                return (message::NOTHING_TO_CLOSE.to_string(), seen, None);
            };
            // The Positions page's own close: claim, paper booking at the
            // live price, exit reason "manual".
            match crate::bot::bot_close_position(app.clone(), id.clone(), alert.bot).await {
                Ok(_) => (message::CLOSED.to_string(), seen, Some(id)),
                Err(code) => (code, seen, Some(id)),
            }
        }
    }
}

/// New trade with the alert's levels (percents from the live price now).
async fn open(app: &AppHandle, a: &Alert) -> Result<String, String> {
    let direction = a.direction().ok_or_else(|| message::BAD_MESSAGE.to_string())?;
    let (tp, sl) = match (a.tp, a.sl) {
        (Some(tp), Some(sl)) => (tp, sl),
        _ => return Err(message::BAD_LEVELS.to_string()),
    };
    let price = if needs_price(a) { user_trade::quote(app, a.bot, &a.symbol).await? } else { 0.0 };
    let (take_profit, stop_loss) = resolve_levels(direction, price, tp, sl);
    let req = TradeRequest {
        kind: a.bot,
        symbol: a.symbol.clone(),
        direction,
        notional_usdt: a.size_usdt,
        take_profit,
        stop_loss,
    };
    user_trade::open(app, &req).await.map(|pos| pos.signal_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_telegram_line_names_the_alert_and_the_code() {
        let r = AlertResult {
            action: Some("buy".into()),
            symbol: Some("BTCUSDT".into()),
            bot: Some("futures".into()),
            outcome: "tvExpired".into(),
            ..Default::default()
        };
        assert_eq!(refusal_line(&r), "TradingView alert not executed: buy BTCUSDT futures · tvExpired");
        let blank = AlertResult { outcome: "tvBadMessage".into(), ..Default::default() };
        assert_eq!(refusal_line(&blank), "TradingView alert not executed: unreadable message · tvBadMessage");
    }
}
