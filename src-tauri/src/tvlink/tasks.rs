//! The read loop and the glue to the desk. Decisions live in `message` and
//! `guard`; the TradingView bot's entry in `bot::engine::tv_trade`; this
//! file reads (stream, or poll as the fallback), runs the open / close /
//! flip paths, and records.

use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use super::api::{with_session, ApiError, Event, EventsPage, PAGE, SIGNED_OUT};
use super::guard::{self, SeenAlert, Usage};
use super::message::{self, close_target, decide, needs_price, resolve_levels, Alert, Clock, Decision, Level, Partial, Plan};
use super::sse::{self, End, Item, Mode};
use super::{load_stored, save_stored, AlertResult, PendingEntry, TvManager};
use crate::bot::engine::tv_passphrase;
use crate::bot::engine::tv_trade::{self, other_holder, own_position};
use crate::bot::engine::user_trade::{self, TradeRequest};
use crate::bot::model::{BotConfig, BotKind, OpenPosition, TradeMode};
use crate::bot::BotManager;
use crate::exchange::ExchangeManager;
use crate::membership::MembershipManager;
use crate::risk::RiskManager;
use crate::signal::model::Direction;
use crate::store::StoreManager;
use crate::telegram::notify::Category;

/// Between polls while on (the fallback when the stream is refused).
const POLL: Duration = Duration::from_secs(5);
/// Between polls while a Live TradingView bot runs.
const LIVE_POLL: Duration = Duration::from_secs(1);
/// After a failed read.
const BACKOFF: Duration = Duration::from_secs(30);
/// A full page means more are waiting.
const NEXT_PAGE: Duration = Duration::from_millis(250);
/// Between lookups of real entries whose outcome is unknown.
const PENDING_EVERY: Duration = Duration::from_secs(10);
/// A pending entry Binance never answered for is given up after this, said
/// loudly; the account's untracked-position report still shows any exposure.
const PENDING_GIVE_UP_MS: u64 = 3_600_000;
/// Between checks of the server's offline notice (default on for Live).
const NOTIFY_EVERY: Duration = Duration::from_secs(600);

pub const PENDING: &str = "livePending";
/// A spot alert while the TradingView bot is Live: spot stays paper.
pub const SPOT_PAPER_ONLY: &str = "tvSpotPaperOnly";
pub const BOT_REQUIRED: &str = "tvBotRequired";
pub const BOT_STOPPED: &str = "tvBotStopped";
/// A close alert on a symbol only another bot holds.
pub const CLOSE_NOT_OWN: &str = "tvCloseNotOwn";
/// A Live entry on a symbol another bot holds (one exchange position per
/// symbol).
pub const HELD_BY_OTHER: &str = "tvSymbolHeldBy";
/// A flip whose close leg did not complete: nothing opened.
pub const FLIP_CLOSE_FAILED: &str = "tvFlipCloseFailed";
/// A flip that closed but whose open leg was refused.
pub const FLIP_OPEN_REFUSED: &str = "tvFlipOpenRefused";
/// Emitted (with the event id) each time an alert's row is written or
/// updated, so the results table refreshes without waiting for a poll.
pub const RESULT_EVENT: &str = "tvlink:result";

fn now_ms() -> u64 {
    crate::bot::engine::now_ms()
}

/// Loads the stored state, then reads for the life of the process, and
/// looks up pending real entries. Idle until the user turns "Receive
/// alerts" on, and while signed out.
pub fn spawn(app: &AppHandle) {
    if let Ok(dir) = app.path().app_data_dir() {
        let stored = load_stored(&app.state::<StoreManager>(), &dir);
        app.state::<TvManager>().lock().stored = stored;
    }
    let a = app.clone();
    tauri::async_runtime::spawn(async move { read_loop(&a).await });
    let b = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut notify_at: Option<Instant> = None;
        loop {
            tokio::time::sleep(PENDING_EVERY).await;
            resolve_pending_all(&b).await;
            if notify_at.is_none_or(|t| t.elapsed() >= NOTIFY_EVERY) {
                notify_at = Some(Instant::now());
                ensure_offline_notice(&b).await;
            }
        }
    });
}

pub(crate) fn save(app: &AppHandle) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|_| "appDataDirUnavailable".to_string())?;
    let stored = app.state::<TvManager>().lock().stored.clone();
    save_stored(&app.state::<StoreManager>(), &dir, &stored)
}

fn active(app: &AppHandle) -> bool {
    app.state::<TvManager>().lock().stored.active()
}

/// A Live TradingView bot is running: alerts move real money, so the poll
/// fallback reads every second.
fn live_bot_running(app: &AppHandle) -> bool {
    app.state::<BotManager>().running_config(BotKind::Tradingview).is_some_and(|c| c.mode == TradeMode::Live)
}

/// The poll interval while the stream is refused (pure).
pub fn poll_interval(live_running: bool) -> Duration {
    if live_running {
        LIVE_POLL
    } else {
        POLL
    }
}

