//! Cockpit checks: on-demand connectivity + health probes behind the UI's two
//! "cockpit" buttons — one for the connected exchange, one for Sentinel
//! (ribqa.com). Unlike the passive health strip, these actively hit the
//! endpoints and report the concrete result, including the stream-ticket step
//! that a mis-set Content-Type fails with 415.

use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::State;

use crate::exchange::ExchangeManager;
use crate::membership::MembershipManager;

const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

/// One cockpit probe result. `detail` is a stable code (`code|detail`) the UI
/// localizes under `errors.*`, never prose.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PingResult {
    pub ok: bool,
    pub latency_ms: Option<u32>,
    pub detail: String,
}

/// Probes the connected exchange's public market endpoint (reachability +
/// latency) via the shared exchange status path.
#[tauri::command]
pub async fn cockpit_exchange(
    exchange: State<'_, ExchangeManager>,
    exchange_id: String,
) -> Result<PingResult, String> {
    let status = exchange.status(&exchange_id, "BTCUSDT").await;
    Ok(PingResult {
        ok: status.healthy,
        latency_ms: status.latency_ms,
        detail: if status.healthy {
            "cockpitReachable".into()
        } else {
            "cockpitUnreachable".into()
        },
    })
}

/// Probes Sentinel (ribqa.com): reachable → signed-in → stream-ticket OK. The
/// ticket step exercises exactly the endpoint the signal stream needs, so a
/// green result here means the live feed can connect.
#[tauri::command]
pub async fn cockpit_sentinel(
    membership: State<'_, MembershipManager>,
) -> Result<PingResult, String> {
    let base = membership.base_url();
    let token = membership.access_token();
    let client = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|_| "probeClientUnavailable".to_string())?;
    let started = Instant::now();

    // 1. Reachability.
    let health_ok = client
        .get(format!("{base}/health"))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false);
    if !health_ok {
        return Ok(PingResult {
            ok: false,
            latency_ms: None,
            // Names the host it actually tried, not a constant. A user who
            // pointed the desk somewhere else deserves to see WHERE it failed.
            detail: format!("cockpitHostUnreachable|{}", crate::app::endpoints::api_base()),
        });
    }

    // 2. Signed in?
    let Some(token) = token else {
        return Ok(PingResult {
            ok: false,
            latency_ms: Some(started.elapsed().as_millis() as u32),
            detail: "cockpitNotSignedIn".into(),
        });
    };

    // 3. Stream ticket (auth + the JSON-middleware-gated endpoint).
    let ticket = client
        .post(format!("{base}/api/v1/stream/ticket"))
        .bearer_auth(&token)
        .header("Content-Type", "application/json")
        .body("{}")
        .send()
        .await;
    let latency = Some(started.elapsed().as_millis() as u32);
    Ok(match ticket {
        Ok(r) if r.status().is_success() => PingResult {
            ok: true,
            latency_ms: latency,
            detail: "cockpitStreamReady".into(),
        },
        Ok(r) => PingResult {
            ok: false,
            latency_ms: latency,
            detail: format!("cockpitTicketFailed|{}", r.status().as_u16()),
        },
        Err(_) => PingResult {
            ok: false,
            latency_ms: latency,
            detail: "cockpitTicketNetwork".into(),
        },
    })
}
