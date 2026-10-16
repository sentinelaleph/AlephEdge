//! The four relay calls on Sentinel (ribqa.com), with the session the app
//! already has (the bearer token of `POST /api/v1/stream/ticket`):
//!
//! - `POST   /api/edge/tv-webhook`         creates or rotates the URL
//! - `GET    /api/edge/tv-webhook`         status
//! - `DELETE /api/edge/tv-webhook`         revokes it
//! - `GET    /api/edge/tv-webhook/events`  `?after=<id>&limit=<1..50>`
//!
//! The base is the membership's (`MembershipManager::base_url`, itself
//! `app::endpoints::api_base()`); tests pass their own local base. The
//! webhook URL appears only in the create reply, in `CreatedUrl`, whose
//! Debug hides it. A `reqwest::Error` is never formatted: every failure is
//! a stable code here.

use std::fmt;
use std::time::Duration;

use reqwest::{Client, StatusCode};
use serde::Serialize;
use serde_json::Value;

use crate::signal::time::parse_rfc3339_ms;

const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// Most events one read returns (the server's cap).
pub const PAGE: u32 = 50;

const PATH: &str = "/api/edge/tv-webhook";

/// A failure. `Unauthorized` lets the caller refresh the session once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    Unauthorized,
    /// 409 on create: another rotation ran at the same time.
    Conflict,
    /// A stable `errors.*` code.
    Code(String),
}

impl ApiError {
    pub fn code(&self) -> String {
        match self {
            ApiError::Unauthorized => "sessionExpired".into(),
            ApiError::Conflict => "tvRotateConflict".into(),
            ApiError::Code(c) => c.clone(),
        }
    }
}

/// The create reply. The only place the secret URL exists in the app; it
/// goes to the UI once and is not kept.
#[derive(Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreatedUrl {
    pub url: String,
    pub created_at: Option<u64>,
}

impl fmt::Debug for CreatedUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreatedUrl").field("url", &"<redacted>").field("created_at", &self.created_at).finish()
    }
}

/// `GET /api/edge/tv-webhook`, times as UNIX ms.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub active: bool,
    pub created_at: Option<u64>,
    pub last_received_at: Option<u64>,
    pub received_24h: u64,
    pub pending: u64,
}

/// One stored alert.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// The server's id as text (a number or a string on the wire).
    pub id: String,
    pub received_at: Option<u64>,
    pub body: String,
}

/// A time field: RFC3339 text, or UNIX seconds / ms as a number.
pub fn time_ms(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::String(s) => parse_rfc3339_ms(s).or_else(|| s.trim().parse::<u64>().ok().map(secs_or_ms)),
        Value::Number(n) => n.as_u64().map(secs_or_ms),
        _ => None,
    }
}

/// Below 10^11 a number is seconds (that is the year 5138 in seconds).
fn secs_or_ms(n: u64) -> u64 {
    if n < 100_000_000_000 {
        n * 1000
    } else {
        n
    }
}

fn count(v: Option<&Value>) -> u64 {
    v.and_then(Value::as_u64).unwrap_or(0)
}

fn id_text(v: &Value) -> Option<String> {
    match v {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    }
}

pub fn parse_status(body: &str) -> Result<ServerStatus, ApiError> {
    let v: Value = serde_json::from_str(body).map_err(|_| bad())?;
    let o = v.as_object().ok_or_else(bad)?;
    Ok(ServerStatus {
        active: o.get("active").and_then(Value::as_bool).unwrap_or(false),
        created_at: time_ms(o.get("createdAt")),
        last_received_at: time_ms(o.get("lastReceivedAt")),
        received_24h: count(o.get("received24h")),
        pending: count(o.get("pending")),
    })
}

pub fn parse_created(body: &str) -> Result<CreatedUrl, ApiError> {
    let v: Value = serde_json::from_str(body).map_err(|_| bad())?;
    let url = v.get("url").and_then(Value::as_str).filter(|u| u.starts_with("https://") || u.starts_with("http://")).ok_or_else(bad)?;
    Ok(CreatedUrl { url: url.to_string(), created_at: time_ms(v.get("createdAt")) })
}

