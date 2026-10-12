//! The live session to the relay.
//!
//! This is what runs `DeskBridge`: it opens an **outgoing** connection to the
//! relay (a home computer sits behind NAT, so it cannot wait for incoming
//! ones), hands envelopes to `DeskLink`, sends the acknowledgements back and
//! sends heartbeats.
//!
//! The decision logic is not here: verification, acknowledgement and replay
//! protection live in `aleph-desk-link` and are tested there. Only the socket,
//! timing and reconnection are here.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aleph_desk_link::session::{Disconnect, Next, Reconnect};
use aleph_desk_link::{DeskLink, HEARTBEAT_EVERY_MS};
use futures_util::{SinkExt, StreamExt};
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;

use super::DeskBridge;
use crate::membership::MembershipManager;

/// The header the relay reads the desk id from.
///
/// NOT the query string: the id is a routing address with no need to sit
/// where proxies and access logs leak most easily.
const DESK_ID_HEADER: &str = "Aleph-Desk";

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The session's externally visible state; the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkState {
    /// No phone is paired.
    Unpaired,
    Connecting,
    Connected,
    /// The connection dropped; it will be retried.
    Retrying {
        in_secs: u64,
    },
    /// Not retried. The user is shown THIS message.
    Stopped {
        message: String,
    },
}

/// The desk's live link layer. `LinkRuntime` keeps a copy too, so that a
/// cancel can revoke the key.
pub type SharedLink = Arc<Mutex<DeskLink<DeskBridge>>>;

/// How often to check that the key is still valid. This is the longest delay
/// between the QR's death and the connection being cut.
const KEY_CHECK_EVERY: Duration = Duration::from_secs(1);

