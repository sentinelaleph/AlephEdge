//! TradingView alerts, through Sentinel's relay, executed by the
//! TradingView bot (`BotKind::Tradingview`, Paper or Live).
//!
//! TradingView posts an alert's message to a per-user secret URL on
//! ribqa.com; the server keeps it (7 days). While the user is signed in and
//! has "Receive alerts" or "Execute alerts" on, this module reads new events
//! with the app's own session (`api::with_session`): over the relay's event
//! stream (SSE, `sse.rs`), or by polling (1 s while a Live TradingView bot
//! runs, 5 s otherwise) when the stream is refused. For each event:
//!
//! - the cursor moves first (at most once: a crash never runs an alert
//!   twice),
//! - reads the message (`message::parse`; format in `message.rs`),
//! - a test alert is recorded as `tvTestOnly` and nothing else, paper or
//!   live,
//! - with "Execute alerts" off records it as `received`,
//! - an alert the relay received while the app was offline is
//!   `tvExpiredOffline`; one older than the bot's stale window on the
//!   SERVER's clock is `tvExpired`,
//! - the passphrase, the duplicate guard (same body or `id` within 5 min),
//!   the bot's own limits, the price-move guard (`guard.rs`), then the
//!   plan (open, close, or a strategy flip) against the bot's own position,
//! - runs it on the TradingView bot (`bot::engine::tv_trade`), or a spot
//!   alert on the Spot bot's paper book, and records the outcome with the
//!   requested and filled quantity, the fill, the slippage against the
//!   alert's price and the alert-to-fill latency.
//!
//! A real entry whose outcome stayed unknown is never sent again: its row
//! waits as `livePending` and its client id (derived from the event id) is
//! looked up until Binance answers (`tasks::resolve_pending`).
//!
//! The webhook URL is shown once (`tv_create_url`) and never stored or
//! logged here; the server keeps only its hash. Neither the passphrase nor
//! an alert body is stored (the duplicate guard keeps a hash). The cursor,
//! the results and the guards' state live in the store's `meta` table.

pub mod api;
pub mod commands;
pub mod guard;
pub mod message;
pub mod sse;
pub(crate) mod tasks;

use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::store::StoreManager;

pub use tasks::spawn;

/// `meta` key of the stored state.
pub const STATE_KEY: &str = "tvlink.state";
/// Results kept.
pub const MAX_RESULTS: usize = 100;

/// One handled alert, as the results table shows it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AlertResult {
    pub event_id: String,
    /// Server receive time (UNIX ms), when the server sent one.
    pub received_at: Option<u64>,
    /// When the app handled it (UNIX ms).
    pub handled_at: u64,
    /// `buy` / `sell` / `close`, when readable.
    pub action: Option<String>,
    pub symbol: Option<String>,
    /// `futures` / `spot`, when readable.
    pub bot: Option<String>,
    /// `received`, `opened`, `closed`, `flipped`, `livePending`, or a
    /// refusal code (`key|detail`).
    pub outcome: String,
    /// The position opened or closed (`manual-...`).
    pub position_id: Option<String>,
    /// A test alert (never executed).
    pub test: bool,
    /// The bot that handled it (its name), and how: `paper` / `live`.
    pub bot_name: Option<String>,
    pub mode: Option<String>,
    /// Quantity the plan sized at the live price, and what filled.
    pub requested_qty: Option<f64>,
    pub filled_qty: Option<f64>,
    pub avg_fill: Option<f64>,
    /// The alert's own `price` (`{{close}}`).
    pub alert_price: Option<f64>,
    /// (fill − alert price) / alert price, in %, signed against the trade
    /// (positive = worse for it).
    pub slippage_pct: Option<f64>,
    /// From the relay's receive to the fill, on the server's clock.
    pub latency_ms: Option<u64>,
    /// The strategy order (`buy long`, `sell flat`) for strategy alerts.
    pub strategy: Option<String>,
}

/// The user's two switches. Execute implies receive.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    pub receive: bool,
    pub execute: bool,
}