pub fn parse_events(body: &str) -> Result<Vec<Event>, ApiError> {
    let v: Value = serde_json::from_str(body).map_err(|_| bad())?;
    let list = v.get("events").and_then(Value::as_array).ok_or_else(bad)?;
    Ok(list
        .iter()
        .filter_map(|e| {
            let id = id_text(e.get("id")?)?;
            let body = match e.get("body") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Null) | None => String::new(),
                // A server that stored the JSON itself: its text.
                Some(other) => other.to_string(),
            };
            Some(Event { id, received_at: time_ms(e.get("receivedAt")), body })
        })
        .collect())
}

fn bad() -> ApiError {
    ApiError::Code("tvBadResponse".into())
}

fn status_error(status: StatusCode) -> ApiError {
    match status.as_u16() {
        401 => ApiError::Unauthorized,
        409 => ApiError::Conflict,
        403 => ApiError::Code("tvForbidden".into()),
        429 => ApiError::Code("tvRateLimited".into()),
        s => ApiError::Code(format!("tvHttp|{s}")),
    }
}

/// The relay client. Holds no token and no URL.
pub struct Api {
    http: Client,
}

impl Default for Api {
    fn default() -> Self {
        Self::new()
    }
}

impl Api {
    pub fn new() -> Self {
        Self { http: Client::builder().timeout(CALL_TIMEOUT).build().unwrap_or_default() }
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<String, ApiError> {
        let res = req.send().await.map_err(|e| {
            ApiError::Code(if e.is_timeout() { "tvTimeout".into() } else { "sentinelUnreachable".into() })
        })?;
        let status = res.status();
        if !status.is_success() {
            return Err(status_error(status));
        }
        res.text().await.map_err(|_| bad())
    }

    pub async fn status(&self, base: &str, token: &str) -> Result<ServerStatus, ApiError> {
        let body = self.send(self.http.get(format!("{base}{PATH}")).bearer_auth(token)).await?;
        parse_status(&body)
    }

    /// Creates or rotates. One retry on 409 (a rotation raced this one).
    pub async fn create(&self, base: &str, token: &str) -> Result<CreatedUrl, ApiError> {
        let call = || {
            self.http
                .post(format!("{base}{PATH}"))
                .bearer_auth(token)
                .header("Content-Type", "application/json")
                .body("{}")
        };
        let body = match self.send(call()).await {
            Err(ApiError::Conflict) => self.send(call()).await?,
            r => r?,
        };
        parse_created(&body)
    }

    pub async fn revoke(&self, base: &str, token: &str) -> Result<bool, ApiError> {
        let body = self.send(self.http.delete(format!("{base}{PATH}")).bearer_auth(token)).await?;
        let v: Value = serde_json::from_str(&body).map_err(|_| bad())?;
        Ok(v.get("revoked").and_then(Value::as_bool).unwrap_or(false))
    }

    /// Events after `after` (none: from the oldest kept), oldest first.
    pub async fn events(&self, base: &str, token: &str, after: Option<&str>) -> Result<Vec<Event>, ApiError> {
        let mut query: Vec<(&str, String)> = vec![("limit", PAGE.to_string())];
        if let Some(a) = after {
            query.push(("after", a.to_string()));
        }
        let req = self.http.get(format!("{base}{PATH}/events")).query(&query).bearer_auth(token);
        let body = self.send(req).await?;
        parse_events(&body)
    }
}

/// Where a call gets its bearer token: the signed-in session.
pub trait Session {
    /// The access token (refreshed ahead of expiry); `None` = signed out.
    fn token(&self) -> impl std::future::Future<Output = Option<String>> + Send;
    /// `rejected` got a 401: refresh once; `true` when a new token is in place.
    fn renew(&self, rejected: &str) -> impl std::future::Future<Output = bool> + Send;
}

impl Session for crate::membership::MembershipManager {
    async fn token(&self) -> Option<String> {
        self.fresh_access_token().await
    }