/// Stream first; a clean close reconnects with the last id at once, a
/// refused or failing stream falls back to the poll for a while
/// (`sse::next_mode`).
async fn read_loop(app: &AppHandle) {
    let mut failures = 0u32;
    let mut poll_until: Option<Instant> = None;
    loop {
        if !active(app) {
            app.state::<TvManager>().lock().transport = None;
            tokio::time::sleep(POLL).await;
            continue;
        }
        if poll_until.is_some_and(|t| Instant::now() < t) {
            let wait = pass(app).await;
            tokio::time::sleep(wait.min(poll_interval(live_bot_running(app)))).await;
            continue;
        }
        let end = match stream_session(app).await {
            Ok(end) => end,
            Err(e) if e.code() == SIGNED_OUT => {
                tokio::time::sleep(POLL).await;
                continue;
            }
            Err(e) => End::Failed(e.code()),
        };
        {
            let mgr = app.state::<TvManager>();
            let mut g = mgr.lock();
            g.transport = None;
            if let Some(code) = stream_failure(&end) {
                g.last_error = Some(code.clone());
                g.stream_error = Some(code);
            }
        }
        failures = if matches!(end, End::Closed | End::Stopped) { 0 } else { failures + 1 };
        let (mode, delay) = sse::next_mode(&end, failures);
        if let Mode::Poll(spell) = mode {
            poll_until = Some(Instant::now() + spell);
            failures = 0;
        }
        tokio::time::sleep(delay).await;
    }
}

/// Why a stream session counts as failed, as the panel shows it while the
/// poll reads (pure): a refusal's code, or `tvStreamIdle` when no byte came
/// for the idle limit (a proxy holding the stream back).
pub fn stream_failure(end: &End) -> Option<String> {
    match end {
        End::Failed(code) => Some(code.clone()),
        End::Idle => Some("tvStreamIdle".to_string()),
        End::Closed | End::Stopped => None,
    }
}

/// One stream session with the signed-in session's token.
async fn stream_session(app: &AppHandle) -> Result<End, ApiError> {
    let membership = app.state::<MembershipManager>();
    let base = membership.base_url();
    with_session(membership.inner(), |token| {
        let app = app.clone();
        let base = base.clone();
        async move {
            let mgr = app.state::<TvManager>();
            let after = mgr.lock().stored.cursor.clone();
            mgr.api
                .stream(&base, &token, after.as_deref(), |item| {
                    let app = app.clone();
                    async move { on_item(&app, item).await }
                })
                .await
        }
    })
    .await
}

/// One stream frame: the server's clock, then its events in order. False
/// ends the session ("Receive alerts" off).
async fn on_item(app: &AppHandle, item: Item) -> bool {
    let mgr = app.state::<TvManager>();
    let (now, events) = match item {
        Item::Events { page, .. } => (page.now, page.events),
        Item::Ping { now } => (now, Vec::new()),
        Item::Other => (None, Vec::new()),
    };
    {
        let mut g = mgr.lock();
        g.transport = Some("stream");
        g.last_error = None;
        g.stream_error = None;
        g.last_poll_at = Some(now_ms());
        if let Some(n) = now {
            g.server_clock = Some((n, Instant::now()));
        }
    }
    if !events.is_empty() {
        let _one = mgr.pass_lock.lock().await;
        for ev in events {
            handle(app, ev).await;
        }
    }
    active(app)
}

/// One poll and the handling of what it returned; the wait before the next.
pub(crate) async fn pass(app: &AppHandle) -> Duration {
    let mgr = app.state::<TvManager>();
    // The loop and "Send test alert" never read at the same time.
    let _one = mgr.pass_lock.lock().await;
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
    let EventsPage { events, now } = match read {
        Ok(p) => p,
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
        if g.transport.is_none() {
            g.transport = Some("poll");
        }
        if let Some(n) = now {
            g.server_clock = Some((n, Instant::now()));
        }
    }
    let full = events.len() as u32 >= PAGE;
    for ev in events {
        handle(app, ev).await;
    }
    if full {
        NEXT_PAGE
    } else {
        poll_interval(live_bot_running(app))
    }
}

/// The stale window in force: the TradingView bot's own, else the default.
fn stale_window_ms(cfg: Option<&BotConfig>) -> u64 {
    cfg.and_then(|c| c.tv.as_ref()).map_or(message::DEFAULT_STALE_MS, |t| u64::from(t.stale_s) * 1000)
}

