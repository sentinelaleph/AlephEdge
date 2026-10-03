//! Historical candles for backtests, from Sentinel's DataHub through the
//! Sentinel API (never from an exchange directly):
//!
//!   GET {base}/api/v1/market/candles/history
//!       ?symbol=BTCUSDT&market=spot|futures&interval=15m|1h|4h|1d
//!       &start=<RFC3339>&end=<RFC3339>                         (auth)
//!
//! At most 9000 candles per request; the caller pages longer windows. Error
//! bodies are `{"error": "..."}` in principle, but the shape is not trusted:
//! anything unreadable keeps only the status code.

use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::time::Duration;

use crate::bot::strategy::backtest::Candle;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub fn build_client() -> Client {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq)]
pub enum HistoryError {
    /// 401: the session token was refused.
    Unauthorized,
    /// 404: the route (or the series) does not exist on this deployment.
    NotAvailable,
    RateLimited,
    /// 400 with the server's message, if it sent one.
    BadRequest(Option<String>),
    /// 5xx / 502 or any other status, with the server's message.
    Upstream(u16, Option<String>),
    Network,
    BadResponse,
}

impl HistoryError {
    /// "code" or "code|detail" for the UI (`backtest.errors.*`).
    pub fn to_code(&self) -> String {
        let with = |code: &str, d: &Option<String>| match d {
            Some(m) if !m.is_empty() => format!("{code}|{m}"),
            _ => code.to_string(),
        };
        match self {
            HistoryError::Unauthorized => "historyUnauthorized".into(),
            HistoryError::NotAvailable => "historyNotAvailable".into(),
            HistoryError::RateLimited => "historyRateLimited".into(),
            HistoryError::BadRequest(m) => with("historyBadRequest", m),
            HistoryError::Upstream(s, m) => with("historyUpstream", &Some(match m {
                Some(m) if !m.is_empty() => format!("{s} {m}"),
                _ => s.to_string(),
            })),
            HistoryError::Network => "historyNetwork".into(),
            HistoryError::BadResponse => "historyBadResponse".into(),
        }
    }
}

#[derive(Deserialize)]
struct RawCandle {
    ts: String,
    o: f64,
    h: f64,
    l: f64,
    c: f64,
}

#[derive(Deserialize, Default, Clone, Copy)]
pub struct Coverage {
    #[serde(default)]
    pub candle_count: Option<u64>,
    #[serde(default)]
    pub expected_candles: Option<u64>,
    #[serde(default)]
    pub coverage_pct: Option<f64>,
}

#[derive(Deserialize)]
struct RawPage {
    #[serde(default)]
    source: Option<String>,
    /// Go encodes an empty (nil) slice as `null`: a window with no candles
    /// (before a listing, a delisted pair) arrives as `"candles": null`.
    #[serde(default, deserialize_with = "null_as_empty")]
    candles: Vec<RawCandle>,
    #[serde(default)]
    coverage: Option<Coverage>,
}

fn null_as_empty<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

/// One parsed page. `unparsed` counts candles whose timestamp was unreadable.
pub struct Page {
    pub source: Option<String>,
    pub candles: Vec<Candle>,
    pub coverage: Option<Coverage>,
    pub unparsed: u32,
}

/// The server's error message from a JSON body, if it has one we can read.
fn error_message(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let m = v
        .get("error")
        .or_else(|| v.get("message"))
        .and_then(|e| e.as_str().map(str::to_string).or_else(|| e.get("message").and_then(|m| m.as_str()).map(str::to_string)))?;
    // A short, single-line message only; never echo a page of HTML.
    let m: String = m.chars().filter(|c| !c.is_control()).take(160).collect();
    (!m.trim().is_empty()).then_some(m)
}

pub fn parse_page(body: &str) -> Result<Page, HistoryError> {
    let raw: RawPage = serde_json::from_str(body).map_err(|_| HistoryError::BadResponse)?;
    let mut unparsed = 0;
    let candles = raw
        .candles
        .into_iter()
        .filter_map(|k| match crate::signal::time::parse_rfc3339_ms(&k.ts) {
            Some(open_ms) => Some(Candle { open_ms, o: k.o, h: k.h, l: k.l, c: k.c }),
            None => {
                unparsed += 1;
                None
            }
        })
        .collect();
    Ok(Page {
        source: raw.source.filter(|s| !s.trim().is_empty()),
        candles,
        coverage: raw.coverage,
        unparsed,
    })
}

