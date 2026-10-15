//! The Telegram Bot API calls this app makes: `getMe`, `getUpdates`,
//! `sendMessage`. Nothing else, and only to `api_base()`.
//!
//! The token is part of every request URL (`/bot<token>/<method>`), so a
//! `reqwest::Error` (whose Display carries the URL) is NEVER formatted or
//! stored: every failure becomes a stable code here, before it leaves.

use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use super::BotToken;

/// The one host. Behind a function so tests never build a request to it.
pub fn api_base() -> &'static str {
    "https://api.telegram.org"
}

/// Every call but the long poll.
const CALL_TIMEOUT: Duration = Duration::from_secs(15);
/// `getUpdates` long-poll window (seconds, sent to Telegram).
pub const LONG_POLL_SECS: u64 = 25;
/// Telegram's own text limit is 4096 characters.
pub const MAX_TEXT: usize = 4000;

/// A failure as a stable code (`errors.*` in the UI).
pub type ApiError = String;

pub struct Api {
    base: String,
    http: reqwest::Client,
}

impl Api {
    pub fn new(base: &str) -> Self {
        let http = reqwest::Client::builder()
            .timeout(CALL_TIMEOUT)
            .build()
            .unwrap_or_default();
        Self { base: base.trim_end_matches('/').to_string(), http }
    }

    /// `<base>/bot<token>/<method>`. Never logged, never returned.
    fn url(&self, token: &BotToken, method: &str) -> String {
        method_url(&self.base, token, method)
    }

    async fn call(&self, token: &BotToken, method: &str, body: Value, timeout: Duration) -> Result<Value, ApiError> {
        let resp = self
            .http
            .post(self.url(token, method))
            .timeout(timeout)
            .json(&body)
            .send()
            .await
            .map_err(|e| transport_code(&e))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.map_err(|e| transport_code(&e))?;
        parse_reply(status, &text)
    }

    /// The bot's @username: the token works.
    pub async fn get_me(&self, token: &BotToken) -> Result<String, ApiError> {
        let v = self.call(token, "getMe", json!({}), CALL_TIMEOUT).await?;
        v.get("username")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "telegramBadResponse".to_string())
    }

    /// Updates after `offset`, waiting up to `wait_secs` for one.
    pub async fn get_updates(&self, token: &BotToken, offset: i64, wait_secs: u64) -> Result<Vec<Update>, ApiError> {
        let body = json!({ "offset": offset, "timeout": wait_secs, "allowed_updates": ["message"] });
        let v = self
            .call(token, "getUpdates", body, Duration::from_secs(wait_secs + 10))
            .await?;
        serde_json::from_value(v).map_err(|_| "telegramBadResponse".to_string())
    }

    /// Plain text, no parse mode: nothing in a message needs escaping.
    pub async fn send_message(&self, token: &BotToken, chat_id: i64, text: &str) -> Result<(), ApiError> {
        let body = json!({ "chat_id": chat_id, "text": clip(text), "disable_web_page_preview": true });
        self.call(token, "sendMessage", body, CALL_TIMEOUT).await.map(|_| ())
    }
}

fn method_url(base: &str, token: &BotToken, method: &str) -> String {
    format!("{base}/bot{}/{method}", token.expose())
}

/// A transport failure without its URL (the URL holds the token).
fn transport_code(e: &reqwest::Error) -> ApiError {
    if e.is_timeout() {
        "telegramTimeout".into()
    } else if e.is_decode() {
        "telegramBadResponse".into()
    } else {
        "telegramNetwork".into()
    }
}

#[derive(Deserialize)]
struct Envelope {
    ok: bool,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error_code: Option<u16>,
}

/// Telegram's `{ok, result}` envelope to a result or a code.
pub fn parse_reply(status: u16, body: &str) -> Result<Value, ApiError> {
    let env: Option<Envelope> = serde_json::from_str(body).ok();
    match env {
        Some(Envelope { ok: true, result: Some(v), .. }) => Ok(v),
        Some(Envelope { error_code, .. }) => Err(status_code(error_code.unwrap_or(status))),
        None if (200..300).contains(&status) => Err("telegramBadResponse".into()),
        None => Err(status_code(status)),
    }
}

