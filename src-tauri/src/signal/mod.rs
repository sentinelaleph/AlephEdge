//! Sentinel signal client: connects to the real SSE stream once membership is
//! active, keeps a rolling buffer of recent signals, and reports connection
//! health. The order path is never touched here — this module only observes
//! and records; F3 wires actual trade execution off these signals.

mod backfill;
mod buffer;
pub mod catalog;
mod client;
mod commands;
pub mod model;
mod regime_veto;
mod resolved;
mod stream_state;
pub mod time;
mod veto;

pub use catalog::combo_catalog;
pub(crate) use veto::veto_note_reason;
pub use commands::{signal_connect, signal_disconnect, signal_health, signal_recent};

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use reqwest::Client;
use tauri::{AppHandle, Manager};
use tokio::sync::watch;

use crate::membership::MembershipManager;
use backfill::{PendingPage, Row};
use buffer::SignalBuffer;
use client::{SseEvent, StreamError};
use model::{OpenStatus, Signal, SignalHealthInfo, StreamNoteKind, VetoEvent};
use regime_veto::RegimeVetoStore;
use resolved::PendingSnapshot;
use stream_state::{StreamState, NO_ACCESS_TOKEN};

const RECONNECT_DELAY: Duration = Duration::from_secs(3);

/// Tauri-managed signal client state.
pub struct SignalManager {
    client: Client,
    buffer: Mutex<SignalBuffer>,
    /// The last pending list read: which buffered signals Sentinel has
    /// already resolved (see `resolved`).
    pending: Mutex<PendingSnapshot>,
    /// Signal ids voided by server-side invalidation frames (btc_trend_flip
    /// etc.). FIFO-bounded so a still-relevant void is never wiped en masse.
    invalidated: Mutex<VecDeque<String>>,
    /// Veto notes (symbol + reason) awaiting drain into the skip feed.
    pending_vetoes: Mutex<VecDeque<VetoEvent>>,
    app: OnceLock<AppHandle>,
    regime_vetoes: RegimeVetoStore,
    /// Connection phase + last real failure (ticket/stream), surfaced to the
    /// UI so a stuck "Disconnected" is diagnosable without progress text ever
    /// posing as an error (see `stream_state`).
    stream: Mutex<StreamState>,
    /// DESIRED connected state (level-triggered) the supervisor reacts to, so a
    /// fast connect→disconnect→connect never "loses" the final connect.
    want: watch::Sender<bool>,
    /// The supervisor task is spawned exactly once, for the app's lifetime.
    supervisor: AtomicBool,
    last_event_at_ms: AtomicU64,
    last_latency_ms: AtomicU64,
}

impl SignalManager {
    pub fn new() -> Self {
        let (want, _) = watch::channel(false);
        Self {
            client: Client::new(),
            buffer: Mutex::new(SignalBuffer::default()),
            pending: Mutex::new(PendingSnapshot::default()),
            invalidated: Mutex::new(VecDeque::new()),
            pending_vetoes: Mutex::new(VecDeque::new()),
            app: OnceLock::new(),
            regime_vetoes: RegimeVetoStore::default(),
            stream: Mutex::new(StreamState::default()),
            want,
            supervisor: AtomicBool::new(false),
            last_event_at_ms: AtomicU64::new(0),
            last_latency_ms: AtomicU64::new(0),
        }
    }

    /// Records the desired stream state. `send_replace`, never `send`: a
    /// watch `send` with no live receiver returns Err WITHOUT storing the
    /// value, and the supervisor subscribes only after the first connect —
    /// so the app's first connect was dropped and the stream stayed idle
    /// until something called connect a second time.
    fn set_wanted(&self, on: bool) {
        self.want.send_replace(on);
    }

