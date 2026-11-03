//! Real Pine entries whose outcome stayed unknown, looked up by client id
//! until Binance answers, and the Telegram lines of a Live bot's actions.

use tauri::{AppHandle, Manager};

use crate::bot::engine::{tv_trade, user_entry};
use crate::bot::model::BotKind;
use crate::store::StoreManager;
use crate::telegram::notify::Category;

use super::act::{fill_row, save};
use super::types::{PinePending, PineResult};
use super::PineManager;

/// Between lookups of pending real entries.
const PENDING_EVERY_MS: u64 = 10_000;
/// A pending entry Binance never answered for is given up after this.
const PENDING_GIVE_UP_MS: u64 = 3_600_000;

fn now_ms() -> u64 {
    crate::bot::engine::now_ms()
}

/// Looks up every pending real entry (at most every PENDING_EVERY_MS) and
/// writes its verdict on its row.
pub(super) async fn resolve_pending_all(app: &AppHandle) {
    let now = now_ms();
    {
        let mgr = app.state::<PineManager>();
        let mut g = mgr.lock();
        if now.saturating_sub(g.last_pending_ms) < PENDING_EVERY_MS {
            return;
        }
        g.last_pending_ms = now;
    }
    let Ok(dir) = app.path().app_data_dir() else {
        return;
    };
    let Ok(rows) = app.state::<StoreManager>().pine_pending(&dir) else {
        return;
    };
    for (_, pending, body) in rows {
        let (Ok(pend), Ok(mut row)) = (serde_json::from_str::<PinePending>(&pending), serde_json::from_str::<PineResult>(&body)) else {
            continue;
        };
        let age = now_ms().saturating_sub(pend.sent_at);
        let verdict = user_entry::resolve_pending(app, BotKind::Pine, &pend.planned, (pend.tp, pend.sl), age).await;
        row.outcome = match verdict {
            tv_trade::Pending::Filled(fill) => {
                let mut pos = pend.planned.clone();
                (pos.entry, pos.qty) = (fill.avg_price, fill.executed_qty);
                fill_row(&mut row, &pos, pend.planned.qty);
                crate::tvlink::message::OPENED.to_string()
            }
            tv_trade::Pending::Failed => crate::bot::engine::live::SKIP_ORDER_FAILED.to_string(),
            tv_trade::Pending::Wait if age >= PENDING_GIVE_UP_MS => crate::bot::engine::live::SKIP_ORDER_UNCONFIRMED.to_string(),
            tv_trade::Pending::Wait => continue,
        };
        row.handled_at = now_ms();
        save(app, &row, true, None);
        notify(app, true, &row).await;
    }
}

/// Telegram on a Live bot: every open, every refusal with its code. A close
/// is already sent as a closed deal from the trade it books.
pub(super) async fn notify(app: &AppHandle, live: bool, r: &PineResult) {
    if !live {
        return;
    }
    let text = match r.outcome.as_str() {
        "opened" | "flipped" => live_open_line(r),
        "closed" => return,
        _ => refusal_line(r),
    };
    crate::telegram::send_line_for(app, Category::BotError, &text, live).await;
}

fn action_word(r: &PineResult) -> &'static str {
    match r.side.as_deref() {
        Some("long") => "buy",
        Some("short") => "sell",
        _ => "close",
    }
}

/// "Pine LIVE open: buy BTCUSDT 0.002 at 65010.5 (Trend)".
pub fn live_open_line(r: &PineResult) -> String {
    let num = |v: Option<f64>| v.map_or("?".to_string(), |x| format!("{x}"));
    format!("Pine LIVE open: {} {} {} at {} ({})", action_word(r), r.symbol, num(r.filled_qty), num(r.fill_price), r.bot_name)
}

/// "Pine bot action not executed: buy BTCUSDT 1h (Trend) · pinePriceMoved|0.80%".
pub fn refusal_line(r: &PineResult) -> String {
    format!("Pine bot action not executed: {} {} {} ({}) · {}", action_word(r), r.symbol, r.timeframe, r.bot_name, r.outcome)
}
