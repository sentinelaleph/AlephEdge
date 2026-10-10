//! The one transport for the live-order path: sign a query built by
//! `binance_requests`, send it to the futures base, hand back the body.
//!
//! Every live call used to repeat sign → URL → send → status mapping inline,
//! which is how the stop kept pointing at `/fapi/v1/order` after Binance moved
//! conditional orders to the Algo service (2025-12-09, error -4120): each copy
//! had to be found and fixed separately. Builders stay pure and tested; this
//! file is the only place that touches the network for them.
//!
//! What the transport owns (2026-09-28, before the first real-money run):
//! - **Server time.** Requests are stamped with Binance's clock (offset from
//!   `/fapi/v1/time`), not the PC's. A Windows clock a second ahead used to
//!   fail every close, stop move and stop placement with -1021.
//! - **recvWindow** is explicit, and a -1021 resyncs the clock and resends
//!   once. A -1021 is a rejection, so resending cannot double an order.
//! - **Rate limits.** 429/418 put every signed call on hold for a while
//!   instead of retrying into an IP ban (docs: bans run 2 min to 3 days).
//! - **Unknown outcomes.** A 5xx or a timeout AFTER the request left means
//!   Binance may have executed it (docs: 503 = "execution status UNKNOWN").
//!   That is `Unknown`, never "failed": the caller must look before acting.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};

use reqwest::{Client, Method, StatusCode};
use serde::Deserialize;

use super::super::model::BinanceKeyCheckError as E;
use super::binance_auth::{futures_base, now_millis, sign_query};

/// Sent with every signed request. Binance's default is 5000 and its maximum
/// 60000; 10 s absorbs a slow link without accepting a stale request.
pub const RECV_WINDOW_MS: u64 = 10_000;
/// Hold after a 429 (Binance sends no Retry-After; weight windows are 1 min).
const RATE_LIMIT_HOLD_MS: u64 = 60_000;
/// Hold after a 418 IP ban (the shortest documented ban is 2 minutes).
const IP_BAN_HOLD_MS: u64 = 180_000;

/// Binance time minus local time, ms.
static CLOCK_OFFSET_MS: AtomicI64 = AtomicI64::new(0);
static CLOCK_SYNCED: AtomicBool = AtomicBool::new(false);
/// Local ms before which no signed request is sent.
static HOLD_UNTIL_MS: AtomicU64 = AtomicU64::new(0);

/// Binance's clock as best known: local time plus the synced offset.
pub fn now_ms() -> u64 {
    apply_offset(now_millis(), CLOCK_OFFSET_MS.load(Ordering::Relaxed))
}

fn apply_offset(local: u64, offset: i64) -> u64 {
    (local as i64).saturating_add(offset).max(0) as u64
}

/// Reads `/fapi/v1/time` and stores the offset (midpoint of the round trip).
/// A failure keeps the previous offset; the request then goes out on it.
pub async fn sync_clock(client: &Client) {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ServerTime {
        server_time: u64,
    }
    let before = now_millis();
    let Ok(res) = client
        .get(format!("{}/fapi/v1/time", futures_base()))
        .send()
        .await
    else {
        return;
    };
    let after = now_millis();
    if let Ok(t) = res.json::<ServerTime>().await {
        let mid = before / 2 + after / 2;
        CLOCK_OFFSET_MS.store(t.server_time as i64 - mid as i64, Ordering::Relaxed);
        CLOCK_SYNCED.store(true, Ordering::Relaxed);
    }
}

/// The synced offset to Binance's clock (ms), None before a sync succeeded.
pub fn clock_offset_ms() -> Option<i64> {
    CLOCK_SYNCED
        .load(Ordering::Relaxed)
        .then(|| CLOCK_OFFSET_MS.load(Ordering::Relaxed))
}

/// Whether signed calls are on hold after a 429/418.
pub fn rate_limited() -> bool {
    now_millis() < HOLD_UNTIL_MS.load(Ordering::Relaxed)
}