/// A real alert entry whose outcome stayed unknown: what was sent, for the
/// lookup by its client id (`tv_trade::alert_client_id(event_id)`).
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PendingEntry {
    pub event_id: String,
    pub planned: crate::bot::model::OpenPosition,
    pub tp: crate::bot::engine::user_trade::Level,
    pub sl: crate::bot::engine::user_trade::Level,
    /// When it was sent (app clock: only the lookup's own age).
    pub sent_at: u64,
    pub received_at: Option<u64>,
}

/// Everything kept across restarts. No URL, no token, no passphrase, no
/// alert body.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Stored {
    pub receive: bool,
    pub execute: bool,
    /// The last event id read; the next read asks for events after it.
    pub cursor: Option<String>,
    /// Newest first, at most `MAX_RESULTS`.
    pub results: Vec<AlertResult>,
    /// The duplicate guard's last 5 minutes (hashes and ids only).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub seen: Vec<guard::SeenAlert>,
    /// Alert entries per UTC day: (day start ms, count).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opens: Option<(u64, u32)>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pending: Vec<PendingEntry>,
    /// The user set the offline notice by hand: the app no longer turns it
    /// on for them.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub notify_touched: bool,
}

impl PartialEq for Stored {
    fn eq(&self, o: &Self) -> bool {
        (self.receive, self.execute, &self.cursor, &self.results, &self.seen, self.opens, self.notify_touched)
            == (o.receive, o.execute, &o.cursor, &o.results, &o.seen, o.opens, o.notify_touched)
            && self.pending.iter().map(|p| &p.event_id).eq(o.pending.iter().map(|p| &p.event_id))
    }
}

impl Stored {
    pub fn prefs(&self) -> Prefs {
        Prefs { receive: self.receive, execute: self.execute }
    }

    pub fn set_prefs(&mut self, p: Prefs) {
        self.execute = p.execute;
        self.receive = p.receive || p.execute;
    }

    /// Polling runs only while one of the switches is on.
    pub fn active(&self) -> bool {
        self.receive || self.execute
    }

    /// Already read: at or before a numeric cursor, or in the results.
    pub fn seen(&self, id: &str) -> bool {
        let numeric = |s: &str| s.parse::<u64>().ok();
        if let (Some(c), Some(i)) = (self.cursor.as_deref().and_then(numeric), numeric(id)) {
            if i <= c {
                return true;
            }
        }
        self.results.iter().any(|r| r.event_id == id)
    }

    pub fn record(&mut self, r: AlertResult) {
        self.results.insert(0, r);
        self.results.truncate(MAX_RESULTS);
    }

    /// Replaces the row of `event_id` (a late verdict on a pending entry).
    pub fn update(&mut self, event_id: &str, f: impl FnOnce(&mut AlertResult)) {
        if let Some(r) = self.results.iter_mut().find(|r| r.event_id == event_id) {
            f(r);
        }
    }

    /// Alert entries made on the UTC day of `now_ms`.
    pub fn opens_on(&self, now_ms: u64) -> u32 {
        match self.opens {
            Some((day, n)) if day == guard::utc_day(now_ms) => n,
            _ => 0,
        }
    }

    pub fn count_open(&mut self, now_ms: u64) {
        let day = guard::utc_day(now_ms);
        let n = self.opens_on(now_ms);
        self.opens = Some((day, n + 1));
    }
}

pub fn load_stored(store: &StoreManager, dir: &Path) -> Stored {
    store
        .get_meta(dir, STATE_KEY)
        .ok()
        .flatten()
        .and_then(|body| serde_json::from_str(&body).ok())
        .unwrap_or_default()
}

pub fn save_stored(store: &StoreManager, dir: &Path, s: &Stored) -> Result<(), String> {
    let body = serde_json::to_string(s).map_err(|_| "storeWriteFailed".to_string())?;
    store.set_meta(dir, STATE_KEY, &body)
}

#[derive(Default)]
pub(crate) struct Inner {
    pub stored: Stored,
    /// Last read failure as a code; cleared by the next good read.
    pub last_error: Option<String>,
    /// Last successful read or stream frame (UNIX ms, app clock: display).
    pub last_poll_at: Option<u64>,
    /// `stream` while the event stream is open, `poll` otherwise.
    pub transport: Option<&'static str>,
    /// Why the last stream session failed (shown while the poll reads).
    pub stream_error: Option<String>,
    /// The server's `now` of the last reply and when it arrived: the stale
    /// guard's clock (`server_now`).
    pub server_clock: Option<(u64, std::time::Instant)>,
}

