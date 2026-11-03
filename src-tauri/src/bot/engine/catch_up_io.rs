//! The catch-up's reads (see `catch_up.rs` for the rules): Sentinel's graded
//! outcome of the signal and the gap's 1m candles, both through the Sentinel
//! API. Throttled per position so a running book asks Sentinel at most every
//! few minutes, and a failed read retries a minute later instead of every tick.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::bot::strategy::backtest::{rfc3339, Candle};
use crate::market::history::{self, HistoryError};
use crate::membership::MembershipManager;
use crate::store::model::TradeRecord;
use crate::store::StoreManager;

use super::super::model::OpenPosition;
use super::catch_up::{
    closed_minutes, forced_close, is_sentinel_id, parse_outcome, price_at, replay, Forced,
    SentinelOutcome, MINUTE_MS,
};
use super::recalc::{recalculate, Verdict};
use super::record::trade_record;

/// A followed position's outcome is re-read this often.
const FOLLOW_EVERY_MS: u64 = 5 * MINUTE_MS;
/// Once Sentinel has invalidated the signal its graded close lands within a
/// few minutes; read it every minute until then.
const FOLLOW_INVALIDATED_EVERY_MS: u64 = MINUTE_MS;
/// A gap whose candles could not be read is retried this often.
const GAP_RETRY_MS: u64 = MINUTE_MS;
/// The server's cap per history request.
const PAGE_MINUTES: u64 = 9_000;
/// No position lives longer (entry window + expiry + 72h); older minutes are
/// never fetched.
const MAX_GAP_MS: u64 = 7 * 24 * 60 * MINUTE_MS;
const OUTCOME_TIMEOUT: Duration = Duration::from_secs(10);

/// What the catch-up found for one paper position.
#[derive(Debug)]
pub enum CatchUp {
    /// Close it with this record (exit price and time already Sentinel's or
    /// the replay's). Boxed: a record dwarfs the other arms.
    Close(Box<TradeRecord>),
    /// The gap was replayed without an exit; the management state may have
    /// advanced (persist it) and the position counts as watched again.
    Replayed,
    /// Not due, or nothing to do.
    Nothing,
    /// The gap could not be replayed (feed note key).
    Failed(&'static str),
}

fn last_try() -> &'static Mutex<HashMap<String, u64>> {
    static MAP: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

fn due(key: &str, every_ms: u64, now_ms: u64) -> bool {
    let mut map = last_try().lock().expect("catch-up mutex");
    let ready = map.get(key).is_none_or(|&at| now_ms.saturating_sub(at) >= every_ms);
    if ready {
        map.insert(key.to_string(), now_ms);
    }
    ready
}

/// Catches one PAPER position up. `gap_from` is the last moment the book saw
/// it (Some only when that is more than a minute ago).
pub async fn paper_catch_up(
    app: &AppHandle,
    pos: &mut OpenPosition,
    gap_from: Option<u64>,
    invalidated: bool,
    max_loss_pct: Option<f64>,
    now_ms: u64,
) -> CatchUp {
    let key = format!("{}|{}", pos.signal_id, pos.bot_kind.as_str());
    let every = if gap_from.is_some() {
        GAP_RETRY_MS
    } else if invalidated {
        FOLLOW_INVALIDATED_EVERY_MS
    } else {
        FOLLOW_EVERY_MS
    };
    if !due(&key, every, now_ms) {
        return CatchUp::Nothing;
    }
    let market = if pos.bot_kind.uses_futures_market() { "futures" } else { "spot" };
    let base = app.state::<MembershipManager>().base_url();
    let client = history::build_client();

    let forced = if is_sentinel_id(&pos.signal_id) {
        fetch_outcome(&client, &base, &pos.signal_id)
            .await
            .and_then(|o| forced_close(pos, &o, max_loss_pct, market))
    } else {
        None
    };
    let needs_candles = gap_from.is_some() || forced.as_ref().is_some_and(|f| f.price.is_none());
    if !needs_candles {
        return forced.map_or(CatchUp::Nothing, |f| {
            let price = f.price.unwrap_or(pos.entry);
            CatchUp::Close(Box::new(record(pos, &f, price)))
        });
    }

    // The window: from the last look (or a minute before Sentinel's exit)
    // to now, or just past Sentinel's exit when there is no gap.
    let from = match (gap_from, &forced) {
        (Some(g), _) => g,
        (None, Some(f)) => f.at_ms.saturating_sub(MINUTE_MS),
        (None, None) => return CatchUp::Nothing,
    }
    .max(pos.opened_at)
    .max(now_ms.saturating_sub(MAX_GAP_MS));
    let to = match (gap_from, &forced) {
        (None, Some(f)) => (f.at_ms + 2 * MINUTE_MS).min(now_ms),
        _ => now_ms,
    };
    let candles = match fetch_minutes(app, &client, &base, &pos.symbol, market, from, to).await {
        Ok(c) => closed_minutes(c, now_ms),
        // Sentinel's own price still closes a followed position.
        Err(key) => {
            return match forced {
                Some(f) if f.price.is_some() => {
                    let price = f.price.unwrap_or(pos.entry);
                    CatchUp::Close(Box::new(record(pos, &f, price)))
                }
                _ => CatchUp::Failed(key),
            }
        }
    };

    if gap_from.is_some() {
        let until = forced.as_ref().map_or(u64::MAX, |f| f.at_ms);
        if let Some((exit, at)) = replay(pos, &candles, max_loss_pct, until) {
            return CatchUp::Close(Box::new(trade_record(pos, exit.price, exit.reason, at)));
        }
    }
    match forced {
        Some(f) => match f.price.or_else(|| price_at(&candles, f.at_ms)) {
            Some(price) => CatchUp::Close(Box::new(record(pos, &f, price))),
            None => CatchUp::Failed("paperCatchUpNoPrice"),
        },
        None => CatchUp::Replayed,
    }
}

fn record(pos: &OpenPosition, f: &Forced, price: f64) -> TradeRecord {
    let mut r = trade_record(pos, price, f.reason, f.at_ms);
    if let Some(code) = &f.code {
        r.veto_reason_code = Some(code.clone());
    }
    r
}

/// GET {base}/api/public/signals/{id} (public). Anything but a readable 200
/// is "no outcome yet".
async fn fetch_outcome(client: &reqwest::Client, base: &str, id: &str) -> Option<SentinelOutcome> {
    let res = client
        .get(format!("{base}/api/public/signals/{id}"))
        .timeout(OUTCOME_TIMEOUT)
        .send()
        .await
        .ok()?;
    if !res.status().is_success() {
        return None;
    }
    parse_outcome(&res.text().await.ok()?)
}

/// 1m candles of `[from, to)` on the position's market, paged at the
/// server's cap; one 401 refreshes the session. Err = feed note key.
async fn fetch_minutes(
    app: &AppHandle,
    client: &reqwest::Client,
    base: &str,
    symbol: &str,
    market: &str,
    from: u64,
    to: u64,
) -> Result<Vec<Candle>, &'static str> {
    let membership = app.state::<MembershipManager>();
    let mut token = membership.fresh_access_token().await.ok_or("paperCatchUpSignIn")?;
    let mut refreshed = false;
    let mut out = Vec::new();
    let mut start = from - from % MINUTE_MS;
    while start < to {
        let end = (start + PAGE_MINUTES * MINUTE_MS).min(to.max(start + MINUTE_MS));
        let page = loop {
            match history::fetch_page(client, base, &token, symbol, market, "1m", &rfc3339(start), &rfc3339(end)).await {
                Err(HistoryError::Unauthorized) if !refreshed => {
                    refreshed = true;
                    if !membership.refresh_after_unauthorized(&token).await {
                        return Err("paperCatchUpSignIn");
                    }
                    token = membership.access_token().ok_or("paperCatchUpSignIn")?;
                }
                Ok(page) => break page,
                Err(_) => return Err("paperCatchUpUnavailable"),
            }
        };
        out.extend(page.candles);
        start = end;
    }
    Ok(out)
}