/// Sends a SIGNED request. `query` is a builder's output; its `timestamp` is
/// replaced with Binance time and `recvWindow` added, then the exact sent
/// string is signed. Ok = HTTP 200 body.
pub async fn signed(
    client: &Client,
    method: Method,
    path: &str,
    query: &str,
    api_key: &str,
    api_secret: &str,
) -> Result<String, E> {
    if !CLOCK_SYNCED.load(Ordering::Relaxed) {
        sync_clock(client).await;
    }
    for attempt in 0..2 {
        if rate_limited() {
            return Err(E::RateLimited);
        }
        let sent = with_recv_window(&restamp(query, now_ms()));
        let signature = sign_query(api_secret, &sent);
        let url = format!("{}{path}?{sent}&signature={signature}", futures_base());
        let res = match client
            .request(method.clone(), url)
            .header("X-MBX-APIKEY", api_key)
            .send()
            .await
        {
            Ok(res) => res,
            // Never connected: nothing reached Binance.
            Err(e) if e.is_connect() => return Err(E::NetworkUnavailable),
            // Timed out or broke after sending: it may have executed.
            Err(_) => return Err(E::Unknown),
        };
        match body_or_error(res).await {
            Err(E::Rejected { code: -1021, .. }) if attempt == 0 => sync_clock(client).await,
            other => return other,
        }
    }
    Err(E::Rejected {
        code: -1021,
        msg: "timestamp outside recvWindow after clock resync".into(),
    })
}

/// Public (unsigned) GET on the futures base, e.g. exchangeInfo.
pub async fn public_get(client: &Client, path_and_query: &str) -> Result<String, E> {
    let url = format!("{}{path_and_query}", futures_base());
    let res = client.get(url).send().await.map_err(|_| E::NetworkUnavailable)?;
    body_or_error(res).await
}

async fn body_or_error(res: reqwest::Response) -> Result<String, E> {
    let status = res.status();
    if status == StatusCode::OK {
        return res.text().await.map_err(|_| E::Unknown);
    }
    if status == StatusCode::TOO_MANY_REQUESTS || status.as_u16() == 418 {
        let hold = if status.as_u16() == 418 { IP_BAN_HOLD_MS } else { RATE_LIMIT_HOLD_MS };
        HOLD_UNTIL_MS.store(now_millis() + hold, Ordering::Relaxed);
        return Err(E::RateLimited);
    }
    if status.is_server_error() {
        return Err(E::Unknown);
    }
    let body = res.text().await.unwrap_or_default();
    let err = classify_rejection(status.as_u16(), &body);
    if matches!(err, E::RateLimited) {
        // A WAF 403 is a limit like 429: hold signed calls the same way.
        HOLD_UNTIL_MS.store(now_millis() + RATE_LIMIT_HOLD_MS, Ordering::Relaxed);
    }
    Err(err)
}

/// A 4xx with Binance's `{"code":-2019,"msg":"..."}`. Key and signature
/// problems are `InvalidCredentials` (the user fixes the key); anything else
/// is a rejection carrying its code, so callers can tell "no such order" from
/// "insufficient margin" from "precision".
pub fn classify_rejection(status: u16, body: &str) -> E {
    #[derive(Deserialize)]
    struct Raw {
        code: i64,
        #[serde(default)]
        msg: String,
    }
    // 408 / -1007: the backend timed out — "execution status unknown". It
    // may have executed, so it is never a rejection (see `E::Unknown`).
    if status == 408 {
        return E::Unknown;
    }
    match serde_json::from_str::<Raw>(body) {
        Ok(r) if r.code == -1007 => E::Unknown,
        Ok(r) if matches!(r.code, -2014 | -2015 | -1022 | -2008) => E::InvalidCredentials,
        Ok(r) => E::Rejected { code: r.code, msg: r.msg },
        Err(_) if status == 401 => E::InvalidCredentials,
        // Binance: "HTTP 403 is used when the WAF Limit has been violated".
        // Key problems arrive as JSON codes (-2014/-2015/-1022) above. Read as
        // a bad key, a WAF block made stop placement give up on a live
        // position; as a limit, it is retried after the hold.
        Err(_) if status == 403 => E::RateLimited,
        Err(_) => E::Rejected {
            code: 0,
            msg: format!("HTTP {status}"),
        },
    }
}

