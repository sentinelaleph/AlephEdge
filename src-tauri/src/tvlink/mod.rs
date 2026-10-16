//! TradingView alerts, through Sentinel's relay.
//!
//! TradingView posts an alert's message to a per-user secret URL on
//! ribqa.com; the server keeps it (7 days). While the user is signed in and
//! has "Receive alerts" or "Execute alerts" on, this module reads new events
//! every few seconds with the app's own session (`api::with_session`), and
//! for each one:
//!
//! - reads the message (`message::parse`; format in `message.rs`),
//! - with "Execute alerts" off records it as `received` and does nothing,
//! - refuses anything received more than 120 s earlier (`tvExpired`),
//! - opens a paper position through New trade (`bot::engine::user_trade`),
//!   or closes the app's own user trade on that symbol, and records the
//!   outcome (`opened`, `closed`, or the refusal code).
//!
//! The webhook URL is shown once (`tv_create_url`) and never stored or
//! logged here; the server keeps only its hash. The cursor (last event id)
//! and the last 100 results live in the store's `meta` table, so a restart
//! reads nothing twice. Paper only in this version.

pub mod api;
pub mod commands;
pub mod message;
mod tasks;

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
    /// `received`, `opened`, `closed`, or a refusal code (`key|detail`).
    pub outcome: String,
    /// The paper position opened or closed (`manual-...`).
    pub position_id: Option<String>,
}

/// The user's two switches. Execute implies receive.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    pub receive: bool,
    pub execute: bool,
}

/// Everything kept across restarts. No URL, no token.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Stored {
    pub receive: bool,
    pub execute: bool,
    /// The last event id read; the next read asks for events after it.
    pub cursor: Option<String>,
    /// Newest first, at most `MAX_RESULTS`.
    pub results: Vec<AlertResult>,
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
    /// Last successful read (UNIX ms).
    pub last_poll_at: Option<u64>,
}

pub struct TvManager {
    pub(crate) inner: Mutex<Inner>,
    pub(crate) api: api::Api,
}

impl TvManager {
    pub fn new() -> Self {
        Self { inner: Mutex::new(Inner::default()), api: api::Api::new() }
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
}
