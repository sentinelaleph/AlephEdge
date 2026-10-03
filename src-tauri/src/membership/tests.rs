//! Membership session tests against a local scripted HTTP server. Nothing here
//! reaches ribqa.com, and the keychain is the per-thread test double in
//! `session::store` — never the user's real entry.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use super::model::{AuthUser, BillingStatus, PersistedSession, Session};
use super::session::{load_persisted_session, persist_session};
use super::MembershipManager;

/// (method, path, bearer, body) -> (status, json body)
type Handler = dyn Fn(&str, &str, &str, &str) -> (u16, String) + Send + Sync;

/// Serves `handler` on 127.0.0.1; returns the base URL. One request per
/// connection, `Connection: close`, so the client never reuses a socket.
fn serve(handler: Arc<Handler>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let handler = handler.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    return;
                }
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or_default().to_string();
                let path = parts.next().unwrap_or_default().to_string();
                let (mut len, mut bearer) = (0usize, String::new());
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
                        break;
                    }
                    let lower = h.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                    if lower.starts_with("authorization:") {
                        bearer = h["authorization:".len()..]
                            .trim()
                            .trim_start_matches("Bearer ")
                            .to_string();
                    }
                }
                let mut body = vec![0u8; len];
                let _ = reader.read_exact(&mut body);
                let body = String::from_utf8_lossy(&body).to_string();
                let (status, out) = handler(&method, &path, &bearer, &body);
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
                    out.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            });
        }
    });
    format!("http://{addr}")
}

const ACTIVE: &str = r#"{"tier":"aleph","status":"active","current_period_end":null,"cancel_at_period_end":false}"#;

fn user() -> AuthUser {
    AuthUser {
        id: "u1".into(),
        email: "member@example.test".into(),
        tier: "aleph".into(),
        role: "user".into(),
        display_name: String::new(),
    }
}

fn signed_in(base: String, access: &str, refresh: &str, expires: Option<Instant>) -> MembershipManager {
    let m = MembershipManager::with_base(base);
    {
        let mut g = m.state.lock().unwrap();
        g.session = Some(Session {
            access_token: access.into(),
            refresh_token: refresh.into(),
            user: user(),
            access_expires: expires,
        });
        g.last_status = Some(BillingStatus {
            tier: "aleph".into(),
            status: "active".into(),
            current_period_end: None,
            cancel_at_period_end: false,
        });
    }
    m
}

