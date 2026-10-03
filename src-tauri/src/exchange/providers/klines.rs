//! Public, keyless Binance klines and funding history: the execution-side
//! price data of the paper DCA / Grid engine. Never signs a request, never
//! sends an API key header, never touches the order path.
//!
//! Only CLOSED bars are returned: a bar whose close time is still in the
//! future is dropped, so a fill is never decided on a bar that can change.

use reqwest::{Client, RequestBuilder};
use serde_json::Value;

const SPOT_BASE: &str = "https://api.binance.com";
pub const MAX_LIMIT: u16 = 500;

/// One closed kline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Kline {
    pub open_ms: u64,
    pub close_ms: u64,
    pub o: f64,
    pub h: f64,
    pub l: f64,
    pub c: f64,
}

/// The futures base follows the same override as the ticker (a testnet build
/// manages paper cycles on testnet prices).
fn futures_base() -> String {
    crate::app::endpoints::binance_futures_base()
}

/// 1m klines from `start_ms` (inclusive), at most `limit` (<= 500).
pub fn klines_request(client: &Client, futures: bool, symbol: &str, start_ms: Option<u64>, limit: u16) -> RequestBuilder {
    let url = if futures {
        format!("{}/fapi/v1/klines", futures_base())
    } else {
        format!("{SPOT_BASE}/api/v3/klines")
    };
    let mut q: Vec<(&str, String)> = vec![
        ("symbol", symbol.to_string()),
        ("interval", "1m".to_string()),
        ("limit", limit.min(MAX_LIMIT).to_string()),
    ];
    if let Some(s) = start_ms {
        q.push(("startTime", s.to_string()));
    }
    client.get(url).query(&q)
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

/// Parses the kline array, dropping any bar not closed at `now_ms`.
pub fn parse_klines(body: &Value, now_ms: u64) -> Result<Vec<Kline>, String> {
    let Some(rows) = body.as_array() else {
        let msg = body.get("msg").and_then(|m| m.as_str()).unwrap_or("unexpected kline response");
        return Err(msg.to_string());
    };
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let a = r.as_array().ok_or("kline row is not an array")?;
        if a.len() < 7 {
            return Err("kline row too short".into());
        }
        let k = Kline {
            open_ms: a[0].as_u64().ok_or("kline open time")?,
            o: num(&a[1]).ok_or("kline open")?,
            h: num(&a[2]).ok_or("kline high")?,
            l: num(&a[3]).ok_or("kline low")?,
            c: num(&a[4]).ok_or("kline close")?,
            close_ms: a[6].as_u64().ok_or("kline close time")?,
        };
        if k.close_ms > now_ms {
            continue; // still forming
        }
        out.push(k);
    }
    Ok(out)
}

/// Closed 1m bars from `start_ms`.
pub async fn fetch_closed_klines(
    client: &Client,
    futures: bool,
    symbol: &str,
    start_ms: Option<u64>,
    now_ms: u64,
) -> Result<Vec<Kline>, String> {
    let res = klines_request(client, futures, symbol, start_ms, MAX_LIMIT)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body: Value = res.json().await.map_err(|e| e.to_string())?;
    let raw_len = body.as_array().map_or(0, Vec::len);
    let bars = parse_klines(&body, now_ms)?;
    Ok(drop_forming_tail(bars, raw_len, MAX_LIMIT, now_ms))
}

/// A response shorter than the limit reached Binance's present, and Binance
/// always returns the still-forming bar last. `close_ms > now` alone drops
/// it only while the local clock is not ahead of Binance's (this path never
/// syncs the clock); when nothing was dropped and the last bar closed less
/// than a minute ago, it may still be forming and is held for the next
/// tick (where it reappears before a newer bar if it really closed).
pub fn drop_forming_tail(mut bars: Vec<Kline>, raw_len: usize, limit: u16, now_ms: u64) -> Vec<Kline> {
    if raw_len < usize::from(limit)
        && raw_len == bars.len()
        && bars.last().is_some_and(|k| k.close_ms + 60_000 > now_ms)
    {
        bars.pop();
    }
    bars
}