    /// Requests the signal stream. Level-triggered: sets the desired state and
    /// spawns the supervisor once. The supervisor (re)establishes the stream
    /// whenever the desired state is true and tears it down when false, so
    /// rapid connect/disconnect/connect (e.g. React StrictMode remount) can't
    /// strand a dropped connect.
    pub fn connect(&self, app: AppHandle) {
        let _ = self.app.set(app.clone());
        self.set_wanted(true);
        if self.supervisor.swap(true, Ordering::SeqCst) {
            return; // supervisor already running; it will observe want=true
        }
        let mut want = self.want.subscribe();
        tauri::async_runtime::spawn(async move {
            loop {
                let signal = app.state::<SignalManager>();
                // Idle until a connection is desired.
                while !*want.borrow() {
                    signal.with_stream(|s| s.idle());
                    if want.changed().await.is_err() {
                        return;
                    }
                }
                let membership = app.state::<MembershipManager>();
                // Refreshed ahead of expiry: the ticket request must not
                // leave with a token that is about to die.
                let Some(token) = membership.fresh_access_token().await else {
                    // No token reaching the stream client — surfaced so this
                    // (vs a request failure) is distinguishable in the UI.
                    signal.with_stream(|s| s.blocked(NO_ACCESS_TOKEN, now_millis()));
                    let _ = tokio::time::timeout(RECONNECT_DELAY, want.changed()).await;
                    continue;
                };
                signal.with_stream(|s| s.attempt(now_millis()));
                let base = membership.base_url();

                // One session, aborted the instant the desired state flips
                // false — this cancels a slow ticket request too, not just the
                // stream, so teardown is prompt.
                let outcome = tokio::select! {
                    r = signal.run_once(&base, &token) => r,
                    _ = wait_until_false(&mut want) => Ok(()),
                };
                signal.with_stream(|s| s.ended(now_millis()));

                // A 401 means the access token expired mid-session — refresh it
                // via the membership manager so a long-lived session's stream
                // self-heals on the next retry without forcing a re-login.
                // Decided on the error's TYPE: it used to match "401" in the
                // English error text, which the stable codes replaced.
                if needs_token_refresh(&outcome) {
                    // Single-flight and rotation-aware (see membership): the
                    // UI poll may be refreshing at the same moment.
                    let _ = membership.refresh_after_unauthorized(&token).await;
                }

                if *want.borrow() {
                    let _ = tokio::time::timeout(RECONNECT_DELAY, want.changed()).await;
                }
            }
        });
    }

    /// Requests teardown of the signal stream. Safe to call any time.
    pub fn disconnect(&self) {
        self.set_wanted(false);
        self.with_stream(|s| s.idle());
    }

    /// One session: ticket, backfill, then the stream until it ends. The
    /// failure (if any) is both recorded for the UI and returned, typed, to
    /// the supervisor.
    async fn run_once(&self, base: &str, token: &str) -> Result<(), StreamError> {
        let (ticket, latency_ms) = match client::request_ticket(&self.client, base, token).await {
            Ok(t) => t,
            Err(e) => {
                // No logging framework here — store the reason so the UI can show
                // it (signal_health), making a stuck "Disconnected" diagnosable.
                self.with_stream(|s| s.failed(e.code(), now_millis()));
                return Err(e);
            }
        };
        self.last_latency_ms
            .store(latency_ms as u64, Ordering::Relaxed);
        self.with_stream(|s| s.live(now_millis()));
        // Backfill on every (re)connect, before the stream, so signals
        // published while disconnected still reach the engine. A failed
        // backfill is surfaced but does not block the live stream.
        let asof = now_millis();
        match backfill::fetch_pending(&self.client, base).await {
            Ok(page) => self.apply_pending(page, asof),
            Err(e) => self.queue_note(String::new(), "—".into(), e, StreamNoteKind::ParseError),
        }
        // Vetoes issued while disconnected never arrive as SSE frames: learn
        // them here so entry is blocked and the tick closes any open position.
        match backfill::fetch_vetoed(&self.client, base).await {
            Ok(events) => {
                self.record_backfilled_vetoes(events);
            }
            Err(e) => self.queue_note(String::new(), "—".into(), e, StreamNoteKind::ParseError),
        }
        // The stream never says a signal resolved; re-reading the pending
        // list does. It runs for as long as the stream does.
        tokio::select! {
            r = self.stream(base, &ticket) => r,
            _ = self.sweep_resolved(base) => Ok(()),
        }
    }