/// A server that rotates refresh tokens the way Sentinel does: `old-refresh`
/// works exactly once, and billing accepts only the new access token.
fn rotating_server(refresh_calls: Arc<AtomicUsize>) -> String {
    let used = Arc::new(StdMutex::new(false));
    serve(Arc::new(move |_m, path, bearer, body| {
        if path.starts_with("/api/v1/billing/status") {
            return if bearer == "new-access" {
                (200, ACTIVE.into())
            } else {
                (401, r#"{"error":"expired"}"#.into())
            };
        }
        if path.starts_with("/api/v1/auth/refresh") {
            refresh_calls.fetch_add(1, Ordering::SeqCst);
            // Long enough that two callers really overlap.
            std::thread::sleep(Duration::from_millis(150));
            let mut used = used.lock().unwrap();
            if body.contains("old-refresh") && !*used {
                *used = true;
                return (
                    200,
                    r#"{"access_token":"new-access","refresh_token":"new-refresh","expires_in":7200}"#
                        .into(),
                );
            }
            return (401, r#"{"error":"Invalid or expired refresh token"}"#.into());
        }
        (404, "{}".into())
    }))
}

#[test]
fn two_callers_hitting_401_together_spend_the_refresh_token_once() {
    // The UI's membership poll and the signal stream's 401 handler both call
    // refresh_status. With rotation, the second refresh used to present a
    // retired token and fail ("token refresh failed") — and with a sign-out
    // on rejection it would have logged the user out.
    let calls = Arc::new(AtomicUsize::new(0));
    let base = rotating_server(calls.clone());
    let m = signed_in(base, "old-access", "old-refresh", None);

    let (a, b) = tauri::async_runtime::block_on(async {
        tokio::join!(m.refresh_status(), m.refresh_status())
    });
    assert!(a.is_ok(), "first caller: {a:?}");
    assert!(b.is_ok(), "second caller: {b:?}");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "refresh token spent twice");
    assert_eq!(m.access_token().as_deref(), Some("new-access"));
    assert!(m.view().active);
}

#[test]
fn a_refused_refresh_ends_the_session_instead_of_keeping_it_active() {
    // Revoked session: billing says 401, refresh says 401. The old code
    // returned an error but kept the session AND the cached "active" billing
    // status, so the trade gate stayed open on a dead session.
    let base = serve(Arc::new(|_m, path, _b, _body| {
        if path.starts_with("/api/v1/auth/refresh") {
            (401, r#"{"error":"Invalid or expired refresh token"}"#.into())
        } else {
            (401, r#"{"error":"expired"}"#.into())
        }
    }));
    let m = signed_in(base, "dead-access", "dead-refresh", None);
    persist_session(&PersistedSession::from_session("dead-refresh".into(), &user()));

    let r = tauri::async_runtime::block_on(m.refresh_status());
    assert!(r.is_err());
    let view = m.view();
    assert!(!view.active, "gate stayed open on a revoked session");
    assert!(!view.authenticated);
    assert!(m.access_token().is_none());
    assert!(load_persisted_session().is_none(), "dead refresh token kept");
}

#[test]
fn a_refresh_that_cannot_reach_the_server_keeps_the_session() {
    // Network trouble is not a sign-out.
    let base = serve(Arc::new(|_m, path, _b, _body| {
        if path.starts_with("/api/v1/auth/refresh") {
            (503, r#"{"error":"down"}"#.into())
        } else {
            (401, r#"{"error":"expired"}"#.into())
        }
    }));
    let m = signed_in(base, "old-access", "old-refresh", None);
    assert!(tauri::async_runtime::block_on(m.refresh_status()).is_err());
    assert!(m.view().authenticated, "signed out over a 503");
    assert_eq!(m.access_token().as_deref(), Some("old-access"));
    // But the gate no longer trusts a billing answer the 401 contradicted.
    assert!(!m.view().active);
}

#[test]
fn an_access_token_about_to_expire_is_refreshed_before_use() {
    // Before, nothing refreshed the token until some request had already
    // failed with 401; long-lived users (relay socket, precheck feeds) kept
    // presenting the dead token.
    let calls = Arc::new(AtomicUsize::new(0));
    let base = rotating_server(calls.clone());
    let m = signed_in(
        base,
        "old-access",
        "old-refresh",
        Some(Instant::now() + Duration::from_secs(30)),
    );
    let t = tauri::async_runtime::block_on(m.fresh_access_token());
    assert_eq!(t.as_deref(), Some("new-access"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Now an hour of life left: no refresh.
    let t = tauri::async_runtime::block_on(m.fresh_access_token());
    assert_eq!(t.as_deref(), Some("new-access"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn a_second_401_on_an_already_replaced_token_does_not_refresh_again() {
    let calls = Arc::new(AtomicUsize::new(0));
    let base = rotating_server(calls.clone());
    let m = signed_in(base, "old-access", "old-refresh", None);
    let first = tauri::async_runtime::block_on(m.refresh_after_unauthorized("old-access"));
    let late = tauri::async_runtime::block_on(m.refresh_after_unauthorized("old-access"));
    assert!(first && late);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn an_offline_launch_does_not_delete_the_saved_session() {
    // Port 9 on loopback: nothing listens, the refresh cannot be sent.
    let m = MembershipManager::with_base("http://127.0.0.1:9".into());
    persist_session(&PersistedSession::from_session("keep-me".into(), &user()));
    let view = tauri::async_runtime::block_on(m.restore());
    assert!(!view.authenticated);
    assert_eq!(
        load_persisted_session().map(|p| p.refresh_token).as_deref(),
        Some("keep-me"),
        "one start without network signed the user out for good"
    );
}

#[test]
fn a_refused_launch_refresh_deletes_the_saved_session() {
    let base = serve(Arc::new(|_m, _p, _b, _body| {
        (401, r#"{"error":"Invalid or expired refresh token"}"#.into())
    }));
    let m = MembershipManager::with_base(base);
    persist_session(&PersistedSession::from_session("revoked".into(), &user()));
    let view = tauri::async_runtime::block_on(m.restore());
    assert!(!view.authenticated);
    assert!(load_persisted_session().is_none());
}

#[test]
fn a_two_factor_challenge_is_named_not_reported_as_garbage() {
    let body = r#"{"two_factor_required":true,"challenge":"c","expires_in":300}"#;
    assert_eq!(
        super::client::parse_login_body(body).unwrap_err(),
        super::client::TWO_FACTOR_UNSUPPORTED
    );
    let ok = r#"{"access_token":"a","refresh_token":"r","expires_in":7200,
        "user":{"id":"1","email":"e","tier":"aleph","role":"user","display_name":""}}"#;
    assert_eq!(super::client::parse_login_body(ok).unwrap().expires_in, Some(7200));
}

#[test]
fn error_responses_become_stable_codes() {
    use super::client::error_code;
    assert_eq!(
        error_code(401, r#"{"error":"invalid credentials"}"#),
        "serverRejected|invalid credentials"
    );
    assert_eq!(error_code(503, "<html>bad gateway</html>"), "requestFailed|503");
    assert_eq!(error_code(500, r#"{"error":"  "}"#), "requestFailed|500");
}

// Launch calls restore twice at once (two mounts). The second call must not
// refresh with the token the first just retired, get 401, and delete the
// session the first one saved: that signed the user out on every launch.
#[test]
fn two_concurrent_launch_restores_keep_the_session() {
    let calls = Arc::new(AtomicUsize::new(0));
    let base = rotating_server(calls.clone());
    let m = Arc::new(MembershipManager::with_base(base));
    persist_session(&PersistedSession::from_session("old-refresh".into(), &user()));
    // Same thread (the test keychain is thread-local), truly concurrent.
    let (a, b) = tauri::async_runtime::block_on(async { tokio::join!(m.restore(), m.restore()) });
    assert!(a.authenticated && b.authenticated, "a concurrent launch restore signed the user out");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "the retired refresh token was sent again");
    assert_eq!(
        load_persisted_session().map(|p| p.refresh_token).as_deref(),
        Some("new-refresh")
    );
}

// Two processes share the keychain entry (an installed app beside a dev or
// test build). When the other one rotates the refresh token, ours is retired:
// the refresh must adopt the newer saved token instead of signing everyone out.
#[test]
fn a_token_rotated_by_another_process_is_adopted_not_a_sign_out() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let base = serve(Arc::new(move |_m, path, _bearer, body| {
        if path.starts_with("/api/v1/auth/refresh") {
            seen.fetch_add(1, Ordering::SeqCst);
            return if body.contains("other-refresh") {
                (200, r#"{"access_token":"a3","refresh_token":"r3","expires_in":7200}"#.into())
            } else {
                (401, r#"{"error":"Invalid or expired refresh token"}"#.into())
            };
        }
        (404, "{}".into())
    }));
    let m = signed_in(base, "old-access", "old-refresh", None);
    persist_session(&PersistedSession::from_session("other-refresh".into(), &user()));
    let ok = tauri::async_runtime::block_on(m.refresh_after_unauthorized("old-access"));
    assert!(ok, "signed out although another process held a valid token");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(m.access_token().as_deref(), Some("a3"));
    assert_eq!(load_persisted_session().map(|p| p.refresh_token).as_deref(), Some("r3"));
}