/// Pause between permalink reads of the startup recalculation.
const RECALC_SPACING: Duration = Duration::from_millis(250);
/// Trades checked per start; the rest on the next one.
const RECALC_BATCH: u32 = 300;

/// Once per start: checks every paper trade not yet checked against
/// Sentinel's graded close and stores the recalculation beside it
/// (`bot::engine::recalc`, `store::recalc`). A trade whose signal Sentinel has
/// not graded yet, or whose read failed, is left for the next start. One
/// feed note says how many were recalculated.
pub async fn recalc_old_trades(app: &AppHandle) {
    let Ok(dir) = app.path().app_data_dir() else {
        return;
    };
    let store = app.state::<StoreManager>();
    let now = super::now_ms();
    let Ok(pending) = store.recalc_pending(&dir, now, RECALC_BATCH) else {
        return;
    };
    let base = app.state::<MembershipManager>().base_url();
    let client = history::build_client();
    let mut recalculated = 0u32;
    for t in pending {
        if !is_sentinel_id(&t.signal_id) {
            let _ = store.save_recalc_kept(&dir, t.id, "notSentinel", now);
            continue;
        }
        let verdict = match fetch_outcome(&client, &base, &t.signal_id).await {
            Some(o) => recalculate(&t, &o),
            None => None,
        };
        match verdict {
            Some(Verdict::Recalculated(r)) => {
                if store.save_recalc(&dir, t.id, &r, now).is_ok() {
                    recalculated += 1;
                }
            }
            Some(Verdict::Kept(status)) => {
                let _ = store.save_recalc_kept(&dir, t.id, status, now);
            }
            None => {}
        }
        tokio::time::sleep(RECALC_SPACING).await;
    }
    if recalculated > 0 {
        app.state::<crate::bot::BotManager>().push_skip("—", "paperRecalculated", Some(recalculated.to_string()));
    }
}
