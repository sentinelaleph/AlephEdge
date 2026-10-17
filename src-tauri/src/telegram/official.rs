//! The official bot (@SentinelAleph_bot), run by Sentinel (ribqa.com).
//!
//! In this mode the app holds no bot token and never calls api.telegram.org:
//! the four calls below go to the membership's base with the signed-in
//! session (`tvlink::api::with_session`: a fresh token, one refresh and
//! retry after a 401), and the server passes the text on to the chat the
//! user linked with `/start <code>`.
//!
//! - `POST   /api/edge/tg/link`  `{}` -> `{code, expiresAt, bot, deepLink}`
//! - `GET    /api/edge/tg`       -> `{available, bot, linked, chatLabel, linkedAt}`
//! - `DELETE /api/edge/tg`       -> `{unlinked}`
//! - `POST   /api/edge/tg/send`  `{text}` -> `{ok:true}` or `{code}`
//!
//! The server relays no commands, so `/status` and `/stop` exist only with
//! the user's own bot. Lines wait in a small `Outbox`: a rate limit or an
//! outage holds them for a while, never for ever.

use std::collections::VecDeque;
use std::time::Duration;

use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};

use crate::tvlink::api::{time_ms, ApiError};

/// The bot's handle when the server does not name it.
pub const BOT: &str = "SentinelAleph_bot";
const PATH: &str = "/api/edge/tg";
const CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// Signed out: the official bot is paused, not failing.
pub const SIGNED_OUT: &str = "tgSignedOut";
pub const NOT_LINKED: &str = "tgNotLinked";
pub const BLOCKED: &str = "tgBlocked";
pub const RATE_LIMITED: &str = "tgRateLimited";
pub const UNAVAILABLE: &str = "tgUnavailable";
pub const SEND_FAILED: &str = "tgSendFailed";
pub const BAD_TEXT: &str = "tgBadText";
/// Lines dropped from the outbox (`tgQueueDropped|<n>`).
pub const DROPPED: &str = "tgQueueDropped";

/// `POST /link`. The code is valid 10 minutes.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkReply {
    pub code: String,
    pub expires_at_ms: u64,
    pub bot: String,
    pub deep_link: String,
}

/// `GET /api/edge/tg`, times as UNIX ms.
#[derive(Clone, Debug, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerView {
    pub available: bool,
    pub bot: String,
    pub linked: bool,
    pub chat_label: Option<String>,
    pub linked_at: Option<u64>,
}

fn bad() -> ApiError {
    ApiError::Code("tgBadResponse".into())
}

fn parse_json(body: &str) -> Result<serde_json::Map<String, Value>, ApiError> {
    match serde_json::from_str::<Value>(body) {
        Ok(Value::Object(o)) => Ok(o),
        _ => Err(bad()),
    }
}

fn text_field(o: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    o.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// Only a Telegram link may be opened from this reply.
pub fn is_telegram_link(url: &str) -> bool {
    url.starts_with("https://t.me/") && !url.contains(char::is_whitespace)
}

pub fn parse_link(body: &str, now_ms: u64) -> Result<LinkReply, ApiError> {
    let o = parse_json(body)?;
    let code = text_field(&o, "code").ok_or_else(bad)?;
    let deep_link = text_field(&o, "deepLink").filter(|u| is_telegram_link(u)).ok_or_else(bad)?;
    let bot = text_field(&o, "bot").map(|b| b.trim_start_matches('@').to_string()).unwrap_or_else(|| BOT.into());
    // A missing time: the contract's 10 minutes from now.
    let expires_at_ms = time_ms(o.get("expiresAt")).unwrap_or(now_ms + super::LINK_TTL_MS);
    Ok(LinkReply { code, expires_at_ms, bot, deep_link })
}

pub fn parse_view(body: &str) -> Result<ServerView, ApiError> {
    let o = parse_json(body)?;
    Ok(ServerView {
        available: o.get("available").and_then(Value::as_bool).unwrap_or(false),
        bot: text_field(&o, "bot").map(|b| b.trim_start_matches('@').to_string()).unwrap_or_else(|| BOT.into()),
        linked: o.get("linked").and_then(Value::as_bool).unwrap_or(false),
        chat_label: text_field(&o, "chatLabel"),
        linked_at: time_ms(o.get("linkedAt")),
    })
}

/// A refused call: the server's `{code}` when it is one of ours, else one
/// by status.
pub fn error_of(status: u16, body: &str) -> ApiError {
    if status == 401 {
        return ApiError::Unauthorized;
    }
    let sent = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("code").and_then(Value::as_str).map(str::to_string))
        .filter(|c| c.starts_with("tg") && c.len() <= 40 && c.bytes().all(|b| b.is_ascii_alphanumeric()));
    if let Some(c) = sent {
        return ApiError::Code(c);
    }
    ApiError::Code(match status {
        400 => BAD_TEXT.into(),
        409 => NOT_LINKED.into(),
        429 => RATE_LIMITED.into(),
        503 => UNAVAILABLE.into(),
        502 => SEND_FAILED.into(),
        s => format!("tgHttp|{s}"),
    })
}

