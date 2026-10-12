//! Tauri commands for pairing and the live relay session.
//!
//! The desktop shows a QR and the phone reads it. The QR carries the key, so
//! it is a standing credential (see `aleph_desk_link::pairing`): the session
//! is short-lived and the UI must show when it expires.
//!
//! # Host and identity are resolved here, never taken from the UI
//!
//! `desk_id` and `relay_url` come from `app::endpoints`, not from command
//! parameters, so the security boundary stays out of the webview.
//!
//! # Why the session starts with the QR
//!
//! The relay accepts a phone only while the desk is already connected (see
//! `aleph_desk_link::pairing`, "Relay contract"). So `link_begin_pairing`
//! both produces the QR and starts the relay session.

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use aleph_desk_link::pairing::{PairingSession, PAIRING_TTL_MS};
use aleph_desk_link::DeskLink;
use aleph_link::PairingKey;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex as AsyncMutex;

use super::session::{self, LinkState, SharedLink};
use super::DeskBridge;
use crate::app::endpoints;
use crate::membership::MembershipManager;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The open pairing session (at most one at a time).
#[derive(Default)]
pub struct PairingState {
    pub(crate) session: Mutex<Option<PairingSession>>,
}

impl PairingState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Everything the UI sees.
///
/// The desk id is **not** here: the UI neither uses it nor needs to send it,
/// and carrying a routing address into the webview for nothing (and from there
/// into logs and error reports) is not free.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingView {
    /// The text printed into the QR. `None` once it has expired.
    pub qr: Option<String>,
    /// Countdown: the user must see when the QR dies.
    pub remaining_ms: u64,
    pub ttl_ms: u64,
    /// Why, if it expired or was used.
    pub expired_reason: Option<String>,
}

fn view(s: &PairingSession, now: u64) -> PairingView {
    match s.qr_payload(now) {
        Ok(qr) => PairingView {
            qr: Some(qr),
            remaining_ms: s.remaining_ms(now),
            ttl_ms: PAIRING_TTL_MS,
            expired_reason: None,
        },
        Err(e) => PairingView {
            qr: None,
            remaining_ms: 0,
            ttl_ms: PAIRING_TTL_MS,
            expired_reason: Some(e.to_string()),
        },
    }
}

/// Opens a new pairing session, starts the relay session and returns the QR
/// text.
///
/// It replaces any existing session: with two valid QRs on screen nobody
/// could tell which one is live. The running relay session is **restarted**
/// with the new key too; staying connected with the old key would leave the
/// phone that reads the new QR without an answer.
#[tauri::command]
pub fn link_begin_pairing(
    app: AppHandle,
    state: State<PairingState>,
    runtime: State<LinkRuntime>,
) -> Result<PairingView, String> {
    // The relay authenticates both ends with Sentinel membership. A QR made
    // while signed out would be a code that can never connect: the panel's
    // honesty rule, applied one step earlier.
    if app.state::<MembershipManager>().access_token().is_none() {
        return Err("linkSignInFirst".into());
    }

    // Host and identity: from one place, not from the UI.
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|_| "appDataDirUnavailable".to_string())?;
    let desk_id = endpoints::desk_id(&dir);
    let relay_url = endpoints::relay_url();

    let now = now_ms();
    let s = PairingSession::begin(desk_id.clone(), relay_url.clone(), now);
    let qr = s.qr_payload(now).map_err(|e| e.to_string())?;
    let remaining = s.remaining_ms(now);
    let key = s.desk_session_key();
    *state.session.lock().map_err(|_| "pairing mutex")? = Some(s);

    // The QR's death is the key's deadline too: if no phone has used the key
    // by then, the desk stops accepting envelopes signed with it.
    runtime.restart(&app, relay_url, desk_id, key, now + PAIRING_TTL_MS)?;

    Ok(PairingView {
        qr: Some(qr),
        remaining_ms: remaining,
        ttl_ms: PAIRING_TTL_MS,
        expired_reason: None,
    })
}

/// The open session's current state.
///
/// An expired QR **returns no text** and says why (see
/// `PairingSession::qr_payload`).
#[tauri::command]
pub fn link_pairing_status(state: State<PairingState>) -> Result<Option<PairingView>, String> {
    let guard = state.session.lock().map_err(|_| "pairing mutex")?;
    let Some(s) = guard.as_ref() else {
        return Ok(None);
    };
    Ok(Some(view(s, now_ms())))
}

/// Cancels the open session **and stops the relay session**.
///
/// Without the second part, "cancel" would take the QR off the screen while
/// the desk stayed connected to the relay. The button is shown only while the
/// QR is live, so no phone has talked with that key yet (see
/// `session::spend_pairing`): it never cuts a working connection.
#[tauri::command]
pub async fn link_cancel_pairing(
    state: State<'_, PairingState>,
    runtime: State<'_, LinkRuntime>,
) -> Result<(), String> {
    {
        let mut guard = state.session.lock().map_err(|_| "pairing mutex")?;
        *guard = None;
    }
    runtime.stop().await
}

// ---------------------------------------------------------------------------
// Live session
// ---------------------------------------------------------------------------

