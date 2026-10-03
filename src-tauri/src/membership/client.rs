//! HTTP client for the Sentinel (ribqa.com) auth + billing endpoints.
//!
//! Real contract (mirrored from the Sentinel web client):
//!   POST /api/v1/auth/login    { email, password } -> { access_token, refresh_token, user }
//!   POST /api/v1/auth/refresh   { refresh_token }   -> { access_token, refresh_token }
//!   GET  /api/v1/billing/status  (Bearer)           -> { tier, status, current_period_end, ... }

use reqwest::{Client, StatusCode};
use serde_json::json;

use super::model::{AuthUser, BillingStatus, LoginResponse, TokenResponse};

/// Distinguishes an expired token (retryable via refresh) from other failures.
pub enum ClientError {
    Unauthorized,
    Message(String),
}

impl ClientError {
    pub fn into_message(self) -> String {
        match self {
            ClientError::Unauthorized => "sessionExpired".to_string(),
            ClientError::Message(m) => m,
        }
    }
}

/// A stable code for an error response: `serverRejected|<server text>` when
/// the body carries `{ "error": ... }` (the server's own wording rides along
/// as detail), else `requestFailed|<status>`.
async fn error_message(res: reqwest::Response) -> String {
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    error_code(status.as_u16(), &body)
}

pub fn error_code(status: u16, body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
        .filter(|m| !m.trim().is_empty())
        .map(|m| format!("serverRejected|{m}"))
        .unwrap_or_else(|| format!("requestFailed|{status}"))
}

pub async fn login(
    client: &Client,
    base: &str,
    email: &str,
    password: &str,
) -> Result<LoginResponse, String> {
    let res = client
        .post(format!("{base}/api/v1/auth/login"))
        .json(&json!({ "email": email, "password": password }))
        .send()
        .await
        .map_err(|_| "sentinelUnreachable".to_string())?;
    if !res.status().is_success() {
        return Err(error_message(res).await);
    }
    let body = res
        .text()
        .await
        .map_err(|_| "unexpectedResponse|login".to_string())?;
    parse_login_body(&body)
}

/// Shown when the account has two-factor sign-in on. The server answers a
/// correct password with a challenge, not tokens, and the desk has no code
/// step yet; saying so beats "unexpectedResponse|login".
pub const TWO_FACTOR_UNSUPPORTED: &str = "twoFactorUnsupported";

/// A 200 from `/auth/login` is either the token pair or, for a 2FA account,
/// `{ "two_factor_required": true, "challenge": ... }`.
pub fn parse_login_body(body: &str) -> Result<LoginResponse, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "unexpectedResponse|login".to_string())?;
    if value
        .get("two_factor_required")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Err(TWO_FACTOR_UNSUPPORTED.to_string());
    }
    serde_json::from_value(value).map_err(|_| "unexpectedResponse|login".to_string())
}

/// Why a refresh did not produce tokens. Only `Rejected` means the session is
/// over: the server looked at the refresh token and refused it (rotated,
/// revoked, expired, account suspended). Everything else is "could not ask",
/// and must never sign the user out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshError {
    Rejected,
    Unavailable(String),
}

impl RefreshError {
    pub fn into_message(self) -> String {
        match self {
            RefreshError::Rejected => "sessionExpired".to_string(),
            RefreshError::Unavailable(m) => m,
        }
    }
}

pub async fn refresh(
    client: &Client,
    base: &str,
    refresh_token: &str,
) -> Result<TokenResponse, RefreshError> {
    let res = client
        .post(format!("{base}/api/v1/auth/refresh"))
        .json(&json!({ "refresh_token": refresh_token }))
        .send()
        .await
        .map_err(|_| RefreshError::Unavailable("sentinelUnreachable".to_string()))?;
    let status = res.status();
    if matches!(
        status,
        StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) {
        return Err(RefreshError::Rejected);
    }
    if !status.is_success() {
        return Err(RefreshError::Unavailable(format!(
            "refreshFailed|{}",
            status.as_u16()
        )));
    }
    res.json::<TokenResponse>()
        .await
        .map_err(|_| RefreshError::Unavailable("unexpectedResponse|refresh".to_string()))
}

/// `GET /api/v1/auth/me`: the account as the server sees it now. The role
/// the desk acts on comes from here, never from the keychain copy.
pub async fn me(client: &Client, base: &str, access_token: &str) -> Result<AuthUser, String> {
    let res = client
        .get(format!("{base}/api/v1/auth/me"))
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|_| "sentinelUnreachable".to_string())?;
    if !res.status().is_success() {
        return Err("accountLookupFailed".to_string());
    }
    res.json::<AuthUser>()
        .await
        .map_err(|_| "unexpectedResponse|account".to_string())
}

pub async fn billing_status(
    client: &Client,
    base: &str,
    access_token: &str,
) -> Result<BillingStatus, ClientError> {
    let res = client
        .get(format!("{base}/api/v1/billing/status"))
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|_| ClientError::Message("sentinelUnreachable".to_string()))?;
    if res.status() == StatusCode::UNAUTHORIZED {
        return Err(ClientError::Unauthorized);
    }
    if !res.status().is_success() {
        return Err(ClientError::Message(error_message(res).await));
    }
    res.json::<BillingStatus>()
        .await
        .map_err(|_| ClientError::Message("unexpectedResponse|billing".to_string()))
}