/// The relay client. Holds no token and no chat id.
pub struct OfficialApi {
    http: Client,
}

impl Default for OfficialApi {
    fn default() -> Self {
        Self::new()
    }
}

impl OfficialApi {
    pub fn new() -> Self {
        Self { http: Client::builder().timeout(CALL_TIMEOUT).build().unwrap_or_default() }
    }

    async fn call(&self, req: reqwest::RequestBuilder) -> Result<String, ApiError> {
        let res = req.send().await.map_err(|e| {
            ApiError::Code(if e.is_timeout() { "tgTimeout".into() } else { "sentinelUnreachable".into() })
        })?;
        let status = res.status().as_u16();
        let body = res.text().await.map_err(|_| bad())?;
        if (200..300).contains(&status) {
            Ok(body)
        } else {
            Err(error_of(status, &body))
        }
    }

    fn post(&self, url: String, token: &str, body: String) -> reqwest::RequestBuilder {
        self.http.post(url).bearer_auth(token).header("Content-Type", "application/json").body(body)
    }

    pub async fn link(&self, base: &str, token: &str, now_ms: u64) -> Result<LinkReply, ApiError> {
        let body = self.call(self.post(format!("{base}{PATH}/link"), token, "{}".into())).await?;
        parse_link(&body, now_ms)
    }

    pub async fn status(&self, base: &str, token: &str) -> Result<ServerView, ApiError> {
        let body = self.call(self.http.get(format!("{base}{PATH}")).bearer_auth(token)).await?;
        parse_view(&body)
    }

    pub async fn unlink(&self, base: &str, token: &str) -> Result<bool, ApiError> {
        let body = self.call(self.http.delete(format!("{base}{PATH}")).bearer_auth(token)).await?;
        Ok(parse_json(&body)?.get("unlinked").and_then(Value::as_bool).unwrap_or(false))
    }

    pub async fn send(&self, base: &str, token: &str, text: &str) -> Result<(), ApiError> {
        let body = json!({ "text": super::api::clip(text) }).to_string();
        let reply = self.call(self.post(format!("{base}{PATH}/send"), token, body)).await?;
        match parse_json(&reply)?.get("ok").and_then(Value::as_bool) {
            Some(true) => Ok(()),
            _ => Err(ApiError::Code(SEND_FAILED.into())),
        }
    }
}

/// Lines waiting for the relay.
pub const OUTBOX_CAP: usize = 10;
/// A line older than this is dropped, sent or not.
pub const OUTBOX_MAX_AGE_MS: u64 = 10 * 60 * 1000;
/// Wait after a 429 (the server allows 20 a minute).
pub const RATE_BACKOFF_MS: u64 = 60_000;
/// Wait after an outage or a failed hand-over.
pub const OUTAGE_BACKOFF_MS: u64 = 30_000;

/// The waiting lines, oldest first, with the time each was queued.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Outbox {
    pub lines: VecDeque<(String, u64)>,
    /// No send before this (UNIX ms).
    pub backoff_until: u64,
    /// Dropped since the last report.
    pub dropped: usize,
}