/// One event: the cursor moves (and is saved) BEFORE anything runs, so a
/// crash mid-trade never runs the same alert twice after a restart.
async fn handle(app: &AppHandle, ev: Event) {
    let mgr = app.state::<TvManager>();
    let (execute, server_now) = {
        let mut g = mgr.lock();
        if g.stored.seen(&ev.id) {
            return;
        }
        g.stored.cursor = Some(ev.id.clone());
        (g.stored.execute, g.server_now())
    };
    if let Err(e) = save(app) {
        mgr.lock().last_error = Some(e);
    }
    let clock = Clock { received_at_ms: ev.received_at, server_now_ms: server_now, app_offline: ev.app_offline };
    let tv_cfg = app.state::<BotManager>().config_for(BotKind::Tradingview);
    // A test event can only come back as Decision::Record (message.rs).
    let decision = decide(&ev.body, ev.test, execute, &clock, stale_window_ms(tv_cfg.as_ref()));
    let mut result = AlertResult {
        event_id: ev.id.clone(),
        received_at: ev.received_at,
        handled_at: now_ms(),
        test: ev.test,
        ..Default::default()
    };
    match decision {
        Decision::Record { code, seen } => {
            fill_seen(&mut result, &seen);
            result.outcome = code;
        }
        Decision::Execute(alert) => run(app, &ev, alert, &mut result).await,
    }
    result.test = result.test || result.outcome == message::TEST_ONLY;
    if result.bot_name.is_none() {
        attribute(&mut result, tv_cfg.as_ref());
    }
    mgr.lock().stored.record(result.clone());
    if let Err(e) = save(app) {
        mgr.lock().last_error = Some(e);
    }
    let _ = app.emit(RESULT_EVENT, &result.event_id);
    notify(app, execute, &result).await;
}

/// Telegram: every Live open, every refusal (with its code) and failure.
/// A close is already sent as a closed deal from the trade it books.
async fn notify(app: &AppHandle, execute: bool, r: &AlertResult) {
    if !execute || r.test {
        return;
    }
    let live = r.mode.as_deref() == Some("live");
    let text = match r.outcome.as_str() {
        message::OPENED | message::FLIPPED if live => live_open_line(r),
        message::OPENED | message::FLIPPED | message::CLOSED | message::RECEIVED => return,
        _ => refusal_line(r),
    };
    crate::telegram::send_line_for(app, Category::BotError, &text, live).await;
}

/// Names the bot a futures alert is for: the TradingView bot (its name and
/// mode), also on a row recorded without running (stale, refused, test), so
/// the table never shows it as the Futures bot's (pure). A spot alert stays
/// the Spot bot's.
pub(crate) fn attribute(r: &mut AlertResult, tv: Option<&BotConfig>) {
    if r.bot.as_deref() == Some("spot") {
        return;
    }
    let Some(cfg) = tv else { return };
    r.bot_name = Some(cfg.tv.as_ref().map_or_else(|| "TradingView".to_string(), |t| t.name.clone()));
    r.mode = Some(if cfg.mode == TradeMode::Live { "live" } else { "paper" }.to_string());
}

fn fill_seen(r: &mut AlertResult, seen: &Partial) {
    r.action = seen.action.map(|a| a.as_str().to_string());
    r.symbol = seen.symbol.clone();
    r.bot = seen.bot.map(|b| b.as_str().to_string());
}

/// "TradingView alert not executed: buy BTCUSDT futures · tvExpired".
pub(crate) fn refusal_line(r: &AlertResult) -> String {
    let what: Vec<&str> = [r.action.as_deref(), r.symbol.as_deref(), r.bot.as_deref()].into_iter().flatten().collect();
    let what = if what.is_empty() { "unreadable message".to_string() } else { what.join(" ") };
    let by = r.bot_name.as_deref().map(|n| format!(" ({n})")).unwrap_or_default();
    format!("TradingView alert not executed: {what}{by} · {}", r.outcome)
}

/// "TradingView LIVE open: buy BTCUSDT 0.002 at 65010.5 (My TV bot)".
pub(crate) fn live_open_line(r: &AlertResult) -> String {
    let num = |v: Option<f64>| v.map_or("?".to_string(), |x| format!("{x}"));
    format!(
        "TradingView LIVE open: {} {} {} at {} ({})",
        r.action.as_deref().unwrap_or("?"),
        r.symbol.as_deref().unwrap_or("?"),
        num(r.filled_qty),
        num(r.avg_fill),
        r.bot_name.as_deref().unwrap_or("TradingView"),
    )
}

/// The fill against the alert's price, in %, positive when it is worse for
/// the trade (a long filled above, a short below).
pub fn slippage_pct(direction: Direction, fill: f64, alert_price: f64) -> Option<f64> {
    if !(fill.is_finite() && alert_price.is_finite() && alert_price > 0.0 && fill > 0.0) {
        return None;
    }
    let raw = (fill - alert_price) / alert_price * 100.0;
    Some(match direction {
        Direction::Long => raw,
        Direction::Short => -raw,
    })
}

/// A price level scaled onto the bot's symbol (a renamed multiplier
/// symbol, `guard::map_symbol`); percents stay as they are.
pub fn scale_level(l: Level, factor: f64) -> Level {
    match l {
        Level::Price(p) => Level::Price(p * factor),
        pct => pct,
    }
}

