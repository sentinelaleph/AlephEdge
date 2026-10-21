//! Tauri commands of Settings, TradingView. Only `tv_create_url` returns the
//! webhook URL, once; nothing here stores it.

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use super::api::{with_session, ApiError, CreatedUrl, ServerSettings, ServerStatus, SIGNED_OUT};
use super::tasks::{pass, save};
use super::{AlertResult, Prefs, TvManager};
use crate::membership::MembershipManager;

/// Online: an open stream, or a read within this long.
const ONLINE_WITHIN_MS: u64 = 15_000;

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
    /// `stream` | `poll` | None (not reading).
    pub transport: Option<String>,
    /// Reading now: an open stream, or a read in the last 15 s.
    pub online: bool,
    /// The server's offline notice (None: not readable, no webhook).
    pub notice: Option<ServerSettings>,
    /// The user set the notice by hand (the app no longer sets it).
    pub notice_touched: bool,
    /// Real entries whose outcome is still being looked up.
    pub pending: usize,
    /// Why the event stream last failed (an `errors.*` code), while the
    /// poll reads instead; None once a stream frame arrives.
    pub stream_error: Option<String>,
}

/// Online from the loop's state (pure).
pub fn is_online(transport: Option<&str>, last_read_at: Option<u64>, now_ms: u64) -> bool {
    transport == Some("stream") || last_read_at.is_some_and(|at| now_ms.saturating_sub(at) <= ONLINE_WITHIN_MS)
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
    let notice = if server.as_ref().is_some_and(|s| s.active) {
        with_session(membership, |token| {
            let (api, base) = (&mgr.api, base.clone());
            async move { api.settings(&base, &token).await }
        })
        .await
        .ok()
    } else {
        None
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
        transport: g.transport.map(str::to_string),
        online: is_online(g.transport, g.last_poll_at, crate::bot::engine::now_ms()),
        notice,
        notice_touched: g.stored.notify_touched,
        pending: g.stored.pending.len(),
        stream_error: g.stream_error.clone(),
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

/// The server's Telegram notice for an alert that arrives while the app is
/// offline. Set by hand here, the app no longer turns it on by itself.
#[tauri::command]
pub async fn tv_set_notice(
    app: AppHandle,
    mgr: State<'_, TvManager>,
    membership: State<'_, MembershipManager>,
    notify_offline: Option<bool>,
    offline_after_s: Option<u32>,
) -> Result<TvStatus, String> {
    if let Some(s) = offline_after_s {
        if !(10..=600).contains(&s) {
            return Err("tvOfflineAfterRange|10–600".to_string());
        }
    }
    let base = membership.base_url();
    with_session(membership.inner(), |token| {
        let (api, base) = (&mgr.api, base.clone());
        async move { api.set_settings(&base, &token, notify_offline, offline_after_s).await }
    })
    .await
    .map_err(|e| e.code())?;
    mgr.lock().stored.notify_touched = true;
    save(&app)?;
    Ok(status(&mgr, &membership).await)
}

/// The smallest order a Binance USDT-M symbol takes now, for the TradingView
/// bot's Limits: lot size and MIN_NOTIONAL at the live price. `status`:
/// `ok`, `unavailable` (Binance or the price did not answer) or `invalid`
/// (not a USDT symbol).
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SymbolMinimum {
    pub symbol: String,
    pub status: &'static str,
    pub price: Option<f64>,
    pub min_qty: Option<f64>,
    pub min_notional: Option<f64>,
    pub min_usdt: Option<f64>,
}

#[tauri::command]
pub async fn tv_symbol_minimum(app: AppHandle, symbol: String) -> SymbolMinimum {
    let empty = |symbol: String, status| SymbolMinimum { symbol, status, price: None, min_qty: None, min_notional: None, min_usdt: None };
    let Ok((symbol, _)) = super::message::read_tv_symbol(&symbol) else {
        return empty(symbol.trim().to_ascii_uppercase(), "invalid");
    };
    let rules = crate::bot::strategy::commands::binance_rules_cached(&app, &symbol).await;
    let exchange = app.state::<crate::exchange::ExchangeManager>();
    let price = crate::bot::engine::live_price(&exchange, "binance", &symbol, crate::bot::model::BotKind::Tradingview)
        .await
        .filter(|p| p.is_finite() && *p > 0.0);
    let (Some(r), Some(p)) = (rules, price) else {
        return SymbolMinimum { price, ..empty(symbol, "unavailable") };
    };
    SymbolMinimum {
        min_usdt: crate::bot::engine::tv_trade::symbol_min_usdt(r.lot.min_qty, r.lot.step, r.min_notional, p),
        symbol,
        status: "ok",
        price: Some(p),
        min_qty: Some(r.lot.min_qty),
        min_notional: Some(r.min_notional),
    }
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
        assert!(!dto.contains("url") && !dto.contains("token") && !dto.contains("passphrase"), "{dto}");
        // Nothing in the module prints or logs.
        for file in [
            include_str!("mod.rs"),
            include_str!("tasks.rs"),
            include_str!("api.rs"),
            include_str!("sse.rs"),
            include_str!("guard.rs"),
            include_str!("message.rs"),
            include_str!("../bot/engine/tv_trade.rs"),
            body,
        ] {
            for bad in ["println!", "eprintln!", "log::", "tracing::", "dbg!"] {
                assert!(!file.contains(bad), "{bad}");
            }
        }
        // No result row field can carry the passphrase or the body.
        let mod_src = include_str!("mod.rs");
        let row = &mod_src[mod_src.find("pub struct AlertResult {").unwrap()..];
        let row = &row[..row.find("\n}").unwrap()];
        assert!(!row.contains("passphrase") && !row.contains("pub body"), "{row}");
    }

    #[test]
    fn online_means_an_open_stream_or_a_recent_read() {
        use super::is_online;
        assert!(is_online(Some("stream"), None, 1_000_000));
        assert!(is_online(Some("poll"), Some(990_000), 1_000_000));
        assert!(!is_online(Some("poll"), Some(900_000), 1_000_000));
        assert!(!is_online(None, None, 1_000_000));
    }
}