/// Funding history (timestamp ms, rate as a fraction).
pub fn parse_funding(body: &Value) -> Result<Vec<(u64, f64)>, String> {
    let rows = body.as_array().ok_or("unexpected funding response")?;
    Ok(rows
        .iter()
        .filter_map(|r| Some((r.get("fundingTime")?.as_u64()?, num(r.get("fundingRate")?)?)))
        .collect())
}

pub fn funding_request(client: &Client, symbol: &str, start_ms: u64, end_ms: u64) -> RequestBuilder {
    client
        .get(format!("{}/fapi/v1/fundingRate", futures_base()))
        .query(&[
            ("symbol", symbol.to_string()),
            ("startTime", start_ms.to_string()),
            ("endTime", end_ms.to_string()),
            ("limit", "100".to_string()),
        ])
}

/// Futures funding events in [start, end].
pub async fn fetch_funding(client: &Client, symbol: &str, start_ms: u64, end_ms: u64) -> Result<Vec<(u64, f64)>, String> {
    let res = funding_request(client, symbol, start_ms, end_ms)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body: Value = res.json().await.map_err(|e| e.to_string())?;
    parse_funding(&body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_and_drops_the_open_bar() {
        let body = json!([
            [1_700_000_000_000u64, "100.0", "101.0", "99.0", "100.5", "12", 1_700_000_059_999u64, "0", 1, "0", "0", "0"],
            [1_700_000_060_000u64, "100.5", "100.9", "100.1", "100.2", "3", 1_700_000_119_999u64, "0", 1, "0", "0", "0"]
        ]);
        let k = parse_klines(&body, 1_700_000_100_000).unwrap();
        assert_eq!(k.len(), 1);
        assert_eq!(k[0].o, 100.0);
        assert_eq!(k[0].close_ms, 1_700_000_059_999);
        assert_eq!(parse_klines(&json!([]), 0).unwrap().len(), 0);
        assert_eq!(parse_klines(&json!({"code": -1121, "msg": "Invalid symbol."}), 0).unwrap_err(), "Invalid symbol.");
        assert!(parse_klines(&json!([[1, "1"]]), 0).is_err());
    }

    #[test]
    fn a_forming_bar_kept_by_a_fast_local_clock_is_held_back() {
        let k = |open: u64| Kline { open_ms: open, close_ms: open + 59_999, o: 1.0, h: 1.0, l: 1.0, c: 1.0 };
        let t0 = 1_700_000_040_000u64; // minute aligned
        // local clock 2s ahead: the bar opened at t0+60s looks closed at t0+120.5s
        let rows = vec![k(t0), k(t0 + 60_000)];
        let out = drop_forming_tail(rows.clone(), 2, 500, t0 + 120_500);
        assert_eq!(out, vec![k(t0)]);
        // the clock already dropped the forming bar: nothing more is held
        assert_eq!(drop_forming_tail(vec![k(t0)], 2, 500, t0 + 100_000), vec![k(t0)]);
        // a full page is backlog, not the present
        assert_eq!(drop_forming_tail(rows.clone(), 500, 500, t0 + 120_500).len(), 2);
        // an old last bar (halted symbol) is not held forever
        assert_eq!(drop_forming_tail(rows, 2, 500, t0 + 600_000).len(), 2);
    }

    #[test]
    fn parses_funding_history() {
        let body = json!([{"symbol": "BTCUSDT", "fundingTime": 1_700_006_400_001u64, "fundingRate": "0.00010000"}]);
        assert_eq!(parse_funding(&body).unwrap(), vec![(1_700_006_400_001, 0.0001)]);
    }

    #[test]
    fn requests_are_keyless_and_unsigned() {
        let client = Client::new();
        for req in [
            klines_request(&client, true, "BTCUSDT", Some(1), 500).build().unwrap(),
            klines_request(&client, false, "BTCUSDT", None, 900).build().unwrap(),
            funding_request(&client, "BTCUSDT", 1, 2).build().unwrap(),
        ] {
            assert!(req.headers().get("X-MBX-APIKEY").is_none());
            let q = req.url().query().unwrap_or("");
            assert!(!q.contains("signature") && !q.contains("timestamp"), "{q}");
        }
        let req = klines_request(&client, false, "BTCUSDT", None, 900).build().unwrap();
        assert!(req.url().query().unwrap().contains("limit=500"));
        assert!(req.url().as_str().starts_with("https://api.binance.com/api/v3/klines"));
    }
}