impl Inner {
    /// The server's time now: its last `now`, advanced by the time since.
    pub fn server_now(&self) -> Option<u64> {
        self.server_clock.map(|(now, at)| now + at.elapsed().as_millis() as u64)
    }
}

pub struct TvManager {
    pub(crate) inner: Mutex<Inner>,
    pub(crate) api: api::Api,
    /// One read pass at a time: the background loop and "Send test alert".
    pub(crate) pass_lock: tokio::sync::Mutex<()>,
}

impl TvManager {
    pub fn new() -> Self {
        Self { inner: Mutex::new(Inner::default()), api: api::Api::new(), pass_lock: tokio::sync::Mutex::new(()) }
    }

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl Default for TvManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("aleph-tvlink-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn result(id: &str) -> AlertResult {
        AlertResult { event_id: id.into(), outcome: "received".into(), ..Default::default() }
    }

    #[test]
    fn execute_implies_receive() {
        let mut s = Stored::default();
        assert!(!s.active());
        s.set_prefs(Prefs { receive: false, execute: true });
        assert_eq!(s.prefs(), Prefs { receive: true, execute: true });
        s.set_prefs(Prefs { receive: true, execute: false });
        assert!(s.active() && !s.execute);
        s.set_prefs(Prefs::default());
        assert!(!s.active());
    }

    #[test]
    fn the_cursor_and_results_survive_a_restart() {
        let dir = temp_dir();
        let store = StoreManager::new();
        assert_eq!(load_stored(&store, &dir), Stored::default(), "nothing stored yet");
        let mut s = Stored { receive: true, execute: true, cursor: Some("41".into()), ..Default::default() };
        s.record(result("41"));
        save_stored(&store, &dir, &s).unwrap();
        // A new process: a fresh store on the same data folder.
        let reopened = StoreManager::new();
        let back = load_stored(&reopened, &dir);
        assert_eq!(back, s);
        assert!(back.seen("41") && back.seen("12"), "at or before the cursor");
        assert!(!back.seen("42"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_numeric_ids_are_matched_by_the_results() {
        let mut s = Stored { cursor: Some("evt_b".into()), ..Default::default() };
        s.record(result("evt_a"));
        assert!(s.seen("evt_a"));
        assert!(!s.seen("evt_c"));
    }

    #[test]
    fn only_the_last_hundred_results_are_kept_newest_first() {
        let mut s = Stored::default();
        for i in 0..130 {
            s.record(result(&i.to_string()));
        }
        assert_eq!(s.results.len(), MAX_RESULTS);
        assert_eq!(s.results[0].event_id, "129");
        assert_eq!(s.results[MAX_RESULTS - 1].event_id, "30");
    }

    #[test]
    fn stored_state_has_no_url_or_token_field() {
        let json = serde_json::to_string(&Stored::default()).unwrap();
        assert_eq!(json, r#"{"receive":false,"execute":false,"cursor":null,"results":[]}"#);
        let partial: Stored = serde_json::from_str(r#"{"execute":true}"#).unwrap();
        assert!(partial.execute && partial.results.is_empty());
    }

    #[test]
    fn daily_opens_count_per_utc_day() {
        let day = 86_400_000u64 * 20_000;
        let mut s = Stored::default();
        assert_eq!(s.opens_on(day + 5), 0);
        s.count_open(day + 5);
        s.count_open(day + 6);
        assert_eq!(s.opens_on(day + 86_399_999), 2);
        assert_eq!(s.opens_on(day + 86_400_000), 0, "a new UTC day starts at zero");
        s.count_open(day + 86_400_001);
        assert_eq!(s.opens, Some((day + 86_400_000, 1)));
    }

    #[test]
    fn a_late_verdict_updates_its_row() {
        let mut s = Stored::default();
        s.record(AlertResult { event_id: "7".into(), outcome: "livePending".into(), ..Default::default() });
        s.update("7", |r| r.outcome = "opened".into());
        assert_eq!(s.results[0].outcome, "opened");
        s.update("8", |r| r.outcome = "x".into());
        assert_eq!(s.results.len(), 1);
    }
}
