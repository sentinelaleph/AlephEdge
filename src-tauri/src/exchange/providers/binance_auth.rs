//! The one authenticated call in the exchange layer: verifying a just-added
//! Binance key's REAL permissions via the signed `apiRestrictions` endpoint
//! (detect a withdraw-enabled key, don't trust a dropdown). Only
//! Binance exposes this today; other exchanges keep self-declared permission.

use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};

use super::binance_http;
use super::binance_sign::BinanceSigner;
use super::super::model::{
    BinanceKeyCheckError, BinancePermissions, FuturesAccount, FuturesPosition,
};

const SPOT_BASE: &str = "https://api.binance.com";

/// The futures REST base, resolved where every other endpoint is (production
/// unless `ALEPH_EDGE_BINANCE_FUTURES_BASE` points it at the testnet).
pub(super) fn futures_base() -> String {
    crate::app::endpoints::binance_futures_base()
}

/// `GET /sapi/v1/account/apiRestrictions`. Binance's documented response
/// carries every field below. `enableWithdrawals`, `enableInternalTransfer`
/// and `permitsUniversalTransfer` are required: a body without them fails to
/// parse and the key is refused as unchecked, never let in on a guess.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiRestrictions {
    enable_withdrawals: bool,
    /// Moves funds to another Binance account (sub/master): a way out of the
    /// account without a withdrawal.
    enable_internal_transfer: bool,
    /// Universal transfer between wallets and sub-accounts: same risk.
    permits_universal_transfer: bool,
    /// Absent = unknown, treated as allowed: only an explicit `false` refuses.
    #[serde(default)]
    enable_futures: Option<bool>,
    /// Absent = unknown: no IP warning either way.
    #[serde(default)]
    ip_restrict: Option<bool>,
    /// Key creation, UNIX ms. Absent = the vault's own date is used for age.
    #[serde(default)]
    create_time: Option<u64>,
}

impl ApiRestrictions {
    fn permissions(&self) -> BinancePermissions {
        BinancePermissions {
            withdraw_enabled: self.enable_withdrawals,
            transfer_enabled: self.enable_internal_transfer || self.permits_universal_transfer,
            futures_enabled: self.enable_futures.unwrap_or(true),
            ip_restricted: self.ip_restrict,
            created_at: self.create_time.filter(|t| *t > 0),
        }
    }
}