fn status_code(code: u16) -> ApiError {
    match code {
        401 | 404 => "telegramTokenRejected".into(),
        // 400 "chat not found", 403 "bot was blocked by the user".
        400 | 403 => "telegramChatUnreachable".into(),
        409 => "telegramPollConflict".into(),
        429 => "telegramRateLimited".into(),
        c => format!("telegramHttp|{c}"),
    }
}

/// Cuts a text to Telegram's limit on a character boundary.
pub fn clip(text: &str) -> String {
    if text.chars().count() <= MAX_TEXT {
        return text.to_string();
    }
    let mut out: String = text.chars().take(MAX_TEXT - 1).collect();
    out.push('…');
    out
}

#[derive(Deserialize, Debug, Clone)]
pub struct Update {
    pub update_id: i64,
    #[serde(default)]
    pub message: Option<Message>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Message {
    pub chat: Chat,
    /// UNIX seconds.
    #[serde(default)]
    pub date: i64,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Chat {
    pub id: i64,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub first_name: Option<String>,
}

impl Chat {
    /// What the panel shows for the linked chat: @username, else first name.
    pub fn label(&self) -> Option<String> {
        self.username
            .as_ref()
            .map(|u| format!("@{u}"))
            .or_else(|| self.first_name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Built at run time so no token-shaped literal sits in the source.
    fn fake() -> String {
        format!("{}:{}", 123_456, "a".repeat(35))
    }

    #[test]
    fn the_base_is_the_telegram_api_and_tests_use_their_own() {
        assert_eq!(api_base(), "https://api.telegram.org");
        let t = BotToken::parse(&fake()).unwrap();
        assert_eq!(method_url("http://127.0.0.1:9", &t, "getMe"), format!("http://127.0.0.1:9/bot{}/getMe", fake()));
        // A trailing slash on the base does not double up.
        assert_eq!(Api::new("http://x/").base, "http://x");
    }

    #[test]
    fn replies_become_results_or_stable_codes() {
        assert_eq!(parse_reply(200, r#"{"ok":true,"result":{"username":"b"}}"#).unwrap()["username"], "b");
        assert_eq!(parse_reply(401, r#"{"ok":false,"error_code":401,"description":"Unauthorized"}"#).unwrap_err(), "telegramTokenRejected");
        assert_eq!(parse_reply(403, r#"{"ok":false,"error_code":403,"description":"Forbidden: bot was blocked by the user"}"#).unwrap_err(), "telegramChatUnreachable");
        assert_eq!(parse_reply(429, r#"{"ok":false,"error_code":429}"#).unwrap_err(), "telegramRateLimited");
        assert_eq!(parse_reply(502, "<html>bad gateway</html>").unwrap_err(), "telegramHttp|502");
        assert_eq!(parse_reply(200, "not json").unwrap_err(), "telegramBadResponse");
    }

    #[test]
    fn updates_parse_with_only_the_fields_used() {
        let v = parse_reply(
            200,
            r#"{"ok":true,"result":[{"update_id":7,"message":{"message_id":1,"date":1700000000,
                "chat":{"id":42,"type":"private","username":"ann","first_name":"Ann"},"text":"/start ABC"}},
                {"update_id":8,"edited_message":{}}]}"#,
        )
        .unwrap();
        let ups: Vec<Update> = serde_json::from_value(v).unwrap();
        assert_eq!(ups.len(), 2);
        let m = ups[0].message.as_ref().unwrap();
        assert_eq!((m.chat.id, m.chat.kind.as_str(), m.text.as_deref()), (42, "private", Some("/start ABC")));
        assert_eq!(m.chat.label().as_deref(), Some("@ann"));
        assert!(ups[1].message.is_none());
    }

    #[test]
    fn long_texts_are_clipped_to_the_limit() {
        let long = "x".repeat(MAX_TEXT + 50);
        let c = clip(&long);
        assert_eq!(c.chars().count(), MAX_TEXT);
        assert!(c.ends_with('…'));
        assert_eq!(clip("short"), "short");
    }
}