    /// Re-reads the pending list every `resolved::SWEEP_EVERY`. A failed read
    /// changes nothing; the snapshot goes stale and entries needing it are
    /// refused by strict bots (`OpenStatus::Unknown`).
    async fn sweep_resolved(&self, base: &str) {
        loop {
            tokio::time::sleep(resolved::SWEEP_EVERY).await;
            let asof = now_millis();
            if let Ok(page) = backfill::fetch_pending(&self.client, base).await {
                self.apply_pending(page, asof);
            }
        }
    }

    /// Buffers the page's signals, then drops buffered signals the page
    /// covers but no longer lists: Sentinel has recorded their outcome.
    fn apply_pending(&self, page: PendingPage, asof_ms: u64) {
        let returned = page.rows.len();
        let complete = page.total.is_some_and(|t| t as usize <= returned);
        let mut ids = HashSet::with_capacity(returned);
        let mut oldest: Option<u64> = None;
        for row in &page.rows {
            match row {
                Row::Ok(sig) => {
                    ids.insert(sig.id.clone());
                    if let Some(c) = time::parse_rfc3339_ms(&sig.created_at) {
                        oldest = Some(oldest.map_or(c, |o| o.min(c)));
                    }
                }
                Row::Bad { id, .. } => {
                    ids.insert(id.clone());
                }
            }
        }
        // Publication order (oldest first): the list arrives newest first.
        let mut rows = page.rows;
        backfill::chronological(&mut rows);
        rows.into_iter().for_each(|row| self.ingest(row));
        let buffered = self.buffer.lock().expect("signal mutex").id_created();
        let gone = self
            .pending
            .lock()
            .expect("signal mutex")
            .update(ids, oldest, complete, asof_ms, &buffered);
        if gone.is_empty() {
            return;
        }
        let removed = self.buffer.lock().expect("signal mutex").remove_ids(&gone);
        for sig in removed {
            self.queue_note(sig.id, sig.symbol, String::new(), StreamNoteKind::Resolved);
        }
    }

    /// Whether Sentinel still lists `sig` as open (see `resolved`).
    pub fn open_status(&self, sig: &Signal, now_ms: u64) -> OpenStatus {
        let created = time::parse_rfc3339_ms(&sig.created_at).unwrap_or(0);
        self.pending
            .lock()
            .expect("signal mutex")
            .status(&sig.id, created, now_ms)
    }

    fn with_stream(&self, f: impl FnOnce(&mut StreamState)) {
        f(&mut self.stream.lock().expect("signal mutex"));
    }

    async fn stream(&self, base: &str, ticket: &str) -> Result<(), StreamError> {
        let result = client::run_stream(&self.client, base, ticket, |event| match event {
            SseEvent::Signal(data) => match serde_json::from_str::<serde_json::Value>(&data) {
                Ok(value) => self.ingest(backfill::parse_row(value)),
                Err(e) => self.ingest(Row::Bad {
                    id: String::new(),
                    symbol: "—".into(),
                    error: e.to_string(),
                }),
            },
            SseEvent::Heartbeat => self.touch(),
            SseEvent::Invalidation(data) => self.mark_invalidated(&data),
            SseEvent::Veto(data) => self.mark_regime_veto(&data),
            SseEvent::Other => {}
        })
        .await;
        if let Err(e) = &result {
            self.with_stream(|s| s.failed(e.code(), now_millis()));
        }
        result
    }