/// Shared state of the running relay session.
pub struct LinkRuntime {
    state: Arc<AsyncMutex<LinkState>>,
    /// Handle of the running session, kept for re-pairing: a new QR means a
    /// new key, and a session still connected with the old key can never
    /// answer the new phone.
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    /// The running session's link layer. Cancelling does not stop at aborting
    /// the task: it also revokes the key, so that no envelope signed with it
    /// is applied even if the abort lands late.
    link: Mutex<Option<SharedLink>>,
}

impl Default for LinkRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl LinkRuntime {
    pub fn new() -> Self {
        Self {
            state: Arc::new(AsyncMutex::new(LinkState::Unpaired)),
            task: Mutex::new(None),
            link: Mutex::new(None),
        }
    }

    /// Starts the session, stopping the previous one if any.
    /// `claim_deadline_ms`: the last moment a phone may first use the key (the
    /// QR's death).
    fn restart(
        &self,
        app: &AppHandle,
        relay_url: String,
        desk_id: String,
        key: PairingKey,
        claim_deadline_ms: u64,
    ) -> Result<(), String> {
        let mut task = self.task.lock().map_err(|_| "link mutex")?;
        if let Some(h) = task.take() {
            h.abort();
        }
        let mut slot = self.link.lock().map_err(|_| "link mutex")?;
        if let Some(old) = slot.take() {
            revoke_later(old);
        }
        // Session stamp: the moment this session started (see
        // `aleph_link::ReplayGuard`).
        let link: SharedLink = Arc::new(AsyncMutex::new(
            DeskLink::new(desk_id.clone(), key, DeskBridge::new(app.clone()), now_ms())
                .with_claim_deadline(claim_deadline_ms),
        ));
        *slot = Some(link.clone());
        *task = Some(tauri::async_runtime::spawn(session::run(
            app.clone(),
            relay_url,
            desk_id,
            link,
            self.state.clone(),
        )));
        Ok(())
    }

    async fn stop(&self) -> Result<(), String> {
        {
            let mut task = self.task.lock().map_err(|_| "link mutex")?;
            if let Some(h) = task.take() {
                h.abort();
            }
        }
        // The task abort takes effect at its next await point; the key dies
        // here regardless.
        let old = self.link.lock().map_err(|_| "link mutex")?.take();
        if let Some(old) = old {
            old.lock().await.revoke();
        }
        // The state is written AFTER the abort; otherwise the dropped
        // session's last "Retrying" could stay on screen.
        *self.state.lock().await = LinkState::Unpaired;
        Ok(())
    }
}

/// Revokes an old session's key once its lock is free. The synchronous
/// `restart` cannot wait for the lock; the key dies the moment the aborted
/// task releases it.
fn revoke_later(link: SharedLink) {
    tauri::async_runtime::spawn(async move {
        link.lock().await.revoke();
    });
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum LinkStateView {
    Unpaired,
    Connecting,
    Connected,
    Retrying { in_secs: u64 },
    Stopped { message: String },
}

impl From<LinkState> for LinkStateView {
    fn from(s: LinkState) -> Self {
        match s {
            LinkState::Unpaired => Self::Unpaired,
            LinkState::Connecting => Self::Connecting,
            LinkState::Connected => Self::Connected,
            LinkState::Retrying { in_secs } => Self::Retrying { in_secs },
            LinkState::Stopped { message } => Self::Stopped { message },
        }
    }
}

/// The session's current state; the UI shows it.
#[tauri::command]
pub async fn link_state(runtime: State<'_, LinkRuntime>) -> Result<LinkStateView, String> {
    Ok(runtime.state.lock().await.clone().into())
}

#[cfg(test)]
mod tests {
    /// The desk id and the relay address are resolved in RUST. Taken as UI
    /// parameters, the choice of which desk to connect would live inside the
    /// webview, and it would be right only as long as the UI asked Rust for
    /// the values.
    ///
    /// The test reads the command's SIGNATURE: it must take no string
    /// parameter from the UI. Adding a `desk_id` or `relay_url` parameter
    /// breaks it.
    #[test]
    fn pairing_command_takes_no_host_from_the_ui() {
        let src = include_str!("commands.rs");
        let body = &src[src
            .find("pub fn link_begin_pairing")
            .expect("command not found")..];
        let signature = &body[..body.find(')').expect("signature does not close")];
        assert!(
            !signature.contains("String"),
            "the command takes a string parameter from the UI: {signature}"
        );
        // And the values really are resolved from the single source.
        assert!(src.contains("endpoints::desk_id(&dir)"));
        assert!(src.contains("endpoints::relay_url()"));
    }

    /// The desk id does not travel TO the UI either: the UI does not use it,
    /// so passing it to the webview (and from there to logs and error
    /// reports) gains nothing.
    #[test]
    fn pairing_view_carries_no_desk_id() {
        let src = include_str!("commands.rs");
        let start = src
            .find("pub struct PairingView {")
            .expect("PairingView not found");
        let body = &src[start..start + src[start..].find('}').unwrap()];
        assert!(
            !body.contains("desk_id"),
            "PairingView carries the desk id: {body}"
        );
    }
}