    async fn renew(&self, rejected: &str) -> bool {
        self.refresh_after_unauthorized(rejected).await
    }
}

/// Signed out: the feature is off.
pub const SIGNED_OUT: &str = "tvSignedOut";

/// Runs `call` with the session's token; on 401 refreshes once and retries.
pub async fn with_session<S, T, F, Fut>(session: &S, call: F) -> Result<T, ApiError>
where
    S: Session,
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<T, ApiError>>,
{
    let token = session.token().await.ok_or_else(|| ApiError::Code(SIGNED_OUT.into()))?;
    match call(token.clone()).await {
        Err(ApiError::Unauthorized) => {
            if !session.renew(&token).await {
                return Err(ApiError::Code(SIGNED_OUT.into()));
            }
            let fresh = session.token().await.ok_or_else(|| ApiError::Code(SIGNED_OUT.into()))?;
            call(fresh).await
        }
        r => r,
    }
}

#[cfg(test)]
mod tests {
    //! Against a local scripted server on 127.0.0.1: nothing reaches
    //! ribqa.com and no real webhook is made.
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    /// (method, path with query, bearer, content type) -> (status, body)
    type Handler = dyn Fn(&str, &str, &str, &str) -> (u16, String) + Send + Sync;

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
                    let (mut len, mut bearer, mut ctype) = (0usize, String::new(), String::new());
                    loop {
                        let mut h = String::new();
                        if reader.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
                            break;
                        }
                        let lower = h.to_ascii_lowercase();
                        if let Some(v) = lower.strip_prefix("content-length:") {
                            len = v.trim().parse().unwrap_or(0);
                        }
                        if let Some(v) = lower.strip_prefix("content-type:") {
                            ctype = v.trim().to_string();
                        }
                        if lower.starts_with("authorization:") {
                            bearer = h["authorization:".len()..].trim().trim_start_matches("Bearer ").to_string();
                        }
                    }
                    let mut body = vec![0u8; len];
                    let _ = reader.read_exact(&mut body);
                    let (status, out) = handler(&method, &path, &bearer, &ctype);
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

    fn run<T>(f: impl std::future::Future<Output = T>) -> T {
        tauri::async_runtime::block_on(f)
    }

    /// Built at run time so no URL-shaped secret sits in the source.
    fn secret() -> String {
        format!("tvsecret{}", "Q7".repeat(12))
    }

    #[test]
    fn create_sends_json_and_retries_once_on_a_conflict() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let s = secret();
        let base = serve(Arc::new(move |m, p, bearer, ct| {
            assert_eq!((m, p, bearer, ct), ("POST", "/api/edge/tv-webhook", "acc", "application/json"));
            if c.fetch_add(1, Ordering::SeqCst) == 0 {
                return (409, r#"{"error":"rotating"}"#.into());
            }
            (200, format!(r#"{{"url":"https://www.ribqa.com/api/tv/{s}","createdAt":"2026-10-16T09:00:00Z"}}"#))
        }));
        let created = run(Api::new().create(&base, "acc")).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(created.url.ends_with(&secret()));
        assert_eq!(created.created_at, Some(1_792_141_200_000));
        // Never in Debug output.
        let dbg = format!("{created:?}");
        assert!(!dbg.contains(&secret()) && dbg.contains("<redacted>"), "{dbg}");
    }

    #[test]
    fn a_second_conflict_is_reported_not_looped() {
        let base = serve(Arc::new(|_, _, _, _| (409, "{}".into())));
        assert_eq!(run(Api::new().create(&base, "acc")).unwrap_err(), ApiError::Conflict);
    }

    #[test]
    fn status_events_and_revoke_parse_the_contract() {
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let log = seen.clone();
        let base = serve(Arc::new(move |m, p, _, _| {
            log.lock().unwrap().push(format!("{m} {p}"));
            match (m, p.split('?').next().unwrap()) {
                ("GET", "/api/edge/tv-webhook") => (
                    200,
                    r#"{"active":true,"createdAt":"2026-10-16T09:00:00.5+03:00","lastReceivedAt":null,"received24h":4,"pending":1}"#.into(),
                ),
                ("GET", "/api/edge/tv-webhook/events") => (
                    200,
                    r#"{"events":[{"id":7,"receivedAt":"2026-10-16T09:00:01Z","contentType":"text/plain","body":"hello"},
                                  {"id":"8","receivedAt":1760605202,"contentType":"application/json","body":{"action":"close"}},
                                  {"receivedAt":"x","body":"no id: dropped"}]}"#
                        .into(),
                ),
                ("DELETE", "/api/edge/tv-webhook") => (200, r#"{"revoked":true}"#.into()),
                _ => (404, "{}".into()),
            }
        }));
        let api = Api::new();
        let st = run(api.status(&base, "acc")).unwrap();
        assert_eq!(st, ServerStatus { active: true, created_at: Some(1_792_130_400_500), last_received_at: None, received_24h: 4, pending: 1 });
        let ev = run(api.events(&base, "acc", Some("6"))).unwrap();
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0], Event { id: "7".into(), received_at: Some(1_792_141_201_000), body: "hello".into() });
        assert_eq!(ev[1].id, "8");
        assert_eq!(ev[1].received_at, Some(1_760_605_202_000), "UNIX seconds read as such");
        assert_eq!(ev[1].body, r#"{"action":"close"}"#);
        assert!(run(api.revoke(&base, "acc")).unwrap());
        let log = seen.lock().unwrap();
        assert!(log.iter().any(|l| l == "GET /api/edge/tv-webhook/events?limit=50&after=6"), "{log:?}");
        // No cursor yet: no `after`.
        drop(log);
        run(api.events(&base, "acc", None)).unwrap();
        assert!(seen.lock().unwrap().iter().any(|l| l == "GET /api/edge/tv-webhook/events?limit=50"));
    }