    /// Buffers a parsed signal, or records a visible note for a row that
    /// failed to parse (it used to be dropped silently). Expired backfill rows
    /// are ignored; regeneration supersedes are surfaced as notes.
    fn ingest(&self, row: Row) {
        let sig = match row {
            Row::Ok(sig) => *sig,
            Row::Bad { id, symbol, error } => {
                self.queue_note(id, symbol, error, StreamNoteKind::ParseError);
                return;
            }
        };
        let now = now_millis();
        if time::parse_rfc3339_ms(&sig.expires_at).is_some_and(|exp| exp <= now) {
            return;
        }
        let outcome = self.buffer.lock().expect("signal mutex").push(sig, now);
        for old in outcome.superseded {
            self.queue_note(
                old.id,
                old.symbol,
                String::new(),
                StreamNoteKind::Superseded,
            );
        }
        self.touch();
    }

    fn queue_note(&self, signal_id: String, symbol: String, reason: String, kind: StreamNoteKind) {
        self.queue_veto(VetoEvent {
            signal_id,
            symbol,
            reason,
            kind,
        });
    }

    fn touch(&self) {
        self.last_event_at_ms.store(now_millis(), Ordering::Relaxed);
    }

    /// Newest-first recent signals.
    pub fn recent(&self) -> Vec<Signal> {
        self.buffer.lock().expect("signal mutex").recent()
    }

    pub fn health(&self) -> SignalHealthInfo {
        let last = self.last_event_at_ms.load(Ordering::Relaxed);
        let last_signal_secs = if last == 0 {
            None
        } else {
            Some(((now_millis().saturating_sub(last)) / 1000) as u32)
        };
        let latency = self.last_latency_ms.load(Ordering::Relaxed);
        let stream = self.stream.lock().expect("signal mutex").clone();
        SignalHealthInfo {
            connected: stream.is_live(),
            phase: stream.phase(),
            not_live_secs: stream.not_live_secs(now_millis()),
            last_signal_secs,
            latency_ms: if latency == 0 {
                None
            } else {
                Some(latency as u32)
            },
            last_error: stream.last_error().map(str::to_string),
        }
    }
}

impl Default for SignalManager {
    fn default() -> Self {
        Self::new()
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Whether a session's outcome calls for a token refresh before the retry.
fn needs_token_refresh(outcome: &Result<(), StreamError>) -> bool {
    matches!(outcome, Err(e) if e.is_unauthorized())
}

/// Resolves once the desired-connected state becomes false. Used to abort a
/// live session promptly on `disconnect()`. The `borrow()` ref is dropped
/// before each `.await`, so no watch lock is held across a suspend point.
async fn wait_until_false(want: &mut watch::Receiver<bool>) {
    while *want.borrow() {
        if want.changed().await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod live_fixture_tests;
#[cfg(test)]
mod model_tests;

#[cfg(test)]
mod want_tests {
    use super::{needs_token_refresh, SignalManager, StreamError};

    /// The 401 refresh rode on `last_error().contains("401")`. With stable
    /// codes the text is `signalTicketRefused|401` today and anything
    /// tomorrow; the decision now reads the typed error.
    #[test]
    fn a_401_from_ticket_or_stream_refreshes_the_token() {
        assert!(needs_token_refresh(&Err(StreamError::TicketStatus(401))));
        assert!(needs_token_refresh(&Err(StreamError::StreamStatus(401))));
        assert!(!needs_token_refresh(&Err(StreamError::TicketStatus(403))));
        assert!(!needs_token_refresh(&Err(StreamError::StreamIdle)));
        assert!(!needs_token_refresh(&Ok(())));
    }

    // Regression: the first connect happens before the supervisor
    // subscribes. It must still be recorded, or the stream never starts.
    #[test]
    fn the_first_connect_is_kept_with_no_subscriber_yet() {
        let m = SignalManager::new();
        m.set_wanted(true);
        assert!(*m.want.subscribe().borrow(), "first connect was dropped");
        m.set_wanted(false);
        assert!(!*m.want.subscribe().borrow());
    }
}