pub fn classify(status: StatusCode, body: &str) -> HistoryError {
    let msg = error_message(body);
    match status.as_u16() {
        401 | 403 => HistoryError::Unauthorized,
        404 => HistoryError::NotAvailable,
        429 => HistoryError::RateLimited,
        400 => HistoryError::BadRequest(msg),
        s => HistoryError::Upstream(s, msg),
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn fetch_page(
    client: &Client,
    base: &str,
    token: &str,
    symbol: &str,
    market: &str,
    interval: &str,
    start: &str,
    end: &str,
) -> Result<Page, HistoryError> {
    let res = client
        .get(format!("{base}/api/v1/market/candles/history"))
        .query(&[
            ("symbol", symbol),
            ("market", market),
            ("interval", interval),
            ("start", start),
            ("end", end),
        ])
        .bearer_auth(token)
        .send()
        .await
        .map_err(|_| HistoryError::Network)?;
    let status = res.status();
    let body = res.text().await.map_err(|_| HistoryError::Network)?;
    if !status.is_success() {
        return Err(classify(status, &body));
    }
    parse_page(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_parses_and_bad_timestamps_are_counted() {
        let body = r#"{"source":"datahub","candles":[
            {"ts":"2025-01-06T00:00:00Z","o":1,"h":2,"l":0.5,"c":1.5,"v":10,"symbol":"BTCUSDT","interval":"1h","precision":2},
            {"ts":"nope","o":1,"h":2,"l":0.5,"c":1.5,"v":10}
        ],"coverage":{"candle_count":1,"expected_candles":2,"coverage_pct":50.0,"extra":true}}"#;
        let p = parse_page(body).unwrap();
        assert_eq!(p.source.as_deref(), Some("datahub"));
        assert_eq!(p.candles.len(), 1);
        assert_eq!(p.candles[0].open_ms, 1_736_121_600_000);
        assert_eq!(p.unparsed, 1);
        let cov = p.coverage.unwrap();
        assert_eq!(cov.expected_candles, Some(2));
        assert_eq!(cov.coverage_pct, Some(50.0));
        assert!(parse_page("<html>").is_err());
        assert!(parse_page(r#"{"candles":[]}"#).unwrap().candles.is_empty());
    }

    #[test]
    fn errors_are_classified_defensively() {
        assert_eq!(classify(StatusCode::NOT_FOUND, "404 page not found"), HistoryError::NotAvailable);
        assert_eq!(classify(StatusCode::UNAUTHORIZED, ""), HistoryError::Unauthorized);
        assert_eq!(classify(StatusCode::TOO_MANY_REQUESTS, "{}"), HistoryError::RateLimited);
        assert_eq!(
            classify(StatusCode::BAD_REQUEST, r#"{"error":"span too long"}"#),
            HistoryError::BadRequest(Some("span too long".into()))
        );
        assert_eq!(
            classify(StatusCode::BAD_REQUEST, r#"{"error":{"message":"bad symbol"}}"#),
            HistoryError::BadRequest(Some("bad symbol".into()))
        );
        assert_eq!(classify(StatusCode::BAD_GATEWAY, "<html>oops</html>"), HistoryError::Upstream(502, None));
        assert_eq!(HistoryError::Upstream(502, None).to_code(), "historyUpstream|502");
        assert_eq!(HistoryError::NotAvailable.to_code(), "historyNotAvailable");
    }

    /// A real `GET /api/v1/market/candles/history` body (BTCUSDT futures 1h,
    /// 2024-10-01..2025-10-01, recorded 1 Oct 2026), trimmed to its first
    /// three candles; the coverage object is the server's, untouched.
    const REAL_BODY: &str = r#"{"source":"datahub.binance.futures.history","candles":[{"ts":"2024-10-01T00:00:00Z","o":63309,"h":63601.7,"l":63000,"c":63513.2,"v":13510.1,"symbol":"BTCUSDT","interval":"1h","precision":0},{"ts":"2024-10-01T01:00:00Z","o":63513.2,"h":63627.5,"l":63345.7,"c":63447.3,"v":6383.848,"symbol":"BTCUSDT","interval":"1h","precision":0},{"ts":"2024-10-01T02:00:00Z","o":63447.2,"h":63447.2,"l":63150,"c":63427.8,"v":4795.932,"symbol":"BTCUSDT","interval":"1h","precision":0}],"coverage":{"source":"datahub.binance.futures.history","symbol":"BTCUSDT","interval":"1h","requested_start":"2024-10-01T00:00:00Z","requested_end":"2025-10-01T00:00:00Z","actual_start":"2024-10-01T00:00:00Z","actual_end":"2025-09-30T23:00:00Z","candle_count":8760,"expected_candles":8760,"coverage_pct":100}}"#;

    #[test]
    fn a_real_datahub_body_parses_through_the_app_parser() {
        let p = parse_page(REAL_BODY).unwrap();
        assert_eq!(p.source.as_deref(), Some("datahub.binance.futures.history"));
        assert_eq!(p.unparsed, 0);
        let t0 = crate::signal::time::parse_rfc3339_ms("2024-10-01T00:00:00Z").unwrap();
        assert_eq!(
            p.candles,
            vec![
                Candle { open_ms: t0, o: 63309.0, h: 63601.7, l: 63000.0, c: 63513.2 },
                Candle { open_ms: t0 + 3_600_000, o: 63513.2, h: 63627.5, l: 63345.7, c: 63447.3 },
                Candle { open_ms: t0 + 7_200_000, o: 63447.2, h: 63447.2, l: 63150.0, c: 63427.8 },
            ]
        );
        let cov = p.coverage.unwrap();
        // `coverage_pct` arrives as the integer `100`.
        assert_eq!((cov.candle_count, cov.expected_candles, cov.coverage_pct), (Some(8760), Some(8760), Some(100.0)));
        // Those candles become bars untouched.
        let (bars, dropped) = crate::bot::strategy::backtest::candles_to_bars(p.candles, 3_600_000, u64::MAX);
        assert_eq!((bars.len(), dropped), (3, 0));
    }

    /// DataHub builds `candles` from a nil Go slice, so a window with no
    /// klines (before a listing, a delisted pair) is `"candles":null`. That is
    /// an empty page, not an unreadable response: a long window whose first
    /// pages predate the listing must still run.
    #[test]
    fn a_null_candle_list_is_an_empty_page() {
        let body = r#"{"source":"datahub.binance.futures.history","candles":null,"coverage":{"source":"datahub.binance.futures.history","symbol":"NEWUSDT","interval":"1h","requested_start":"2024-01-01T00:00:00Z","requested_end":"2024-02-01T00:00:00Z","actual_start":"0001-01-01T00:00:00Z","actual_end":"0001-01-01T00:00:00Z","candle_count":0,"expected_candles":744,"coverage_pct":0}}"#;
        let p = parse_page(body).expect("null candles = empty page");
        assert!(p.candles.is_empty());
        assert_eq!(p.coverage.unwrap().expected_candles, Some(744));
    }
}
