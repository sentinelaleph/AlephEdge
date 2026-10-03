//! Is the phone relay actually there?
//!
//! Audit 2026-09-16: the pairing panel drew a live, counting-down QR although
//! `{api_base}/link/health` answered 404 — the relay had been built and tested
//! (aleph-edge-mobile/relay) but never deployed, and the phone app had not been
//! released. A QR that no phone can ever use is a control the user believes in
//! and cannot operate. The panel now asks this command first and only offers
//! pairing when the relay answers.

use std::time::Duration;

use crate::app::endpoints::api_base;

/// Health path served by the relay (`relay/server.go`: `GET /link/health`).
pub const RELAY_HEALTH_PATH: &str = "/link/health";

/// Builds the health URL from the API base. Pure, so the one derivation can be
/// tested without the network.
pub fn relay_health_url(base: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), RELAY_HEALTH_PATH)
}

/// `true` only when the relay answers its health path with a 2xx within a few
/// seconds. Any error, timeout or non-2xx is "unavailable" — never a guess.
#[tauri::command]
pub async fn link_relay_health() -> bool {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
    else {
        return false;
    };
    match client.get(relay_health_url(&api_base())).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_url_is_derived_from_the_api_base() {
        assert_eq!(
            relay_health_url("https://ribqa.com"),
            "https://ribqa.com/link/health"
        );
        assert_eq!(
            relay_health_url("https://ribqa.com/"),
            "https://ribqa.com/link/health"
        );
    }
}
