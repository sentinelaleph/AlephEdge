//! HTTP client for Sentinel's market-data services (PRD §3.3: FR + liquidity
//! depth come from the Sentinel API, not from each exchange directly).
//!
//! The routes this client calls, as the public API Center documents them
//! (https://ribqa.com/api-center):
//!   funding  → GET {base}/api/v1/market/funding-rates    (auth)
//!   depth    → GET {base}/api/v1/market/depth/{symbol}   (auth)
//!   btc      → GET {base}/api/v1/market/btc-macro
//!
//! Funding and depth are sentinel-api proxies onto DataHub, which reads
//! Binance USDT-M futures for every listed perp: funding for all perps in one
//! snapshot (rate in percent per interval, cached 60s, served `stale` for up
//! to 10 minutes if Binance fails) and ±1% depth for any perp on demand
//! (cached 15s). The older `/api/v1/arbitrage/funding-rates` (18 symbols)
//! and `/api/v1/liquidity/depth/{symbol}` (streamed books only) routes stay
//! for the web; the bot no longer uses them. An unknown symbol is a 404,
//! which lands on the same "unavailable" error as any other non-2xx.

use reqwest::Client;
use std::time::Duration;

use super::model::{BtcMacro, DepthAnalysis, FundingRate, FundingRatesResponse};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

pub fn build_client() -> Client {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .unwrap_or_default()
}

fn funding_url(base: &str) -> String {
    format!("{base}/api/v1/market/funding-rates")
}

fn depth_url(base: &str, symbol: &str) -> String {
    format!("{base}/api/v1/market/depth/{symbol}")
}

pub async fn funding_rates(
    client: &Client,
    base: &str,
    token: &str,
) -> Result<FundingSnapshot, String> {
    let res = client
        .get(funding_url(base))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|_| "fundingUnreachable".to_string())?;
    if !res.status().is_success() {
        return Err(format!("fundingRefused|{}", res.status().as_u16()));
    }
    let body: FundingRatesResponse = res
        .json()
        .await
        .map_err(|_| "fundingMalformed".to_string())?;
    usable_funding(body)
}

/// Funding rates plus whether DataHub served them from its stale fallback.
pub struct FundingSnapshot {
    pub rates: Vec<FundingRate>,
    pub stale: bool,
}

/// A snapshot DataHub marks unavailable is an error. A `stale` one is at most
/// 10 minutes old (DataHub refuses anything older) and is still used.
fn usable_funding(body: FundingRatesResponse) -> Result<FundingSnapshot, String> {
    if body.available == Some(false) {
        return Err("fundingUnavailable".to_string());
    }
    Ok(FundingSnapshot {
        rates: body.rates,
        stale: body.stale,
    })
}

pub async fn liquidity_depth(
    client: &Client,
    base: &str,
    token: &str,
    symbol: &str,
) -> Result<DepthAnalysis, String> {
    let res = client
        .get(depth_url(base, symbol))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|_| "liquidityUnreachable".to_string())?;
    if !res.status().is_success() {
        return Err(format!("liquidityRefused|{}", res.status().as_u16()));
    }
    res.json::<DepthAnalysis>()
        .await
        .map_err(|_| "liquidityMalformed".to_string())
}

pub async fn btc_macro(client: &Client, base: &str) -> Result<BtcMacro, String> {
    let res = client
        .get(format!("{base}/api/v1/market/btc-macro"))
        .send()
        .await
        .map_err(|_| "btcMacroUnreachable".to_string())?;
    if !res.status().is_success() {
        return Err(format!("btcMacroRefused|{}", res.status().as_u16()));
    }
    res.json::<BtcMacro>()
        .await
        .map_err(|_| "btcMacroMalformed".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FUNDING_BODY: &str = r#"{"source":"datahub.binance.futures.premiumIndex","available":true,"updated_at":"2026-10-01T12:00:00Z","count":2,"rates":[{"symbol":"BTCUSDT","rate":0.01,"next_funding":"2026-10-01T16:00:00Z","mark_price":60000.1,"index_price":59990,"funding_interval_hours":8,"annualized":10.95},{"symbol":"DOGEUSDT","rate":-0.05,"next_funding":"2026-10-01T16:00:00Z","mark_price":0.12,"index_price":0.1199,"funding_interval_hours":4,"annualized":-109.5}]}"#;

    const DEPTH_BODY: &str = r#"{"symbol":"SUIUSDT","bid_depth_usd":120000.5,"ask_depth_usd":80000.25,"imbalance":0.2,"spread_bps":1.5,"mid_price":1.2,"band_pct":1,"levels":1000,"truncated":true,"source":"datahub.binance.futures.depth","updated_at":"2026-10-01T12:00:00Z"}"#;

    #[test]
    fn routes_are_the_datahub_proxies() {
        assert_eq!(
            funding_url("https://ribqa.com"),
            "https://ribqa.com/api/v1/market/funding-rates"
        );
        assert_eq!(
            depth_url("https://ribqa.com", "SUIUSDT"),
            "https://ribqa.com/api/v1/market/depth/SUIUSDT"
        );
    }

    #[test]
    fn parses_the_funding_snapshot() {
        let body: FundingRatesResponse = serde_json::from_str(FUNDING_BODY).unwrap();
        let snap = usable_funding(body).unwrap();
        assert!(!snap.stale);
        let rates = snap.rates;
        assert_eq!(rates.len(), 2);
        let doge = rates.iter().find(|r| r.symbol == "DOGEUSDT").unwrap();
        assert!((doge.rate + 0.05).abs() < 1e-12, "rate stays in percent");
        assert!((doge.annualized + 109.5).abs() < 1e-9);
        assert_eq!(doge.next_funding, "2026-10-01T16:00:00Z");
    }

    #[test]
    fn a_stale_snapshot_is_used_but_unavailable_is_not() {
        let stale =
            FUNDING_BODY.replacen(r#""available":true"#, r#""available":true,"stale":true"#, 1);
        let body: FundingRatesResponse = serde_json::from_str(&stale).unwrap();
        let snap = usable_funding(body).unwrap();
        assert!(snap.stale);
        assert_eq!(snap.rates.len(), 2);

        let down: FundingRatesResponse = serde_json::from_str(
            r#"{"error":"funding source unavailable","source":"x","available":false}"#,
        )
        .unwrap();
        // A stable code the UI localizes (`errors.fundingUnavailable`).
        assert_eq!(usable_funding(down).err().as_deref(), Some("fundingUnavailable"));
    }

    #[test]
    fn parses_the_depth_body() {
        let d: DepthAnalysis = serde_json::from_str(DEPTH_BODY).unwrap();
        assert_eq!(d.symbol, "SUIUSDT");
        assert!((d.bid_depth_usd - 120_000.5).abs() < 1e-9);
        assert!((d.ask_depth_usd - 80_000.25).abs() < 1e-9);
        assert!((d.imbalance - 0.2).abs() < 1e-12);
        assert!((d.spread_bps - 1.5).abs() < 1e-12);
        assert!(d.truncated);
        // A body without the optional flag still parses.
        let plain: DepthAnalysis = serde_json::from_str(
            r#"{"symbol":"BTCUSDT","bid_depth_usd":1,"ask_depth_usd":2,"imbalance":0,"spread_bps":0}"#,
        )
        .unwrap();
        assert!(!plain.truncated);
    }
}