/// Runs one alert that passed the message checks.
async fn run(app: &AppHandle, ev: &Event, alert: Alert, r: &mut AlertResult) {
    fill_seen(r, &Partial::of(&alert));
    r.alert_price = alert.price;
    if let Some(p) = alert.market_position {
        r.strategy = Some(format!("{} {}", alert.action.as_str(), format!("{p:?}").to_ascii_lowercase()));
    }
    let bots = app.state::<BotManager>();
    let cfg = bots.config_for(BotKind::Tradingview);
    if let Some(cfg) = &cfg {
        let tv = cfg.tv.clone().unwrap_or_default();
        attribute(r, Some(cfg));
        let given = alert.passphrase.as_ref().map(|s| s.0.as_str());
        match tv_passphrase::check(&tv, given) {
            tv_passphrase::Verdict::Refused => {
                r.outcome = guard::PASSPHRASE.to_string();
                return;
            }
            // Saved by 0.3.19 (salted SHA-256): the alert passes, and the
            // passphrase it just proved is stored as Argon2id from now on.
            tv_passphrase::Verdict::LegacyMatch => {
                if let (Some(matched), Some(g)) = (tv.passphrase_sha256.as_deref(), given) {
                    if let Ok(h) = tv_passphrase::hash(g.trim()) {
                        bots.upgrade_tv_passphrase(app, matched, h);
                    }
                }
            }
            tv_passphrase::Verdict::Match | tv_passphrase::Verdict::NoPassphrase => {}
        }
    }
    // The duplicate guard: same body, or same `id`, within 5 minutes.
    {
        let mgr = app.state::<TvManager>();
        let mut g = mgr.lock();
        let hash = guard::body_hash(&ev.body);
        let at = ev.received_at.unwrap_or(0);
        if guard::is_duplicate(&g.stored.seen, &hash, alert.tag.as_deref(), at) {
            r.outcome = guard::DUPLICATE.to_string();
            return;
        }
        guard::remember(&mut g.stored.seen, SeenAlert { hash, tag: alert.tag.clone(), at_ms: at });
    }
    if alert.bot == BotKind::Spot {
        r.outcome = run_spot(app, &alert, cfg.as_ref(), r).await;
        return;
    }
    let Some(cfg) = cfg else {
        r.outcome = BOT_REQUIRED.to_string();
        return;
    };
    r.outcome = match run_tv(app, ev, &alert, &cfg, r).await {
        Ok(code) | Err(code) => code,
    };
}

/// A spot alert: the Spot bot's paper book, through New trade (spot stays
/// paper in this phase; a Live TradingView bot refuses it).
async fn run_spot(app: &AppHandle, a: &Alert, tv: Option<&BotConfig>, r: &mut AlertResult) -> String {
    if tv.is_some_and(|c| c.mode == TradeMode::Live) {
        return SPOT_PAPER_ONLY.to_string();
    }
    r.bot_name = Some("spot".to_string());
    r.mode = Some("paper".to_string());
    let rows = app.state::<BotManager>().positions_snapshot();
    let held = paper_close_target(&rows, BotKind::Spot, &a.symbol);
    let plan = match message::plan(a, held.as_ref().map(|_| Direction::Long)) {
        Ok(p) => p,
        Err(code) => return code,
    };
    match plan {
        Plan::Close => {
            let id = held.unwrap_or_default();
            r.position_id = Some(id.clone());
            match crate::bot::bot_close_position(app.clone(), id, BotKind::Spot).await {
                Ok(_) => message::CLOSED.to_string(),
                Err(code) => code,
            }
        }
        Plan::Open(direction) => match open_spot(app, a, direction).await {
            Ok(pos) => {
                r.position_id = Some(pos.signal_id.clone());
                (r.filled_qty, r.avg_fill) = (Some(pos.qty), Some(pos.entry));
                r.requested_qty = Some(pos.qty);
                message::OPENED.to_string()
            }
            Err(code) => code,
        },
        // Spot is long-only: a flip is never planned (no short is held).
        Plan::Flip(_) => message::SPOT_LONG_ONLY.to_string(),
    }
}

/// The user trade a spot `close` alert closes: Paper positions only, so a
/// spot alert never sends a real order.
pub(crate) fn paper_close_target(rows: &[OpenPosition], kind: BotKind, symbol: &str) -> Option<String> {
    close_target(rows.iter().filter(|p| !p.live).map(|p| (p.signal_id.as_str(), p.bot_kind, p.symbol.as_str())), kind, symbol)
}

/// New trade on the Spot bot with the alert's levels (percents from the
/// live price now).
async fn open_spot(app: &AppHandle, a: &Alert, direction: Direction) -> Result<OpenPosition, String> {
    let (Some(tp), Some(sl)) = (a.tp, a.sl) else {
        return Err(message::BAD_LEVELS.to_string());
    };
    let price = if needs_price(a) { user_trade::quote(app, BotKind::Spot, &a.symbol).await? } else { 0.0 };
    let (take_profit, stop_loss) = resolve_levels(direction, price, tp, sl);
    let req = TradeRequest {
        kind: BotKind::Spot,
        symbol: a.symbol.clone(),
        direction,
        notional_usdt: a.size_usdt,
        take_profit,
        stop_loss,
        mode: TradeMode::Paper,
        confirmation: None,
        confirmed_margin_usdt: None,
    };
    user_trade::open(app, &req).await
}

