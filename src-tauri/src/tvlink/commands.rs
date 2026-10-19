//! Tauri commands of Settings, TradingView. Only `tv_create_url` returns the
//! webhook URL, once; nothing here stores it.

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use super::api::{with_session, ApiError, CreatedUrl, ServerStatus, SIGNED_OUT};
use super::tasks::{pass, save};
use super::{AlertResult, Prefs, TvManager};
use crate::membership::MembershipManager;

/// What the panel shows. No URL, no token.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TvStatus {
    pub signed_in: bool,
    /// The server's view; `None` when signed out or not readable.
    pub server: Option<ServerStatus>,
    /// Why `server` is missing while signed in (`errors.*` code).
    pub server_error: Option<String>,
    pub prefs: Prefs,
    /// Newest first.
    pub results: Vec<AlertResult>,
    /// Last read failure of the background loop.
    pub last_error: Option<String>,
    pub last_poll_at: Option<u64>,
}

async fn status(mgr: &TvManager, membership: &MembershipManager) -> TvStatus {
    let base = membership.base_url();
    let read = with_session(membership, |token| {
        let (api, base) = (&mgr.api, base.clone());
        async move { api.status(&base, &token).await }
    })
    .await;
    let (signed_in, server, server_error) = match read {
        Ok(s) => (true, Some(s), None),
        Err(e) if e.code() == SIGNED_OUT => (false, None, None),
        Err(e) => (true, None, Some(e.code())),
    };
    let g = mgr.lock();
    TvStatus {
        signed_in,
        server,
        server_error,
        prefs: g.stored.prefs(),
        results: g.stored.results.clone(),
        last_error: g.last_error.clone(),
        last_poll_at: g.last_poll_at,
    }
}

#[tauri::command]
pub async fn tv_status(mgr: State<'_, TvManager>, membership: State<'_, MembershipManager>) -> Result<TvStatus, String> {
    Ok(status(&mgr, &membership).await)
}

/// Creates the webhook URL, or rotates it (the old one stops working). The
/// reply is the only time the app sees it.
#[tauri::command]
pub async fn tv_create_url(mgr: State<'_, TvManager>, membership: State<'_, MembershipManager>) -> Result<CreatedUrl, String> {
    let base = membership.base_url();
    with_session(membership.inner(), |token| {
        let (api, base) = (&mgr.api, base.clone());
        async move { api.create(&base, &token).await }
    })
    .await
    .map_err(|e: ApiError| e.code())
}

/// Revokes the URL: TradingView's posts to it are refused from now on.
#[tauri::command]
pub async fn tv_revoke(mgr: State<'_, TvManager>, membership: State<'_, MembershipManager>) -> Result<TvStatus, String> {
    let base = membership.base_url();
    with_session(membership.inner(), |token| {
        let (api, base) = (&mgr.api, base.clone());
        async move { api.revoke(&base, &token).await }
    })
    .await
    .map_err(|e| e.code())?;
    Ok(status(&mgr, &membership).await)
}

/// One test alert through the relay (TradingView sends webhooks only on
/// paid plans). With "Receive alerts" on it is read at once and listed as a
/// test; it never trades.
#[tauri::command]
pub async fn tv_send_test(app: AppHandle, mgr: State<'_, TvManager>, membership: State<'_, MembershipManager>) -> Result<TvStatus, String> {
    let base = membership.base_url();
    with_session(membership.inner(), |token| {
        let (api, base) = (&mgr.api, base.clone());
        async move { api.send_test(&base, &token).await }
    })
    .await
    .map_err(|e| e.code())?;
    if mgr.lock().stored.active() {
        pass(&app).await;
    }
    Ok(status(&mgr, &membership).await)
}

/// Saves the two switches (execute implies receive).
#[tauri::command]
pub async fn tv_set_prefs(
    app: AppHandle,
    mgr: State<'_, TvManager>,
    membership: State<'_, MembershipManager>,
    prefs: Prefs,
) -> Result<TvStatus, String> {
    mgr.lock().stored.set_prefs(prefs);
    save(&app)?;
    Ok(status(&mgr, &membership).await)
}

/// The last results, newest first.
#[tauri::command]
pub fn tv_results(app: AppHandle) -> Vec<AlertResult> {
    app.state::<TvManager>().lock().stored.results.clone()
}

#[cfg(test)]
mod tests {
    /// Only `tv_create_url` returns the URL type; the status DTO has no
    /// field that could carry it.
    #[test]
    fn only_create_returns_the_url() {
        let src = include_str!("commands.rs");
        let body = &src[..src.find("#[cfg(test)]").unwrap()];
        let returning_url: Vec<_> = body.lines().filter(|l| l.contains("Result<CreatedUrl")).collect();
        assert_eq!(returning_url.len(), 1, "{returning_url:?}");
        assert!(returning_url[0].contains("pub async fn tv_create_url"));
        let start = body.find("pub struct TvStatus {").unwrap();
        let dto = &body[start..start + body[start..].find('}').unwrap()];
        assert!(!dto.contains("url") && !dto.contains("token"), "{dto}");
        // Nothing in the module prints or logs.
        for file in [include_str!("mod.rs"), include_str!("tasks.rs"), include_str!("api.rs"), body] {
            for bad in ["println!", "eprintln!", "log::", "tracing::", "dbg!"] {
                assert!(!file.contains(bad), "{bad}");
            }
        }
    }
}
