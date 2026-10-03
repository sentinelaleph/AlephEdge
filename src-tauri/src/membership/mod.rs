//! Sentinel membership: real ribqa.com auth + billing gate.
//!
//! The bot cannot place a single order unless `MembershipView.active` is true
//! (PRD red line). Any doubt — signed out, network error, unfetched status —
//! resolves to *not active* (fail-safe). The state mutex is never held across
//! an `.await`; tokens are cloned out, the request runs, then results are
//! stored. The only lock held across a request is `refresh_gate`, an async
//! mutex that makes token refresh single-flight.

mod client;
mod commands;
pub mod model;
mod session;

pub use commands::{
    membership_login, membership_logout, membership_refresh, membership_restore, membership_status,
};

use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::Client;

use client::{ClientError, RefreshError};
use model::{
    health_from_billing, BillingStatus, MembershipHealth, MembershipView, PersistedSession, Session,
};
use session::{clear_persisted_session, load_persisted_session, persist_session};

// The host lives in ONE place: `app::endpoints`. This module used to carry its
// own copy of the default and its own env lookup, which is how a build ends up
// authenticating against one deployment and streaming from another. Same rule
// as the relay URL, and the same reason.

/// An access token this close to its expiry is refreshed before use, so a
/// request never leaves with a token that dies in flight.
const REFRESH_AHEAD: Duration = Duration::from_secs(120);

struct MemState {
    base_url: String,
    session: Option<Session>,
    last_status: Option<BillingStatus>,
}

/// Tauri-managed membership state.
pub struct MembershipManager {
    client: Client,
    state: Mutex<MemState>,
    /// Single-flight gate for `/auth/refresh`. The server ROTATES the refresh
    /// token on every use, so two refreshes racing with the same token (the UI
    /// poll and the signal stream's 401 handler, say) left the loser holding a
    /// token the server had already retired. Held across the request on
    /// purpose; it is an async mutex, unlike `state`.
    refresh_gate: tokio::sync::Mutex<()>,
}

/// `expires_in` (seconds) as a deadline on the monotonic clock.
fn expiry_from(expires_in: Option<u64>) -> Option<Instant> {
    expires_in.map(|secs| Instant::now() + Duration::from_secs(secs))
}

impl MembershipManager {
    pub fn new() -> Self {
        Self::with_base(crate::app::endpoints::api_base())
    }

