//! The one authenticated call in the exchange layer: verifying a just-added
//! Binance key's REAL permissions via the signed `apiRestrictions` endpoint
//! (PRD §5.5 — detect a withdraw-enabled key, don't trust a dropdown). Only
//! Binance exposes this today; other exchanges keep self-declared permission.

use hmac::{Hmac, Mac};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

use super::binance_http;
use super::super::model::{
    BinanceKeyCheckError, BinancePermissions, FuturesAccount, FuturesPosition,
};

const SPOT_BASE: &str = "https://api.binance.com";

/// The futures REST base, resolved where every other endpoint is (production
/// unless `ALEPH_EDGE_BINANCE_FUTURES_BASE` points it at the testnet).
pub(super) fn futures_base() -> String {
    crate::app::endpoints::binance_futures_base()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiRestrictions {
    enable_withdrawals: bool,
    /// Absent = unknown, treated as allowed: only an explicit `false` refuses.
    #[serde(default)]
    enable_futures: Option<bool>,
}

pub async fn check_binance_permissions(
    client: &Client,
    api_key: &str,
    api_secret: &str,
) -> Result<BinancePermissions, BinanceKeyCheckError> {
    // apiRestrictions lives on the production SPOT API; a testnet key is
    // unknown there. On the testnet the key is checked by a signed futures
    // account read instead: testnet balances are not real money and the
    // testnet has no withdrawals, so a key that can read the account is a
    // trade key there. (A testnet key is rejected by production Binance, so
    // it cannot reach real funds if the base is switched back.)
    if !crate::app::endpoints::binance_is_production() {
        binance_http::sync_clock(client).await;
        return fetch_binance_futures_account(client, api_key, api_secret)
            .await
            .map(|_| BinancePermissions { withdraw_enabled: false, futures_enabled: true });
    }
    // Stamp with Binance's clock: a PC clock running ahead makes Binance
    // answer -1021, which must not read as "wrong key".
    binance_http::sync_clock(client).await;
    let query = binance_http::with_recv_window(&format!("timestamp={}", binance_http::now_ms()));
    let signature = sign_query(api_secret, &query);
    let url = format!("{SPOT_BASE}/sapi/v1/account/apiRestrictions?{query}&signature={signature}");

    let res = client
        .get(url)
        .header("X-MBX-APIKEY", api_key)
        .send()
        .await
        .map_err(|_| BinanceKeyCheckError::NetworkUnavailable)?;

    match res.status() {
        StatusCode::OK => res
            .json::<ApiRestrictions>()
            .await
            .map(|body| BinancePermissions {
                withdraw_enabled: body.enable_withdrawals,
                futures_enabled: body.enable_futures.unwrap_or(true),
            })
            .map_err(|_| BinanceKeyCheckError::NetworkUnavailable),
        // -2015 ("Invalid API-key, IP, or permissions") and bad signatures
        // land on 401/400 — almost always a copy/paste typo. A -1021 is a
        // clock problem, not a key problem: retryable, never "rejected".
        status @ (StatusCode::UNAUTHORIZED | StatusCode::BAD_REQUEST) => {
            let body = res.text().await.unwrap_or_default();
            Err(key_check_rejection(status.as_u16(), &body))
        }
        _ => Err(BinanceKeyCheckError::NetworkUnavailable),
    }
}

/// Maps a 400/401 from the key check. Only a timestamp error (-1021) is kept
/// out of "invalid credentials"; everything else there is the key.
fn key_check_rejection(status: u16, body: &str) -> BinanceKeyCheckError {
    match binance_http::classify_rejection(status, body) {
        BinanceKeyCheckError::Rejected { code: -1021, .. } => BinanceKeyCheckError::NetworkUnavailable,
        _ => BinanceKeyCheckError::InvalidCredentials,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFuturesAccount {
    total_wallet_balance: String,
    available_balance: String,
    total_unrealized_profit: String,
    positions: Vec<RawFuturesPosition>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFuturesPosition {
    symbol: String,
    position_amt: String,
    entry_price: String,
    unrealized_profit: String,
}

/// Fetches the signed USDT-M futures account: wallet balance + open positions.
/// Read-only, so a Trade-only key suffices; reuses the same HMAC signing as the
/// permission check. Amounts arrive as strings and are parsed to f64.
pub async fn fetch_binance_futures_account(
    client: &Client,
    api_key: &str,
    api_secret: &str,
) -> Result<FuturesAccount, BinanceKeyCheckError> {
    let body = binance_http::signed(
        client,
        reqwest::Method::GET,
        "/fapi/v2/account",
        &format!("timestamp={}", binance_http::now_ms()),
        api_key,
        api_secret,
    )
    .await?;
    serde_json::from_str::<RawFuturesAccount>(&body)
        .map(to_futures_account)
        .map_err(|_| BinanceKeyCheckError::Unknown)
}

fn parse_amount(s: &str) -> f64 {
    s.parse().unwrap_or(0.0)
}

fn to_futures_account(raw: RawFuturesAccount) -> FuturesAccount {
    let positions = raw
        .positions
        .into_iter()
        .filter(|p| parse_amount(&p.position_amt) != 0.0)
        .map(|p| FuturesPosition {
            symbol: p.symbol,
            position_amt: parse_amount(&p.position_amt),
            entry_price: parse_amount(&p.entry_price),
            unrealized_pnl: parse_amount(&p.unrealized_profit),
        })
        .collect();
    FuturesAccount {
        total_wallet_balance: parse_amount(&raw.total_wallet_balance),
        available_balance: parse_amount(&raw.available_balance),
        total_unrealized_pnl: parse_amount(&raw.total_unrealized_profit),
        positions,
    }
}

/// HMAC-SHA256 request signing, Binance's standard signed-endpoint scheme.
pub(super) fn sign_query(secret: &str, query: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts a key of any length");
    mac.update(query.as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub(super) fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod key_check_tests {
    use super::*;

    #[test]
    fn a_clock_error_is_not_a_rejected_key() {
        assert!(matches!(
            key_check_rejection(400, r#"{"code":-1021,"msg":"Timestamp for this request was 1000ms ahead"}"#),
            BinanceKeyCheckError::NetworkUnavailable
        ));
        assert!(matches!(
            key_check_rejection(401, r#"{"code":-2015,"msg":"Invalid API-key"}"#),
            BinanceKeyCheckError::InvalidCredentials
        ));
        assert!(matches!(key_check_rejection(400, "garbage"), BinanceKeyCheckError::InvalidCredentials));
    }
}