impl Outbox {
    /// Queues a line; past the cap the oldest one goes.
    pub fn push(&mut self, text: String, now_ms: u64) {
        self.lines.push_back((text, now_ms));
        while self.lines.len() > OUTBOX_CAP {
            self.lines.pop_front();
            self.dropped += 1;
        }
    }

    /// Drops lines that waited too long.
    pub fn expire(&mut self, now_ms: u64) {
        let before = self.lines.len();
        self.lines.retain(|(_, at)| at + OUTBOX_MAX_AGE_MS > now_ms);
        self.dropped += before - self.lines.len();
    }

    pub fn clear(&mut self) {
        self.dropped += self.lines.len();
        self.lines.clear();
        self.backoff_until = 0;
    }

    /// The drop count to report, once.
    pub fn take_dropped(&mut self) -> Option<String> {
        let n = std::mem::take(&mut self.dropped);
        (n > 0).then(|| format!("{DROPPED}|{n}"))
    }
}

/// How a drain ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Drained {
    /// Everything ready went out (`sent` lines).
    Done { sent: usize },
    /// A failure set `backoff_until`; `code` says why.
    Held { sent: usize, code: String },
    /// Still inside an earlier back-off: nothing was tried.
    Waiting,
    /// Signed out: nothing more is sent until the session is back.
    SignedOut { sent: usize },
    /// 409: the chat is gone or blocked the bot. Stop sending.
    Disconnected { sent: usize, code: String },
}

/// Sends the waiting lines in order through `send` until the box is empty
/// or a failure says to stop. A bad line is dropped and the rest go on.
/// The caller reports `last_error` from the result and `take_dropped`.
pub async fn drain<F, Fut>(outbox: &mut Outbox, now_ms: u64, mut send: F) -> (Drained, Option<String>)
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<(), ApiError>>,
{
    outbox.expire(now_ms);
    let mut sent = 0;
    let mut last_error = None;
    if outbox.backoff_until > now_ms {
        if outbox.lines.is_empty() {
            return (Drained::Done { sent }, last_error);
        }
        return (Drained::Waiting, last_error);
    }
    while let Some((text, _)) = outbox.lines.front().cloned() {
        match send(text).await {
            Ok(()) => {
                outbox.lines.pop_front();
                sent += 1;
                last_error = None;
            }
            Err(ApiError::Code(c)) if c == SIGNED_OUT || c == crate::tvlink::api::SIGNED_OUT => {
                outbox.clear();
                return (Drained::SignedOut { sent }, last_error);
            }
            Err(ApiError::Code(c)) if c == NOT_LINKED || c == BLOCKED => {
                outbox.clear();
                return (Drained::Disconnected { sent, code: c }, last_error);
            }
            Err(ApiError::Code(c)) if c == RATE_LIMITED => {
                outbox.backoff_until = now_ms + RATE_BACKOFF_MS;
                return (Drained::Held { sent, code: c }, last_error);
            }
            Err(ApiError::Code(c)) if c == UNAVAILABLE || c == SEND_FAILED || c == "sentinelUnreachable" || c == "tgTimeout" || c.starts_with("tgHttp|5") => {
                outbox.backoff_until = now_ms + OUTAGE_BACKOFF_MS;
                return (Drained::Held { sent, code: c }, last_error);
            }
            Err(e) => {
                // A line the server will never take (tgBadText, a 4xx).
                outbox.lines.pop_front();
                outbox.dropped += 1;
                last_error = Some(e.code());
            }
        }
    }
    (Drained::Done { sent }, last_error)
}

#[cfg(test)]
mod tests {
    //! Against a local scripted server on 127.0.0.1: nothing reaches
    //! ribqa.com or Telegram.
    use super::*;
    use crate::tvlink::api::{with_session, Session};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    /// (method, path, bearer, content type, body) -> (status, body)
    type Handler = dyn Fn(&str, &str, &str, &str, &str) -> (u16, String) + Send + Sync;

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
                    let body = String::from_utf8_lossy(&body).to_string();
                    let (status, out) = handler(&method, &path, &bearer, &ctype, &body);
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

    struct FakeSession {
        tokens: Mutex<Vec<&'static str>>,
        renewals: AtomicUsize,
    }