/// An alert on the TradingView bot: symbol mapping, the plan against its
/// own position, then close / open / flip, each leg checked.
async fn run_tv(app: &AppHandle, ev: &Event, a: &Alert, cfg: &BotConfig, r: &mut AlertResult) -> Result<String, String> {
    let exchange = app.state::<ExchangeManager>();
    let listed = exchange.symbols(&cfg.exchange_id, true).await;
    let (symbol, factor) = guard::map_symbol(&a.symbol, &listed)?;
    r.symbol = Some(symbol.clone());
    let mut a = a.clone();
    a.symbol = symbol.clone();
    a.price = a.price.map(|p| p * factor);
    a.tp = a.tp.map(|l| scale_level(l, factor));
    a.sl = a.sl.map(|l| scale_level(l, factor));
    r.alert_price = a.price;
    let book = app.state::<BotManager>().positions_snapshot();
    let own = own_position(&book, &symbol).cloned();
    let plan = message::plan(&a, own.as_ref().map(|p| p.direction)).map_err(|code| {
        // Nothing of its own to close: say whose position it is.
        match other_holder(&book, &symbol) {
            Some(k) if code == message::NOTHING_TO_CLOSE => format!("{CLOSE_NOT_OWN}|{}", k.as_str()),
            _ => code,
        }
    })?;
    // A stopped bot takes no new entry; a close still runs (it only reduces).
    let running = app.state::<BotManager>().running_config(BotKind::Tradingview).is_some();
    if let Some(code) = stopped_refusal(plan, running) {
        return Err(code.to_string());
    }
    match plan {
        Plan::Close => {
            let pos = own.ok_or_else(|| message::NOTHING_TO_CLOSE.to_string())?;
            close_own(app, &pos, r).await.map(|_| message::CLOSED.to_string())
        }
        Plan::Open(direction) => {
            open_entry(app, ev, &a, cfg, direction, r).await?;
            Ok(message::OPENED.to_string())
        }
        Plan::Flip(direction) => {
            let pos = own.ok_or_else(|| message::NOTHING_TO_CLOSE.to_string())?;
            // The close leg first; only once the position is gone, the open.
            close_own(app, &pos, r).await.map_err(|code| format!("{FLIP_CLOSE_FAILED}|{code}"))?;
            match open_entry(app, ev, &a, cfg, direction, r).await {
                Ok(()) => Ok(message::FLIPPED.to_string()),
                Err(code) => Err(format!("{FLIP_OPEN_REFUSED}|{code}")),
            }
        }
    }
}

/// Why a plan may not run on a stopped bot (pure): entries and flips need
/// it running; a close never does.
pub fn stopped_refusal(plan: Plan, running: bool) -> Option<&'static str> {
    (!running && !matches!(plan, Plan::Close)).then_some(BOT_STOPPED)
}

/// Closes the TradingView bot's own position (paper booking, or a real
/// reduce-only order sized from the exchange): `Err` while it is still in
/// the book afterwards (a close the exchange did not confirm).
async fn close_own(app: &AppHandle, pos: &OpenPosition, r: &mut AlertResult) -> Result<(), String> {
    r.position_id = Some(pos.signal_id.clone());
    crate::bot::bot_close_position(app.clone(), pos.signal_id.clone(), BotKind::Tradingview).await.map(|_| ())
}