/// Keeps the relay session going indefinitely.
///
/// The loop ends only on `Stopped`: a desk that gives up is a desk that died
/// silently, and the user cannot see that from the computer.
///
/// `link` is built by the caller (`LinkRuntime::restart`), which attaches the
/// session stamp and the key's deadline.
pub async fn run(
    app: AppHandle,
    relay_url: String,
    desk_id: String,
    link: SharedLink,
    state: Arc<Mutex<LinkState>>,
) {
    let mut backoff = Reconnect::new();

    // One refresh-and-retry per run of 401s. The relay checks the same
    // Sentinel access token every other route does, and that token lives two
    // hours, so the first reconnect after it expires gets a 401 that a refresh
    // cures. Only a 401 that survives a refreshed token is the PERMANENT
    // `Unauthorized` stop ("sign in again").
    let mut auth_retry_used = false;

    loop {
        // Reconnecting with a QR key that died unused would carry a dead key
        // to the relay: stop and say why.
        if !link.lock().await.key_live(now_ms()) {
            if let Next::Stop { message, .. } = backoff.on_disconnect(&Disconnect::PairingExpired) {
                *state.lock().await = LinkState::Stopped { message };
            }
            return;
        }

        *state.lock().await = LinkState::Connecting;

        let membership = app.state::<MembershipManager>();
        let result = match membership.fresh_access_token().await {
            Some(token) => {
                let r = connect_once(&app, &relay_url, &desk_id, &token, &link, &state).await;
                if matches!(r, Err(Disconnect::Unauthorized)) {
                    let refreshed = !auth_retry_used
                        && membership.refresh_after_unauthorized(&token).await;
                    match unauthorized_next(auth_retry_used, refreshed) {
                        AuthRetry::Retry => {
                            auth_retry_used = true;
                            Err(Disconnect::Transient("access token refreshed".into()))
                        }
                        AuthRetry::Stop => r,
                    }
                } else {
                    auth_retry_used = false;
                    r
                }
            }
            None => Err(Disconnect::Unauthorized),
        };

        match result {
            Ok(()) => {
                // A clean close is a disconnect too; reconnect.
                backoff.connected();
            }
            Err(why) => match backoff.on_disconnect(&why) {
                Next::Stop { message, .. } => {
                    *state.lock().await = LinkState::Stopped { message };
                    return;
                }
                Next::RetryIn(d) => {
                    *state.lock().await = LinkState::Retrying {
                        in_secs: d.as_secs(),
                    };
                    tokio::time::sleep(d).await;
                    continue;
                }
            },
        }

        // A short pause after a successful session, so the loop never spins.
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[derive(Debug, PartialEq, Eq)]
enum AuthRetry {
    Retry,
    Stop,
}

/// What a relay 401 means: retry once when a NEW access token is in place,
/// otherwise it is the permanent "sign in again" stop.
fn unauthorized_next(auth_retry_used: bool, refreshed: bool) -> AuthRetry {
    if !auth_retry_used && refreshed {
        AuthRetry::Retry
    } else {
        AuthRetry::Stop
    }
}

async fn connect_once(
    app: &AppHandle,
    relay_url: &str,
    desk_id: &str,
    token: &str,
    link: &SharedLink,
    state: &Arc<Mutex<LinkState>>,
) -> Result<(), Disconnect> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    // `token` is fetched FRESH for every attempt (`run`: `fresh_access_token`)
    // and never carried from one connection to the next.

    let url = format!("{relay_url}/link/desk");
    let mut req = url
        .into_client_request()
        .map_err(|e| Disconnect::Transient(e.to_string()))?;
    req.headers_mut().insert(
        "Authorization",
        format!("Bearer {token}")
            .parse()
            .map_err(|_| Disconnect::Unauthorized)?,
    );
    req.headers_mut().insert(
        DESK_ID_HEADER,
        desk_id.parse().map_err(|_| Disconnect::Unauthorized)?,
    );

    let (ws, res) = tokio_tungstenite::connect_async(req)
        .await
        .map_err(classify)?;
    // A relay 401 already fails the handshake; checking explicitly still
    // guarantees the reason is classified correctly.
    if res.status() == 401 {
        return Err(Disconnect::Unauthorized);
    }

    *state.lock().await = LinkState::Connected;
    let (mut tx, mut rx) = ws.split();

    let mut beat = tokio::time::interval(Duration::from_millis(HEARTBEAT_EVERY_MS));
    let mut key_check = tokio::time::interval(KEY_CHECK_EVERY);
    // The first tick fires at once and tells the relay the desk is connected.
    loop {
        tokio::select! {
            _ = key_check.tick() => {
                // If the QR died unused (or the pairing was cancelled) the
                // connection is cut here; the relay session never stays open
                // on a dead key.
                if !link.lock().await.key_live(now_ms()) {
                    return Err(Disconnect::PairingExpired);
                }
            }
            _ = beat.tick() => {
                let env = { link.lock().await.heartbeat_due(now_ms()) };
                if let Some(env) = env {
                    let raw = serde_json::to_string(&env)
                        .map_err(|e| Disconnect::Transient(e.to_string()))?;
                    tx.send(Message::Text(raw.into()))
                        .await
                        .map_err(classify)?;
                }
            }
            msg = rx.next() => {
                let Some(msg) = msg else { return Ok(()) };
                let msg = msg.map_err(classify)?;
                let Message::Text(raw) = msg else { continue };

                // Verification, applying and the acknowledgement are NOT
                // here; they all live in DeskLink, under test.
                let out = { link.lock().await.handle_raw(now_ms(), &raw).await };
                match out {
                    Ok(envs) => {
                        // A verified envelope means a phone REALLY holds the
                        // key: the QR is spent.
                        spend_pairing(app);
                        for env in envs {
                            let raw = serde_json::to_string(&env)
                                .map_err(|e| Disconnect::Transient(e.to_string()))?;
                            tx.send(Message::Text(raw.into()))
                                .await
                                .map_err(classify)?;
                        }
                    }
                    Err(e) => {
                        // An envelope that fails verification is logged, not
                        // swallowed; but no reply goes out, or anyone sending
                        // unsigned envelopes could probe for the desk.
                        eprintln!("link: envelope rejected: {e}");
                    }
                }
            }
        }
    }
}

/// A phone used the key: the QR is spent.
///
/// # Why single use is marked HERE
///
/// The desktop cannot SEE a phone read the QR; it does not hold the camera.
/// Marking the QR spent when the session starts would kill it before it was
/// ever shown.
///
/// This is the only moment the desk can know: a verified envelope proves the
/// sender HOLDS the key. From then on the QR dies on screen and says why.
///
/// On its own this does not stop a second phone that read the same QR within
/// `PAIRING_TTL_MS`. That limit (one phone per pairing, and only the desk
/// owner's phone) is enforced by the relay, not by the desk; see
/// `aleph_desk_link::pairing`, "Relay contract".
fn spend_pairing(app: &AppHandle) {
    let state = app.state::<super::commands::PairingState>();
    let mut guard = match state.session.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    if let Some(s) = guard.as_mut() {
        // An "already used" error is not news: this runs on every envelope
        // and changes nothing after the first time.
        let _ = s.complete(now_ms());
    }
}

/// Maps a socket error to the class the reconnect policy understands.
///
/// The default is **transient**: treating an unknown error as permanent would
/// shut the desk down for good for no reason. That is the costlier mistake
/// here.
fn classify(e: tokio_tungstenite::tungstenite::Error) -> Disconnect {
    use tokio_tungstenite::tungstenite::Error as E;
    match &e {
        E::Http(r) if r.status() == 401 || r.status() == 403 => Disconnect::Unauthorized,
        E::Http(r) if r.status() == 409 => Disconnect::DeskConflict,
        E::Protocol(_) => Disconnect::Transient(e.to_string()),
        _ => Disconnect::Transient(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// See `classify`: the default is transient.
    #[test]
    fn unknown_error_counts_as_transient() {
        let e = tokio_tungstenite::tungstenite::Error::ConnectionClosed;
        assert!(matches!(classify(e), Disconnect::Transient(_)));
    }

    /// The access token lives two hours. The first 401 after it expires is
    /// retried ONCE with a refreshed token.
    #[test]
    fn expired_token_is_refreshed_and_retried_once() {
        assert_eq!(unauthorized_next(false, true), AuthRetry::Retry);
        // The refresh failed, or the refreshed token was rejected too: stop.
        assert_eq!(unauthorized_next(false, false), AuthRetry::Stop);
        assert_eq!(unauthorized_next(true, true), AuthRetry::Stop);
    }

    #[test]
    fn stopped_state_carries_a_message_for_the_user() {
        let s = LinkState::Stopped {
            message: "linkUnauthorized".into(),
        };
        match s {
            LinkState::Stopped { message } => assert!(!message.is_empty()),
            _ => panic!("expected Stopped"),
        }
    }
}
