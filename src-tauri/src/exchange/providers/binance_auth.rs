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
    // unknown there. Unverified, not "invalid".
    if !crate::app::endpoints::binance_is_production() {
        return Err(BinanceKeyCheckError::NetworkUnavailable);
    }
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
        // land on 401/400 — almost always a copy/paste typo.
        StatusCode::UNAUTHORIZED | StatusCode::BAD_REQUEST => {
            Err(BinanceKeyCheckError::InvalidCredentials)
        }
        _ => Err(BinanceKeyCheckError::NetworkUnavailable),
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