/// The TradingView bot's limits, the price guard, then the entry.
/// A flip runs it after its close leg: the book then is the usage.
async fn open_entry(app: &AppHandle, ev: &Event, a: &Alert, cfg: &BotConfig, direction: Direction, r: &mut AlertResult) -> Result<(), String> {
    let bots = app.state::<BotManager>();
    let tv = cfg.tv.clone().ok_or_else(|| "tvSettingsMissing".to_string())?;
    let (Some(tp), Some(sl)) = (a.tp, a.sl) else {
        return Err(message::BAD_LEVELS.to_string());
    };
    let size = a.size_usdt.unwrap_or(tv.max_size_usdt);
    let leverage = crate::bot::engine::precheck::effective_leverage(cfg, &app.state::<RiskManager>().limits());
    let book = bots.positions_snapshot();
    let mine: Vec<&OpenPosition> = book.iter().filter(|p| p.bot_kind == BotKind::Tradingview).collect();
    // A flip's closed leg has left the book already: the book is the usage.
    let usage = Usage {
        open: mine.len(),
        margin_usdt: mine.iter().map(|p| p.capital).sum::<f64>(),
        opens_today: app.state::<TvManager>().lock().stored.opens_on(now_ms()),
    };
    guard::limit_refusal(cfg, &tv, &a.symbol, size, leverage, usage)?;
    let live_mode = cfg.mode == TradeMode::Live;
    if live_mode {
        // One exchange position per symbol: another bot's (or a DCA / Grid
        // bot's) position there refuses, saying whose.
        if let Some(k) = other_holder(&book, &a.symbol) {
            return Err(format!("{HELD_BY_OTHER}|{}", k.as_str()));
        }
        if crate::bot::strategy_live::owns_symbol(app, &a.symbol) {
            return Err(format!("{HELD_BY_OTHER}|strategy"));
        }
    }
    let exchange = app.state::<ExchangeManager>();
    let live = crate::bot::engine::live_price(&exchange, &cfg.exchange_id, &a.symbol, BotKind::Tradingview)
        .await
        .filter(|p| p.is_finite() && *p > 0.0)
        .ok_or_else(|| "priceUnavailable".to_string())?;
    guard::price_refusal(a.price, live, tv.price_guard_pct, tv.require_price)?;
    // The symbol's smallest order (lot size and MIN_NOTIONAL at the live
    // price), named in the refusal; paper refuses it too, as the exchange
    // could never fill it. Rules unavailable: the order path checks again.
    if cfg.exchange_id == "binance" {
        let rules = crate::bot::strategy::commands::binance_rules_cached(app, &a.symbol).await;
        let min = rules.and_then(|r| tv_trade::symbol_min_usdt(r.lot.min_qty, r.lot.step, r.min_notional, live));
        tv_trade::symbol_min_refusal(&a.symbol, size, tv.max_size_usdt, min)?;
    }
    let (take_profit, stop_loss) = resolve_levels(direction, live, tp, sl);
    let req = TradeRequest {
        kind: BotKind::Tradingview,
        symbol: a.symbol.clone(),
        direction,
        notional_usdt: Some(size),
        take_profit,
        stop_loss,
        mode: cfg.mode,
        confirmation: None,
        confirmed_margin_usdt: None,
    };
    match tv_trade::open_alert(app, &req, &ev.id, (tp, sl)).await {
        Ok(open) => {
            app.state::<TvManager>().lock().stored.count_open(now_ms());
            record_fill(app, r, ev.received_at, a.price, &open.pos, open.requested_qty);
            Ok(())
        }
        Err(refusal) if refusal.outcome_unknown => {
            // Sent, no verdict: never sent again. Looked up by its client id.
            let mgr = app.state::<TvManager>();
            let mut g = mgr.lock();
            g.stored.count_open(now_ms());
            if let Some(planned) = refusal.planned.clone() {
                r.requested_qty = Some(planned.qty);
                r.position_id = Some(planned.signal_id.clone());
                g.stored.pending.push(PendingEntry {
                    event_id: ev.id.clone(),
                    planned,
                    tp,
                    sl,
                    sent_at: now_ms(),
                    received_at: ev.received_at,
                });
            }
            Err(format!("{PENDING}|{}", refusal.code))
        }
        Err(refusal) => Err(refusal.code),
    }
}

/// Requested vs filled, the fill, slippage and latency on the result row.
fn record_fill(app: &AppHandle, r: &mut AlertResult, received_at: Option<u64>, alert_price: Option<f64>, pos: &OpenPosition, requested_qty: f64) {
    r.position_id = Some(pos.signal_id.clone());
    r.requested_qty = Some(requested_qty);
    r.filled_qty = Some(pos.qty);
    r.avg_fill = Some(pos.entry);
    r.slippage_pct = alert_price.and_then(|p| slippage_pct(pos.direction, pos.entry, p));
    let server_now = app.state::<TvManager>().lock().server_now();
    r.latency_ms = fill_latency_ms(received_at, server_now);
}

/// Alert-to-fill latency (pure): from the relay's receive time to the fill,
/// both on the server's clock (its last `now`, advanced by the app's
/// monotonic clock since). None without either time; never negative.
pub fn fill_latency_ms(received_at: Option<u64>, server_now_at_fill: Option<u64>) -> Option<u64> {
    match (received_at, server_now_at_fill) {
        (Some(at), Some(now)) => Some(now.saturating_sub(at)),
        _ => None,
    }
}

