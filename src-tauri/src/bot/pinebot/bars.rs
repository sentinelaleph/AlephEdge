//! Closed bars for the Pine bot (spec section 5, decision D2): public,
//! keyless Binance USDT-M klines on the bot's timeframe, the same
//! execution-side feed and base URL as the paper DCA / Grid engine
//! (`exchange::providers::klines`, which reads 1m only). Never signs a
//! request or sends a key. A bar still forming at `now` is dropped, so the
//! script never sees a bar that can change.

use reqwest::Client;
use serde_json::Value;

use crate::bot::model::PineTimeframe;
use crate::pine::PineBar;

/// Binance's largest futures kline page.
pub const PAGE: u32 = 1_500;

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

/// Parses one kline page, oldest first, dropping any bar not closed at
/// `now_ms` (Binance's close time is the last ms of the bar).
pub fn parse(body: &Value, now_ms: u64) -> Result<Vec<PineBar>, String> {
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
        let close_ms = a[6].as_u64().ok_or("kline close time")?;
        if close_ms >= now_ms {
            continue; // still forming
        }
        out.push(PineBar {
            t: a[0].as_u64().ok_or("kline open time")?,
            o: num(&a[1]).ok_or("kline open")?,
            h: num(&a[2]).ok_or("kline high")?,
            l: num(&a[3]).ok_or("kline low")?,
            c: num(&a[4]).ok_or("kline close")?,
            v: num(&a[5]).ok_or("kline volume")?,
        });
    }
    Ok(out)
}

/// One page: up to `limit` bars ending at `end_ms` (or the newest).
pub fn request(client: &Client, symbol: &str, tf: PineTimeframe, end_ms: Option<u64>, limit: u32) -> reqwest::RequestBuilder {
    let mut q: Vec<(&str, String)> = vec![("symbol", symbol.to_string()), ("interval", tf.as_str().to_string()), ("limit", limit.clamp(1, PAGE).to_string())];
    if let Some(e) = end_ms {
        q.push(("endTime", e.to_string()));
    }
    client.get(format!("{}/fapi/v1/klines", crate::app::endpoints::binance_futures_base())).query(&q)
}

/// Merges an older page in front of `bars` (both oldest first), without
/// repeating a bar.
pub fn prepend(older: Vec<PineBar>, bars: Vec<PineBar>) -> Vec<PineBar> {
    let first = bars.first().map_or(u64::MAX, |b| b.t);
    let mut out: Vec<PineBar> = older.into_iter().filter(|b| b.t < first).collect();
    out.extend(bars);
    out
}

/// The newest `count` closed bars of `symbol`, oldest first, paging back
/// with `endTime` past Binance's 1,500-bar page.
pub async fn fetch(client: &Client, symbol: &str, tf: PineTimeframe, count: u32, now_ms: u64) -> Result<Vec<PineBar>, String> {
    let read = |end: Option<u64>, limit: u32| async move {
        let res = request(client, symbol, tf, end, limit).send().await.map_err(|_| "pineBarsUnavailable".to_string())?;
        let body: Value = res.json().await.map_err(|_| "pineBarsUnavailable".to_string())?;
        parse(&body, now_ms).map_err(|m| format!("pineBarsFailed|{m}"))
    };
    // One more than asked: the forming bar comes back last and is dropped.
    let mut bars = read(None, (count + 1).min(PAGE)).await?;
    while (bars.len() as u32) < count {
        let Some(first) = bars.first().map(|b| b.t) else {
            break;
        };
        let want = (count - bars.len() as u32).min(PAGE);
        let older = read(Some(first.saturating_sub(1)), want).await?;
        if older.is_empty() {
            break; // a young symbol: fewer bars than asked
        }
        let before = bars.len();
        bars = prepend(older, bars);
        if bars.len() == before {
            break;
        }
    }
    let skip = bars.len().saturating_sub(count as usize);
    Ok(bars.split_off(skip))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const H: u64 = 3_600_000;

    fn row(open: u64, iv: u64) -> Value {
        json!([open, "100.0", "101.0", "99.0", "100.5", "12.5", open + iv - 1, "0", 1, "0", "0", "0"])
    }

    #[test]
    fn only_closed_bars_reach_the_script() {
        let t = 1_761_897_600_000u64; // an hour boundary
        let body = json!([row(t - H, H), row(t, H)]);
        // 5 s after the boundary: the bar that opened at t is still forming.
        let bars = parse(&body, t + 5_000).unwrap();
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].t, t - H);
        assert_eq!((bars[0].o, bars[0].c, bars[0].v), (100.0, 100.5, 12.5));
        // Exactly at the close time's last millisecond it is not closed yet.
        assert_eq!(parse(&json!([row(t, H)]), t + H - 1).unwrap().len(), 0);
        assert_eq!(parse(&json!([row(t, H)]), t + H).unwrap().len(), 1);
        assert_eq!(parse(&json!({"code": -1121, "msg": "Invalid symbol."}), t).unwrap_err(), "Invalid symbol.");
        assert!(parse(&json!([[1, "1"]]), t).is_err());
    }

    #[test]
    fn pages_join_without_repeating_a_bar() {
        let b = |t: u64| PineBar { t, o: 1.0, h: 1.0, l: 1.0, c: 1.0, v: 0.0 };
        let joined = prepend(vec![b(1), b(2), b(3)], vec![b(3), b(4)]);
        assert_eq!(joined.iter().map(|x| x.t).collect::<Vec<_>>(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn requests_name_the_timeframe_and_are_keyless() {
        let client = Client::new();
        let req = request(&client, "BTCUSDT", PineTimeframe::H4, Some(99), 5_000).build().unwrap();
        let q = req.url().query().unwrap();
        assert!(q.contains("interval=4h") && q.contains("limit=1500") && q.contains("endTime=99"), "{q}");
        assert!(req.url().path().ends_with("/fapi/v1/klines"));
        assert!(req.headers().get("X-MBX-APIKEY").is_none() && !q.contains("signature"));
    }
}