/// Replaces the `timestamp` parameter's value (builders always add one).
pub fn restamp(query: &str, ts: u64) -> String {
    query
        .split('&')
        .map(|kv| {
            if kv.starts_with("timestamp=") {
                format!("timestamp={ts}")
            } else {
                kv.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("&")
}

pub fn with_recv_window(query: &str) -> String {
    if query.split('&').any(|kv| kv.starts_with("recvWindow=")) {
        query.to_string()
    } else {
        format!("{query}&recvWindow={RECV_WINDOW_MS}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restamp_replaces_only_the_timestamp() {
        assert_eq!(
            restamp("symbol=BTCUSDT&side=BUY&timestamp=1", 42),
            "symbol=BTCUSDT&side=BUY&timestamp=42"
        );
        assert_eq!(restamp("timestamp=1", 7), "timestamp=7");
    }

    #[test]
    fn recv_window_is_added_once() {
        assert_eq!(with_recv_window("timestamp=1"), "timestamp=1&recvWindow=10000");
        assert_eq!(
            with_recv_window("timestamp=1&recvWindow=5000"),
            "timestamp=1&recvWindow=5000"
        );
    }

    #[test]
    fn offset_moves_the_clock_both_ways() {
        assert_eq!(apply_offset(1_000, 250), 1_250);
        assert_eq!(apply_offset(1_000, -250), 750);
        assert_eq!(apply_offset(100, -250), 0);
    }

    // Binance: HTTP 408 / -1007 "Timeout waiting for response from backend
    // server. Send status unknown; execution status unknown." The order may
    // have executed: reading it as a rejection made a timed-out entry final
    // ("liveOrderFailed") while the position could be open with no stop.
    #[test]
    fn backend_timeout_is_unknown_not_a_rejection() {
        let body = r#"{"code":-1007,"msg":"Timeout waiting for response from backend server. Send status unknown; execution status unknown."}"#;
        assert!(matches!(classify_rejection(408, body), E::Unknown));
        assert!(matches!(classify_rejection(400, body), E::Unknown));
        assert!(matches!(classify_rejection(408, ""), E::Unknown));
    }

    #[test]
    fn rejections_keep_binance_codes_and_key_problems_stay_key_problems() {
        assert!(matches!(
            classify_rejection(400, r#"{"code":-2019,"msg":"Margin is insufficient."}"#),
            E::Rejected { code: -2019, .. }
        ));
        assert!(matches!(
            classify_rejection(400, r#"{"code":-1021,"msg":"Timestamp..."}"#),
            E::Rejected { code: -1021, .. }
        ));
        assert!(matches!(
            classify_rejection(401, r#"{"code":-2015,"msg":"Invalid API-key"}"#),
            E::InvalidCredentials
        ));
        assert!(matches!(classify_rejection(401, "<html>"), E::InvalidCredentials));
        // WAF block: a limit, not a key problem; a JSON key code still wins.
        assert!(matches!(classify_rejection(403, "<html>403 Forbidden</html>"), E::RateLimited));
        assert!(matches!(classify_rejection(403, ""), E::RateLimited));
        assert!(matches!(
            classify_rejection(403, r#"{"code":-2015,"msg":"Invalid API-key"}"#),
            E::InvalidCredentials
        ));
        assert!(matches!(
            classify_rejection(400, "garbage"),
            E::Rejected { code: 0, .. }
        ));
    }
}