    fn with_base(base: String) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            state: Mutex::new(MemState {
                base_url: base,
                session: None,
                last_status: None,
            }),
            refresh_gate: tokio::sync::Mutex::new(()),
        }
    }

    fn base(&self) -> String {
        self.state
            .lock()
            .expect("membership mutex")
            .base_url
            .clone()
    }

    /// The Sentinel API base URL. Used by other subsystems (e.g. the signal
    /// client) that authenticate against the same host.
    pub fn base_url(&self) -> String {
        self.base()
    }

    /// The current access token, if signed in. Never crosses IPC — only
    /// consumed Rust-side by subsystems that need to call Sentinel on the
    /// user's behalf (e.g. opening the signal stream).
    ///
    /// Prefer `fresh_access_token` where the caller can await: this one hands
    /// out the token as stored, even if it expired a minute ago.
    pub fn access_token(&self) -> Option<String> {
        self.state
            .lock()
            .expect("membership mutex")
            .session
            .as_ref()
            .map(|s| s.access_token.clone())
    }

    /// The access token, refreshed first when it is known to expire within
    /// `REFRESH_AHEAD`. A refresh that could not reach the server hands back
    /// the stored token (it may still be accepted); a refresh the server
    /// refused signs the user out and this is `None`.
    pub async fn fresh_access_token(&self) -> Option<String> {
        let (access, refresh, due) = {
            let g = self.state.lock().expect("membership mutex");
            let s = g.session.as_ref()?;
            let due = s
                .access_expires
                .is_some_and(|at| at <= Instant::now() + REFRESH_AHEAD);
            (s.access_token.clone(), s.refresh_token.clone(), due)
        };
        if !due {
            return Some(access);
        }
        match self.refresh_tokens(&refresh).await {
            Ok(fresh) => Some(fresh),
            Err(RefreshError::Rejected) => None,
            Err(RefreshError::Unavailable(_)) => self.access_token(),
        }
    }

    /// A Sentinel route refused `rejected_access` with 401. Refreshes once
    /// (single-flight) and reports whether a NEW access token is now in place,
    /// so the caller knows a retry can succeed. If another caller already
    /// replaced the token, no second refresh is spent.
    pub async fn refresh_after_unauthorized(&self, rejected_access: &str) -> bool {
        let refresh = {
            let g = self.state.lock().expect("membership mutex");
            match g.session.as_ref() {
                Some(s) if s.access_token != rejected_access => return true,
                Some(s) => s.refresh_token.clone(),
                None => return false,
            }
        };
        self.refresh_tokens(&refresh).await.is_ok()
    }

    /// The one place `/auth/refresh` is called for a live session.
    ///
    /// `seen_refresh` is the refresh token the caller read. If the session
    /// moved on while this call waited for the gate, the other refresh's
    /// result is used instead of spending a retired token. A rejected refresh
    /// signs the user out — a dead session must not keep a stale "active"
    /// billing status driving the trade gate.
    async fn refresh_tokens(&self, seen_refresh: &str) -> Result<String, RefreshError> {
        let _gate = self.refresh_gate.lock().await;
        let base = {
            let g = self.state.lock().expect("membership mutex");
            match g.session.as_ref() {
                None => return Err(RefreshError::Rejected),
                Some(s) if s.refresh_token != seen_refresh => {
                    return Ok(s.access_token.clone());
                }
                Some(_) => g.base_url.clone(),
            }
        };
        // The keychain entry is shared by every process of this app (and the
        // installed one beside a dev build). If another process rotated the
        // token, ours is retired: adopt the newer saved one and try it once
        // instead of signing the user out of every copy.
        let mut sent = seen_refresh.to_string();
        let mut adopted = false;
        loop {
            match client::refresh(&self.client, &base, &sent).await {
                Ok(tokens) => {
                    let persisted = {
                        let mut g = self.state.lock().expect("membership mutex");
                        match g.session.as_mut() {
                            // Signed out (or signed in afresh) while the request
                            // was in flight: do not resurrect or overwrite it.
                            Some(s) if s.refresh_token == seen_refresh => {
                                s.access_token = tokens.access_token.clone();
                                s.refresh_token = tokens.refresh_token.clone();
                                s.access_expires = expiry_from(tokens.expires_in);
                                Some(PersistedSession::from_session(
                                    tokens.refresh_token.clone(),
                                    &s.user,
                                ))
                            }
                            _ => None,
                        }
                    };
                    return match persisted {
                        Some(p) => {
                            // Keychain write happens OUTSIDE the state lock
                            // (blocking OS call).
                            persist_session(&p);
                            Ok(tokens.access_token)
                        }
                        None => Err(RefreshError::Rejected),
                    };
                }
                Err(RefreshError::Rejected) => {
                    let saved = load_persisted_session().map(|p| p.refresh_token);
                    if let Some(newer) = saved.filter(|t| *t != sent && !adopted) {
                        sent = newer;
                        adopted = true;
                        continue;
                    }
                    let ended = {
                        let mut g = self.state.lock().expect("membership mutex");
                        let same = g
                            .session
                            .as_ref()
                            .is_some_and(|s| s.refresh_token == seen_refresh);
                        if same {
                            g.session = None;
                            g.last_status = None;
                        }
                        same
                    };
                    // Only the token we sent is known dead.
                    if ended && load_persisted_session().is_some_and(|p| p.refresh_token == sent) {
                        clear_persisted_session();
                    }
                    return Err(RefreshError::Rejected);
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Authenticate, then fetch billing status (best-effort). Persists the
    /// refresh token so the next app launch can restore this session without
    /// asking for a password again.
    pub async fn login(&self, email: &str, password: &str) -> Result<MembershipView, String> {
        let base = self.base();
        let resp = client::login(&self.client, &base, email, password).await?;
        let token = resp.access_token.clone();
        persist_session(&PersistedSession::from_session(
            resp.refresh_token.clone(),
            &resp.user,
        ));
        {
            let mut g = self.state.lock().expect("membership mutex");
            g.session = Some(Session {
                access_token: resp.access_token,
                refresh_token: resp.refresh_token,
                user: resp.user,
                access_expires: expiry_from(resp.expires_in),
            });
            g.last_status = None;
        }
        let status = client::billing_status(&self.client, &base, &token)
            .await
            .ok();
        self.state.lock().expect("membership mutex").last_status = status;
        Ok(self.view())
    }

    /// Silently restores a session on app launch from the persisted refresh
    /// token, if any. Any failure (no persisted session, expired/revoked
    /// token, network error) falls back to the anonymous view — the user
    /// signs in fresh, same as before this existed.
    ///
    /// Only a refusal from the server deletes the saved session. An offline
    /// launch used to delete it too, so one start without network silently
    /// signed the user out for good.
    fn has_session(&self) -> bool {
        self.state.lock().expect("membership mutex").session.is_some()
    }

    pub async fn restore(&self) -> MembershipView {
        // Everything below runs under the refresh gate. Launch can call this
        // twice at once (two mounts); with the session check and the keychain
        // read outside the gate, the second call refreshed with the token the
        // first had just retired, got 401, and deleted the new session.
        let _gate = self.refresh_gate.lock().await;
        if self.has_session() {
            return self.view();
        }
        let Some(persisted) = load_persisted_session() else {
            return MembershipView::anonymous();
        };
        let base = self.base();
        let tokens = match client::refresh(&self.client, &base, &persisted.refresh_token).await {
            Ok(tokens) => tokens,
            Err(RefreshError::Rejected) => {
                // Only the token we sent is known dead; a newer one saved
                // meanwhile (another process) is left alone.
                if load_persisted_session().is_some_and(|p| p.refresh_token == persisted.refresh_token) {
                    clear_persisted_session();
                }
                return MembershipView::anonymous();
            }
            Err(RefreshError::Unavailable(_)) => return MembershipView::anonymous(),
        };
        // The rotated refresh token is saved FIRST: the old one is already
        // retired server-side, so losing this one (a crash during `me`) would
        // sign the user out on the next launch.
        persist_session(&PersistedSession::from_session(
            tokens.refresh_token.clone(),
            &persisted.user(),
        ));
        // The keychain copy is a file any local process can edit; the role
        // (admin skips the subscription gate) is re-read from the server. If
        // the server cannot say, the session continues as an ordinary member.
        let user = match client::me(&self.client, &base, &tokens.access_token).await {
            Ok(user) => user,
            Err(_) => {
                let mut user = persisted.user();
                user.role = String::new();
                user
            }
        };
        persist_session(&PersistedSession::from_session(
            tokens.refresh_token.clone(),
            &user,
        ));
        let access = tokens.access_token.clone();
        {
            let mut g = self.state.lock().expect("membership mutex");
            g.session = Some(Session {
                access_token: tokens.access_token,
                refresh_token: tokens.refresh_token,
                user,
                access_expires: expiry_from(tokens.expires_in),
            });
        }
        let status = client::billing_status(&self.client, &base, &access)
            .await
            .ok();
        self.state.lock().expect("membership mutex").last_status = status;
        self.view()
    }

    pub fn logout(&self) {
        let mut g = self.state.lock().expect("membership mutex");
        g.session = None;
        g.last_status = None;
        drop(g);
        clear_persisted_session();
    }

    /// Re-fetch billing status, refreshing the access token once on 401.
    ///
    /// A 401 that a refresh cannot cure clears the cached billing status: the
    /// gate then reads "unknown" (not active) instead of trading on the last
    /// answer a now-dead session received.
    pub async fn refresh_status(&self) -> Result<MembershipView, String> {
        let (base, access, refresh_tok) = {
            let g = self.state.lock().expect("membership mutex");
            match g.session.as_ref() {
                Some(s) => (
                    g.base_url.clone(),
                    s.access_token.clone(),
                    s.refresh_token.clone(),
                ),
                None => return Ok(self.view()),
            }
        };
        match client::billing_status(&self.client, &base, &access).await {
            Ok(status) => {
                self.state.lock().expect("membership mutex").last_status = Some(status);
                Ok(self.view())
            }
            Err(ClientError::Unauthorized) => self.refresh_and_retry(&base, &refresh_tok).await,
            Err(e) => Err(e.into_message()),
        }
    }

    async fn refresh_and_retry(
        &self,
        base: &str,
        refresh_tok: &str,
    ) -> Result<MembershipView, String> {
        let new_access = match self.refresh_tokens(refresh_tok).await {
            Ok(access) => access,
            Err(e) => {
                self.state.lock().expect("membership mutex").last_status = None;
                return Err(e.into_message());
            }
        };
        match client::billing_status(&self.client, base, &new_access).await {
            Ok(status) => {
                self.state.lock().expect("membership mutex").last_status = Some(status);
                Ok(self.view())
            }
            Err(e) => {
                if matches!(e, ClientError::Unauthorized) {
                    self.state.lock().expect("membership mutex").last_status = None;
                }
                Err(e.into_message())
            }
        }
    }

    pub fn view(&self) -> MembershipView {
        build_view(&self.state.lock().expect("membership mutex"))
    }

    /// Health-strip projection of membership state.
    pub fn health_state(&self) -> MembershipHealth {
        self.view().state
    }
}

impl Default for MembershipManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;

fn build_view(g: &MemState) -> MembershipView {
    let Some(session) = g.session.as_ref() else {
        return MembershipView::anonymous();
    };
    let email = Some(session.user.email.clone());
    let display_name = Some(session.user.display_name.clone()).filter(|s| !s.is_empty());

    // Admins have full access — the subscription gate never applies to them
    // (they have no billing record, so billing/status would otherwise leave
    // them stuck at "unknown"/"inactive").
    if session.user.is_admin() {
        let tier = if session.user.tier.is_empty() {
            "admin".to_string()
        } else {
            session.user.tier.clone()
        };
        return MembershipView {
            authenticated: true,
            active: true,
            state: MembershipHealth::Active,
            email,
            display_name,
            tier: Some(tier),
            current_period_end: None,
            cancel_at_period_end: false,
        };
    }

    let (state, tier, period_end, cancel) = match g.last_status.as_ref() {
        Some(b) => (
            health_from_billing(b),
            Some(b.tier.clone()),
            b.current_period_end.clone().filter(|s| !s.is_empty()),
            b.cancel_at_period_end,
        ),
        None => (
            MembershipHealth::Unknown,
            Some(session.user.tier.clone()),
            None,
            false,
        ),
    };
    let active = matches!(state, MembershipHealth::Active | MembershipHealth::Expiring);
    MembershipView {
        authenticated: true,
        active,
        state,
        email,
        display_name,
        tier,
        current_period_end: period_end,
        cancel_at_period_end: cancel,
    }
}