/// Looks up every real entry whose outcome stayed unknown and updates its
/// row with the verdict.
async fn resolve_pending_all(app: &AppHandle) {
    let pending: Vec<PendingEntry> = app.state::<TvManager>().lock().stored.pending.clone();
    if pending.is_empty() {
        return;
    }
    for p in pending {
        let age = now_ms().saturating_sub(p.sent_at);
        let verdict = tv_trade::resolve_pending(app, &p.event_id, &p.planned, (p.tp, p.sl), age).await;
        let outcome = match verdict {
            tv_trade::Pending::Filled(fill) => {
                let planned = p.planned.clone();
                let mgr = app.state::<TvManager>();
                let server_now = mgr.lock().server_now();
                let mut g = mgr.lock();
                g.stored.update(&p.event_id, |r| {
                    r.outcome = message::OPENED.to_string();
                    r.filled_qty = Some(fill.executed_qty);
                    r.avg_fill = Some(fill.avg_price);
                    r.slippage_pct = r.alert_price.and_then(|ap| slippage_pct(planned.direction, fill.avg_price, ap));
                    r.latency_ms = fill_latency_ms(p.received_at, server_now);
                });
                Some(message::OPENED.to_string())
            }
            tv_trade::Pending::Failed => Some(crate::bot::engine::live::SKIP_ORDER_FAILED.to_string()),
            tv_trade::Pending::Wait if age >= PENDING_GIVE_UP_MS => Some(crate::bot::engine::live::SKIP_ORDER_UNCONFIRMED.to_string()),
            tv_trade::Pending::Wait => None,
        };
        let Some(outcome) = outcome else { continue };
        let row = {
            let mgr = app.state::<TvManager>();
            let mut g = mgr.lock();
            g.stored.pending.retain(|x| x.event_id != p.event_id);
            if outcome != message::OPENED {
                g.stored.update(&p.event_id, |r| r.outcome = outcome.clone());
            }
            g.stored.results.iter().find(|r| r.event_id == p.event_id).cloned()
        };
        let _ = save(app);
        let _ = app.emit(RESULT_EVENT, &p.event_id);
        if let Some(row) = row {
            notify(app, true, &row).await;
        }
    }
}

/// The server's offline notice is on by default once a Live TradingView
/// bot exists and Telegram is linked, unless the user set it by hand.
async fn ensure_offline_notice(app: &AppHandle) {
    let live_bot = app.state::<BotManager>().config_for(BotKind::Tradingview).is_some_and(|c| c.mode == TradeMode::Live);
    let mgr = app.state::<TvManager>();
    if !live_bot || mgr.lock().stored.notify_touched {
        return;
    }
    let membership = app.state::<MembershipManager>();
    let base = membership.base_url();
    let read = with_session(membership.inner(), |token| {
        let (api, base) = (&mgr.api, base.clone());
        async move { api.settings(&base, &token).await }
    })
    .await;
    let Ok(s) = read else { return };
    if offline_notice_wanted(live_bot, false, &s.telegram, s.notify_offline) {
        let _ = with_session(membership.inner(), |token| {
            let (api, base) = (&mgr.api, base.clone());
            async move { api.set_settings(&base, &token, Some(true), None).await }
        })
        .await;
    }
}