    #[test]
    fn failures_become_stable_codes() {
        let base = serve(Arc::new(|_, p, _, _| match p {
            "/api/edge/tv-webhook" => (401, "{}".into()),
            _ => (502, "<html>".into()),
        }));
        let api = Api::new();
        assert_eq!(run(api.status(&base, "acc")).unwrap_err(), ApiError::Unauthorized);
        assert_eq!(run(api.events(&base, "acc", None)).unwrap_err().code(), "tvHttp|502");
        // Nothing listens on this port.
        let dead = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://{}", l.local_addr().unwrap())
        };
        assert_eq!(run(api.status(&dead, "acc")).unwrap_err().code(), "sentinelUnreachable");
        assert_eq!(parse_status("nope").unwrap_err().code(), "tvBadResponse");
        assert_eq!(parse_created(r#"{"url":"javascript:x"}"#).unwrap_err().code(), "tvBadResponse");
    }

    struct FakeSession {
        tokens: Mutex<Vec<&'static str>>,
        renewals: AtomicUsize,
        renew_ok: bool,
    }

    impl Session for FakeSession {
        async fn token(&self) -> Option<String> {
            self.tokens.lock().unwrap().first().map(|t| t.to_string())
        }
        async fn renew(&self, _rejected: &str) -> bool {
            self.renewals.fetch_add(1, Ordering::SeqCst);
            if self.renew_ok {
                self.tokens.lock().unwrap().remove(0);
            }
            self.renew_ok
        }
    }

    #[test]
    fn a_401_refreshes_the_session_once_then_retries() {
        let base = serve(Arc::new(|_, _, bearer, _| {
            if bearer == "new" {
                (200, r#"{"active":false}"#.into())
            } else {
                (401, "{}".into())
            }
        }));
        let api = Api::new();
        let s = FakeSession { tokens: Mutex::new(vec!["old", "new"]), renewals: AtomicUsize::new(0), renew_ok: true };
        let st = run(with_session(&s, |t| {
            let (api, base) = (&api, base.clone());
            async move { api.status(&base, &t).await }
        }))
        .unwrap();
        assert!(!st.active);
        assert_eq!(s.renewals.load(Ordering::SeqCst), 1);

        // Refresh refused: signed out, no second call.
        let s = FakeSession { tokens: Mutex::new(vec!["old"]), renewals: AtomicUsize::new(0), renew_ok: false };
        let r = run(with_session(&s, |t| {
            let (api, base) = (&api, base.clone());
            async move { api.status(&base, &t).await }
        }));
        assert_eq!(r.unwrap_err().code(), SIGNED_OUT);

        // Signed out from the start: no request at all.
        let s = FakeSession { tokens: Mutex::new(vec![]), renewals: AtomicUsize::new(0), renew_ok: false };
        let r = run(with_session(&s, |_t| async { Err::<(), _>(ApiError::Code("unreached".into())) }));
        assert_eq!(r.unwrap_err().code(), SIGNED_OUT);
    }
}
