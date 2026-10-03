//! Membership types mirroring the Sentinel (ribqa.com) auth + billing contract.
//!
//! The access token lives only in memory. The refresh token is persisted to
//! the OS keychain (never the UI, never disk) so re-launching the app doesn't
//! force a fresh email/password login every time — the same "stay signed in"
//! expectation the Sentinel web app gets from its own token persistence. Only
//! `MembershipView` (no tokens) ever reaches the UI.

use serde::{Deserialize, Serialize};

/// User identity returned by `/api/v1/auth/login`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthUser {
    pub id: String,
    pub email: String,
    /// Subscription tier slug, e.g. "aleph", "pro", "free".
    pub tier: String,
    /// The role the API reports for this account, e.g. "admin" | "user".
    /// The desk only reads it; what each role may do is the server's business
    /// and is not described here.
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub display_name: String,
}

impl AuthUser {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }
}

/// Login response envelope from `/api/v1/auth/login`.
#[derive(Debug, Clone, Deserialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub user: AuthUser,
    /// Access-token lifetime in seconds (`expires_in`). Kept so the desk can
    /// refresh BEFORE the token dies instead of discovering it from a 401.
    #[serde(default)]
    pub expires_in: Option<u64>,
}

/// Refresh response from `/api/v1/auth/refresh`.
#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default)]
    pub expires_in: Option<u64>,
}

/// Billing/subscription status from `/api/v1/billing/status`.
#[derive(Debug, Clone, Deserialize)]
pub struct BillingStatus {
    pub tier: String,
    /// "active" | "trialing" | "past_due" | "canceled" | "none" | ...
    pub status: String,
    /// The backend sends `null` here for accounts without a subscription, so
    /// it MUST be optional — a bare `String` fails to deserialize `null` and
    /// left the whole gate stuck at "unknown".
    #[serde(default)]
    pub current_period_end: Option<String>,
    #[serde(default)]
    pub cancel_at_period_end: bool,
}

/// The in-memory authenticated session (tokens never leave the process).
#[derive(Debug, Clone)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    pub user: AuthUser,
    /// When the access token stops being accepted, on the MONOTONIC clock:
    /// measured from `expires_in` at receipt, so a wrong wall clock on this
    /// machine (or the server's) cannot make a dead token look alive.
    pub access_expires: Option<std::time::Instant>,
}

/// What's persisted to the OS keychain across app launches — enough to
/// silently restore a session via `/api/v1/auth/refresh` without asking the
/// user to sign in again. No access token (short-lived; a fresh one is
/// fetched immediately via refresh on restore).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSession {
    pub refresh_token: String,
    pub id: String,
    pub email: String,
    pub tier: String,
    #[serde(default)]
    pub role: String,
    pub display_name: String,
}

impl PersistedSession {
    pub fn from_session(refresh_token: String, user: &AuthUser) -> Self {
        Self {
            refresh_token,
            id: user.id.clone(),
            email: user.email.clone(),
            tier: user.tier.clone(),
            role: user.role.clone(),
            display_name: user.display_name.clone(),
        }
    }

    pub fn user(&self) -> AuthUser {
        AuthUser {
            id: self.id.clone(),
            email: self.email.clone(),
            tier: self.tier.clone(),
            role: self.role.clone(),
            display_name: self.display_name.clone(),
        }
    }
}

/// Gate state derived from auth + billing. Mirrors the health strip vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MembershipHealth {
    Active,
    Expiring,
    Inactive,
    Unknown,
}

/// Secret-free membership projection sent to the UI. `active` is the single
/// gate the trading path checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipView {
    pub authenticated: bool,
    /// The only field the order path consults: false ⇒ bot cannot trade.
    pub active: bool,
    pub state: MembershipHealth,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_period_end: Option<String>,
    pub cancel_at_period_end: bool,
}

impl MembershipView {
    /// The signed-out / never-fetched view: authenticated but locked, or fully
    /// anonymous. Bot stays locked (fail-safe) whenever state is not `Active`.
    pub fn anonymous() -> Self {
        Self {
            authenticated: false,
            active: false,
            state: MembershipHealth::Unknown,
            email: None,
            display_name: None,
            tier: None,
            current_period_end: None,
            cancel_at_period_end: false,
        }
    }
}

/// Compute the gate from a billing status. Paid + active ⇒ trading allowed.
pub fn health_from_billing(b: &BillingStatus) -> MembershipHealth {
    let paid = b.tier != "free" && !b.tier.is_empty();
    let live = matches!(b.status.as_str(), "active" | "trialing");
    match (paid && live, b.cancel_at_period_end) {
        (true, true) => MembershipHealth::Expiring,
        (true, false) => MembershipHealth::Active,
        (false, _) => MembershipHealth::Inactive,
    }
}