/// Whether the app turns the offline notice on (pure).
pub fn offline_notice_wanted(live_bot: bool, touched: bool, telegram: &str, on: bool) -> bool {
    live_bot && !touched && telegram == "linked" && !on
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one path from an event to `run` (which opens and closes) passes
    /// the event's test flag to `decide`, which only records a test.
    #[test]
    fn the_events_test_flag_reaches_the_decision() {
        let src = include_str!("tasks.rs");
        let body = &src[..src.find("#[cfg(test)]").unwrap()];
        let calls: Vec<_> = body.lines().filter(|l| l.contains("decide(")).collect();
        assert_eq!(calls, vec!["    let decision = decide(&ev.body, ev.test, execute, &clock, stale_window_ms(tv_cfg.as_ref()));"]);
        assert_eq!(body.matches("run(app, &ev, alert").count(), 1, "run is reached only from handle");
        assert_eq!(body.matches("Decision::Execute(").count(), 1, "only handle's match arm");
        // The clock is the server's: the PC clock never feeds the decision.
        assert!(body.contains("server_now_ms: server_now"));
        assert!(!body.contains("server_now_ms: Some(now_ms())"));
    }

    #[test]
    fn the_telegram_lines_name_the_alert_the_bot_and_the_code() {
        let r = AlertResult {
            action: Some("buy".into()),
            symbol: Some("BTCUSDT".into()),
            bot: Some("futures".into()),
            outcome: "tvExpired".into(),
            ..Default::default()
        };
        assert_eq!(refusal_line(&r), "TradingView alert not executed: buy BTCUSDT futures · tvExpired");
        let named = AlertResult { bot_name: Some("Trend".into()), outcome: "tvPriceMoved|0.80%".into(), ..r.clone() };
        assert_eq!(refusal_line(&named), "TradingView alert not executed: buy BTCUSDT futures (Trend) · tvPriceMoved|0.80%");
        let blank = AlertResult { outcome: "tvBadMessage".into(), ..Default::default() };
        assert_eq!(refusal_line(&blank), "TradingView alert not executed: unreadable message · tvBadMessage");
        let open = AlertResult { filled_qty: Some(0.002), avg_fill: Some(65010.5), bot_name: Some("Trend".into()), ..r };
        assert_eq!(live_open_line(&open), "TradingView LIVE open: buy BTCUSDT 0.002 at 65010.5 (Trend)");
    }

    #[test]
    fn slippage_is_signed_against_the_trade() {
        assert!((slippage_pct(Direction::Long, 101.0, 100.0).unwrap() - 1.0).abs() < 1e-9, "a long filled above");
        assert!((slippage_pct(Direction::Short, 101.0, 100.0).unwrap() + 1.0).abs() < 1e-9, "a short filled above: better");
        assert!((slippage_pct(Direction::Short, 99.0, 100.0).unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(slippage_pct(Direction::Long, 101.0, 0.0), None);
    }

    #[test]
    fn latency_is_measured_in_milliseconds_on_the_servers_clock() {
        assert_eq!(fill_latency_ms(Some(1_000), Some(1_850)), Some(850), "under a second stays in ms");
        assert_eq!(fill_latency_ms(Some(1_000), Some(4_200)), Some(3_200));
        assert_eq!(fill_latency_ms(Some(5_000), Some(4_900)), Some(0), "a server clock tick behind is 0, never negative");
        assert_eq!(fill_latency_ms(None, Some(4_200)), None);
        assert_eq!(fill_latency_ms(Some(1_000), None), None);
    }

    // Testnet run, 21 Oct: a stale alert for the TradingView bot showed Bot = "Futures".
    #[test]
    fn a_futures_alert_row_names_the_tradingview_bot_even_when_not_run() {
        let mut cfg = BotConfig::new_default(BotKind::Tradingview, "binance");
        cfg.tv.as_mut().unwrap().name = "Trend".into();
        let mut stale = AlertResult { bot: Some("futures".into()), outcome: "tvExpired".into(), ..Default::default() };
        attribute(&mut stale, Some(&cfg));
        assert_eq!((stale.bot_name.as_deref(), stale.mode.as_deref()), (Some("Trend"), Some("paper")));
        cfg.mode = TradeMode::Live;
        let mut refused = AlertResult { bot: None, outcome: "tvBadMessage".into(), ..Default::default() };
        attribute(&mut refused, Some(&cfg));
        assert_eq!((refused.bot_name.as_deref(), refused.mode.as_deref()), (Some("Trend"), Some("live")));
        // A spot alert stays the Spot bot's; no TradingView bot: nothing named.
        let mut spot = AlertResult { bot: Some("spot".into()), ..Default::default() };
        attribute(&mut spot, Some(&cfg));
        assert_eq!(spot.bot_name, None);
        let mut none = AlertResult { bot: Some("futures".into()), ..Default::default() };
        attribute(&mut none, None);
        assert_eq!(none.bot_name, None);
    }

    #[test]
    fn a_failed_or_idle_stream_says_why_while_the_poll_reads() {
        assert_eq!(stream_failure(&End::Failed("tvHttp|404".into())), Some("tvHttp|404".into()));
        assert_eq!(stream_failure(&End::Idle), Some("tvStreamIdle".into()));
        assert_eq!(stream_failure(&End::Closed), None);
        assert_eq!(stream_failure(&End::Stopped), None);
    }

    #[test]
    fn a_multiplier_symbol_scales_price_levels_only() {
        assert_eq!(scale_level(Level::Price(0.00001), 1000.0), Level::Price(0.01));
        assert_eq!(scale_level(Level::Pct(3.0), 1000.0), Level::Pct(3.0));
    }

    #[test]
    fn a_stopped_bot_still_closes_but_never_opens() {
        use crate::signal::model::Direction;
        assert_eq!(stopped_refusal(Plan::Close, false), None);
        assert_eq!(stopped_refusal(Plan::Open(Direction::Long), false), Some(BOT_STOPPED));
        assert_eq!(stopped_refusal(Plan::Flip(Direction::Short), false), Some(BOT_STOPPED));
        assert_eq!(stopped_refusal(Plan::Open(Direction::Long), true), None);
    }

    #[test]
    fn a_live_bot_polls_every_second_when_the_stream_is_refused() {
        assert_eq!(poll_interval(true), Duration::from_secs(1));
        assert_eq!(poll_interval(false), Duration::from_secs(5));
    }

    #[test]
    fn the_offline_notice_is_turned_on_once_for_a_live_bot_with_telegram() {
        assert!(offline_notice_wanted(true, false, "linked", false));
        assert!(!offline_notice_wanted(true, false, "linked", true), "already on");
        assert!(!offline_notice_wanted(true, true, "linked", false), "the user turned it off by hand");
        assert!(!offline_notice_wanted(true, false, "not_linked", false), "no Telegram to send to");
        assert!(!offline_notice_wanted(false, false, "linked", false), "paper only: not by default");
    }

    #[test]
    fn the_stale_window_is_the_bots_own() {
        let mut cfg = BotConfig::new_default(BotKind::Tradingview, "binance");
        assert_eq!(stale_window_ms(Some(&cfg)), 30_000, "live default 30 s");
        cfg.tv.as_mut().unwrap().stale_s = 90;
        assert_eq!(stale_window_ms(Some(&cfg)), 90_000);
        assert_eq!(stale_window_ms(None), message::DEFAULT_STALE_MS);
    }
}