pub async fn check_binance_permissions(
    client: &Client,
    api_key: &str,
    signer: BinanceSigner<'_>,
) -> Result<BinancePermissions, BinanceKeyCheckError> {
    // apiRestrictions lives on the production SPOT API; a testnet key is
    // unknown there. On the testnet the key is checked by a signed futures
    // account read instead: testnet balances are not real money and the
    // testnet has no withdrawals, so a key that can read the account is a
    // trade key there. (A testnet key is rejected by production Binance, so
    // it cannot reach real funds if the base is switched back.)
    if !crate::app::endpoints::binance_is_production() {
        binance_http::sync_clock(client).await;
        return fetch_binance_futures_account(client, api_key, signer)
            .await
            .map(|_| BinancePermissions {
                withdraw_enabled: false,
                transfer_enabled: false,
                futures_enabled: true,
                ip_restricted: None,
                created_at: None,
            });
    }
    // Stamp with Binance's clock: a PC clock running ahead makes Binance
    // answer -1021, which must not read as "wrong key".
    binance_http::sync_clock(client).await;
    let query = binance_http::with_recv_window(&format!("timestamp={}", binance_http::now_ms()));
    let signature = signer.signature_param(&query)?;
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
            .map(|body| body.permissions())
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
/// out of "invalid credentials"; everything else there is the key, with
/// Binance's code and text kept for the user.
fn key_check_rejection(status: u16, body: &str) -> BinanceKeyCheckError {
    match binance_http::classify_rejection(status, body) {
        BinanceKeyCheckError::Rejected { code: -1021, .. } => BinanceKeyCheckError::NetworkUnavailable,
        e @ BinanceKeyCheckError::InvalidCredentials { .. } => e,
        BinanceKeyCheckError::Rejected { code, msg } => BinanceKeyCheckError::InvalidCredentials { code, msg },
        _ => BinanceKeyCheckError::InvalidCredentials { code: 0, msg: format!("HTTP {status}") },
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
/// Read-only, so a Trade-only key suffices; signed like the permission check.
/// Amounts arrive as strings and are parsed to f64.
pub async fn fetch_binance_futures_account(
    client: &Client,
    api_key: &str,
    signer: BinanceSigner<'_>,
) -> Result<FuturesAccount, BinanceKeyCheckError> {
    let body = binance_http::signed(
        client,
        reqwest::Method::GET,
        "/fapi/v2/account",
        &format!("timestamp={}", binance_http::now_ms()),
        api_key,
        signer,
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
/// Requests sign through `BinanceSigner`; this name stays for the
/// documentation known-answer test below.
#[cfg(test)]
fn sign_query(secret: &str, query: &str) -> String {
    super::binance_sign::hmac_hex(secret, query)
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

    /// The signed-endpoint example from Binance's API documentation, read at
    /// run time from `tests/fixtures/binance_hmac_vector.json` (its `source`
    /// names the page): the documented example key is not a literal here.
    fn documented_vector() -> (String, String, String) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/binance_hmac_vector.json");
        let raw = std::fs::read_to_string(&path).expect("fixture readable");
        let v: serde_json::Value = serde_json::from_str(&raw).expect("fixture parses");
        let field = |k: &str| v[k].as_str().expect(k).to_string();
        (field("secretKey"), field("query"), field("signature"))
    }

    #[test]
    fn signature_matches_the_binance_documentation_example() {
        let (secret, query, signature) = documented_vector();
        assert_eq!(sign_query(&secret, &query), signature);
    }

    #[test]
    fn the_hmac_signer_is_the_documented_scheme() {
        let (secret, query, _) = documented_vector();
        assert_eq!(BinanceSigner::Hmac(&secret).signature_param(&query).unwrap(), sign_query(&secret, &query));
    }

    /// The response shape from Binance's "Get API Key Permission" docs.
    const DOC_BODY: &str = r#"{"ipRestrict":false,"createTime":1623840271000,"enableReading":true,"enableWithdrawals":false,"enableInternalTransfer":true,"enableMargin":false,"enableFutures":false,"permitsUniversalTransfer":true,"enableVanillaOptions":false,"enableFixApiTrade":false,"enableFixReadOnly":true,"enableSpotAndMarginTrading":false,"enablePortfolioMarginTrading":true}"#;

    #[test]
    fn the_documented_response_parses_with_transfers_ip_and_age() {
        let p = serde_json::from_str::<ApiRestrictions>(DOC_BODY).unwrap().permissions();
        assert!(!p.withdraw_enabled);
        assert!(p.transfer_enabled, "internal + universal transfer on");
        assert!(!p.futures_enabled);
        assert_eq!(p.ip_restricted, Some(false));
        assert_eq!(p.created_at, Some(1_623_840_271_000));
    }

    #[test]
    fn either_transfer_flag_alone_counts_and_optional_fields_may_be_absent() {
        let body = |internal: bool, universal: bool| {
            format!(r#"{{"enableWithdrawals":false,"enableInternalTransfer":{internal},"permitsUniversalTransfer":{universal},"enableFutures":true}}"#)
        };
        let p = |b: String| serde_json::from_str::<ApiRestrictions>(&b).unwrap().permissions();
        assert!(p(body(true, false)).transfer_enabled);
        assert!(p(body(false, true)).transfer_enabled);
        let clean = p(body(false, false));
        assert!(!clean.transfer_enabled && clean.futures_enabled);
        assert_eq!((clean.ip_restricted, clean.created_at), (None, None));
        // The transfer flags are required: a body without them is refused as
        // unchecked rather than read as "no transfers".
        assert!(serde_json::from_str::<ApiRestrictions>(r#"{"enableWithdrawals":false}"#).is_err());
    }

    #[test]
    fn a_clock_error_is_not_a_rejected_key() {
        assert!(matches!(
            key_check_rejection(400, r#"{"code":-1021,"msg":"Timestamp for this request was 1000ms ahead"}"#),
            BinanceKeyCheckError::NetworkUnavailable
        ));
        assert_eq!(
            key_check_rejection(401, r#"{"code":-2015,"msg":"Invalid API-key"}"#),
            BinanceKeyCheckError::InvalidCredentials { code: -2015, msg: "Invalid API-key".into() }
        );
        assert!(matches!(key_check_rejection(400, "garbage"), BinanceKeyCheckError::InvalidCredentials { code: 0, .. }));
    }
}