    impl FakeSession {
        fn with(tokens: Vec<&'static str>) -> Self {
            Self { tokens: Mutex::new(tokens), renewals: AtomicUsize::new(0) }
        }
    }

    impl Session for FakeSession {
        async fn token(&self) -> Option<String> {
            self.tokens.lock().unwrap().first().map(|t| t.to_string())
        }
        async fn renew(&self, _rejected: &str) -> bool {
            self.renewals.fetch_add(1, Ordering::SeqCst);
            let mut t = self.tokens.lock().unwrap();
            if t.len() > 1 {
                t.remove(0);
                true
            } else {
                false
            }
        }
    }

    /// What the notifier does: the session's token, one retry after a 401,
    /// signed out as `tgSignedOut`.
    async fn send_via(api: &OfficialApi, base: &str, s: &FakeSession, text: String) -> Result<(), ApiError> {
        with_session(s, |t| {
            let (base, text) = (base.to_string(), text.clone());
            async move { api.send(&base, &t, &text).await }
        })
        .await
        .map_err(|e| match e {
            ApiError::Code(c) if c == crate::tvlink::api::SIGNED_OUT => ApiError::Code(SIGNED_OUT.into()),
            e => e,
        })
    }

    #[test]
    fn the_link_flow_follows_the_contract() {
        let linked = Arc::new(Mutex::new(false));
        let l = linked.clone();
        let base = serve(Arc::new(move |m, p, bearer, ct, body| {
            assert_eq!(bearer, "acc");
            match (m, p) {
                ("POST", "/api/edge/tg/link") => {
                    assert_eq!((ct, body), ("application/json", "{}"));
                    (200, r#"{"code":"K7M2Q9XR","expiresAt":"2026-10-16T09:10:00Z","bot":"SentinelAleph_bot","deepLink":"https://t.me/SentinelAleph_bot?start=K7M2Q9XR"}"#.into())
                }
                ("GET", "/api/edge/tg") => {
                    if *l.lock().unwrap() {
                        (200, r#"{"available":true,"bot":"SentinelAleph_bot","linked":true,"chatLabel":"@ann","linkedAt":"2026-10-16T09:01:00Z"}"#.into())
                    } else {
                        (200, r#"{"available":true,"bot":"SentinelAleph_bot","linked":false,"chatLabel":null,"linkedAt":null}"#.into())
                    }
                }
                ("DELETE", "/api/edge/tg") => {
                    *l.lock().unwrap() = false;
                    (200, r#"{"unlinked":true}"#.into())
                }
                _ => (404, "{}".into()),
            }
        }));
        let api = OfficialApi::new();
        let link = run(api.link(&base, "acc", 0)).unwrap();
        assert_eq!(link.code, "K7M2Q9XR");
        assert_eq!(link.expires_at_ms, 1_792_141_800_000);
        assert_eq!(link.bot, "SentinelAleph_bot");
        assert!(link.deep_link.starts_with("https://t.me/SentinelAleph_bot?start="));
        let before = run(api.status(&base, "acc")).unwrap();
        assert!(before.available && !before.linked && before.chat_label.is_none());
        // The user sends /start CODE to the bot.
        *linked.lock().unwrap() = true;
        let after = run(api.status(&base, "acc")).unwrap();
        assert!(after.linked);
        assert_eq!(after.chat_label.as_deref(), Some("@ann"));
        assert_eq!(after.linked_at, Some(1_792_141_260_000));
        assert!(run(api.unlink(&base, "acc")).unwrap());
        assert!(!run(api.status(&base, "acc")).unwrap().linked);
    }

    #[test]
    fn a_link_reply_must_carry_a_telegram_link() {
        assert!(parse_link(r#"{"code":"A","deepLink":"https://evil.example/x"}"#, 0).is_err());
        assert!(parse_link(r#"{"code":"A","deepLink":"javascript:alert(1)"}"#, 0).is_err());
        assert!(parse_link(r#"{"deepLink":"https://t.me/b?start=A"}"#, 0).is_err(), "no code");
        let r = parse_link(r#"{"code":"A","deepLink":"https://t.me/b?start=A","bot":"@b"}"#, 1_000).unwrap();
        assert_eq!((r.bot.as_str(), r.expires_at_ms), ("b", 1_000 + super::super::LINK_TTL_MS));
        assert_eq!(parse_view("nope").unwrap_err().code(), "tgBadResponse");
    }

    #[test]
    fn a_send_posts_json_text_and_succeeds() {
        let got = Arc::new(Mutex::new(Vec::<String>::new()));
        let g = got.clone();
        let base = serve(Arc::new(move |m, p, bearer, ct, body| {
            assert_eq!((m, p, bearer, ct), ("POST", "/api/edge/tg/send", "acc", "application/json"));
            g.lock().unwrap().push(body.to_string());
            (200, r#"{"ok":true}"#.into())
        }));
        let api = OfficialApi::new();
        let s = FakeSession::with(vec!["acc"]);
        let mut ob = Outbox::default();
        ob.push("Aleph Edge · Paper\nTrade closed".into(), 0);
        ob.push("x".repeat(5_000), 0);
        let (end, err) = run(drain(&mut ob, 1, |t| send_via(&api, &base, &s, t)));
        assert_eq!(end, Drained::Done { sent: 2 });
        assert!(err.is_none() && ob.lines.is_empty() && ob.take_dropped().is_none());
        let got = got.lock().unwrap();
        let first: Value = serde_json::from_str(&got[0]).unwrap();
        assert_eq!(first, json!({ "text": "Aleph Edge · Paper\nTrade closed" }));
        let second: Value = serde_json::from_str(&got[1]).unwrap();
        assert_eq!(second["text"].as_str().unwrap().chars().count(), 4000, "clipped to the contract's limit");
    }

    #[test]
    fn a_401_refreshes_once_and_retries() {
        let base = serve(Arc::new(|_, _, bearer, _, _| if bearer == "new" { (200, r#"{"ok":true}"#.into()) } else { (401, "{}".into()) }));
        let api = OfficialApi::new();
        let s = FakeSession::with(vec!["old", "new"]);
        assert_eq!(run(send_via(&api, &base, &s, "hi".into())), Ok(()));
        assert_eq!(s.renewals.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_429_holds_the_line_backs_off_then_drops_past_the_cap() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let base = serve(Arc::new(move |_, _, _, _, _| {
            c.fetch_add(1, Ordering::SeqCst);
            (429, r#"{"code":"tgRateLimited"}"#.into())
        }));
        let api = OfficialApi::new();
        let s = FakeSession::with(vec!["acc"]);
        let mut ob = Outbox::default();
        ob.push("one".into(), 1_000);
        let (end, _) = run(drain(&mut ob, 1_000, |t| send_via(&api, &base, &s, t)));
        assert_eq!(end, Drained::Held { sent: 0, code: RATE_LIMITED.into() });
        assert_eq!(ob.lines.len(), 1, "kept");
        assert_eq!(ob.backoff_until, 1_000 + RATE_BACKOFF_MS);
        // Inside the back-off: no request at all.
        let (end, _) = run(drain(&mut ob, 30_000, |t| send_via(&api, &base, &s, t)));
        assert_eq!(end, Drained::Waiting);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // More lines than the cap: the oldest go, and that is reported once.
        for i in 0..OUTBOX_CAP + 2 {
            ob.push(format!("l{i}"), 31_000);
        }
        assert_eq!(ob.lines.len(), OUTBOX_CAP);
        assert_eq!(ob.lines.front().unwrap().0, "l2");
        assert_eq!(ob.take_dropped().as_deref(), Some("tgQueueDropped|3"));
        assert!(ob.take_dropped().is_none());
        // Nothing waits for ever.
        ob.expire(31_000 + OUTBOX_MAX_AGE_MS);
        assert!(ob.lines.is_empty());
        assert_eq!(ob.take_dropped().as_deref(), Some("tgQueueDropped|10"));
    }

    #[test]
    fn a_409_stops_sending_and_clears_the_box() {
        for code in [NOT_LINKED, BLOCKED] {
            let calls = Arc::new(AtomicUsize::new(0));
            let c = calls.clone();
            let body = format!(r#"{{"code":"{code}"}}"#);
            let base = serve(Arc::new(move |_, _, _, _, _| {
                c.fetch_add(1, Ordering::SeqCst);
                (409, body.clone())
            }));
            let api = OfficialApi::new();
            let s = FakeSession::with(vec!["acc"]);
            let mut ob = Outbox::default();
            ob.push("a".into(), 0);
            ob.push("b".into(), 0);
            let (end, _) = run(drain(&mut ob, 1, |t| send_via(&api, &base, &s, t)));
            assert_eq!(end, Drained::Disconnected { sent: 0, code: code.into() });
            assert!(ob.lines.is_empty());
            assert_eq!(calls.load(Ordering::SeqCst), 1, "{code}: no second line tried");
        }
        // A 409 without a body is "not linked".
        assert_eq!(error_of(409, "").code(), NOT_LINKED);
    }

    #[test]
    fn a_503_holds_for_a_while_and_names_the_outage() {
        let base = serve(Arc::new(|_, _, _, _, _| (503, r#"{"code":"tgUnavailable"}"#.into())));
        let api = OfficialApi::new();
        let s = FakeSession::with(vec!["acc"]);
        let mut ob = Outbox::default();
        ob.push("a".into(), 0);
        let (end, _) = run(drain(&mut ob, 5, |t| send_via(&api, &base, &s, t)));
        assert_eq!(end, Drained::Held { sent: 0, code: UNAVAILABLE.into() });
        assert_eq!(ob.backoff_until, 5 + OUTAGE_BACKOFF_MS);
        assert_eq!(ob.lines.len(), 1);
        assert_eq!(error_of(502, "<html>").code(), SEND_FAILED);
        assert_eq!(error_of(500, "").code(), "tgHttp|500");
        assert_eq!(error_of(400, r#"{"code":"tgBadText"}"#).code(), BAD_TEXT);
        assert_eq!(error_of(400, r#"{"code":"<b>evil</b>"}"#).code(), BAD_TEXT, "only our own codes pass through");
    }

    #[test]
    fn a_bad_line_is_dropped_and_the_rest_go_on() {
        let base = serve(Arc::new(|_, _, _, _, body| {
            if body.contains("bad") {
                (400, r#"{"code":"tgBadText"}"#.into())
            } else {
                (200, r#"{"ok":true}"#.into())
            }
        }));
        let api = OfficialApi::new();
        let s = FakeSession::with(vec!["acc"]);
        let mut ob = Outbox::default();
        ob.push("bad".into(), 0);
        ob.push("good".into(), 0);
        let (end, err) = run(drain(&mut ob, 1, |t| send_via(&api, &base, &s, t)));
        assert_eq!(end, Drained::Done { sent: 1 });
        assert!(err.is_none(), "a later success clears it");
        assert_eq!(ob.take_dropped().as_deref(), Some("tgQueueDropped|1"));
    }

    #[test]
    fn signed_out_pauses_without_a_request() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let base = serve(Arc::new(move |_, _, _, _, _| {
            c.fetch_add(1, Ordering::SeqCst);
            (200, r#"{"ok":true}"#.into())
        }));
        let api = OfficialApi::new();
        let s = FakeSession::with(vec![]);
        let mut ob = Outbox::default();
        ob.push("a".into(), 0);
        let (end, _) = run(drain(&mut ob, 1, |t| send_via(&api, &base, &s, t)));
        assert_eq!(end, Drained::SignedOut { sent: 0 });
        assert!(ob.lines.is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        // A refused refresh after a 401 is signed out too.
        let base = serve(Arc::new(|_, _, _, _, _| (401, "{}".into())));
        let s = FakeSession::with(vec!["old"]);
        assert_eq!(run(send_via(&api, &base, &s, "x".into())).unwrap_err().code(), SIGNED_OUT);
    }

    #[test]
    fn only_telegram_links_open() {
        assert!(is_telegram_link("https://t.me/SentinelAleph_bot?start=K7M2Q9XR"));
        for bad in ["http://t.me/x", "https://t.me.evil.example/x", "tg://resolve?domain=x", "https://t.me/x y", "file:///c:/x"] {
            assert!(!is_telegram_link(bad), "{bad}");
        }
    }
}
