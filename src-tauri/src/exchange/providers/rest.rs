//! Generic public-ticker fetcher: reads spot + futures last-price for any
//! `ExchangeSpec` and measures the spot call's round-trip as the health
//! signal. One code path serves every catalog exchange.

use reqwest::Client;
use serde_json::Value;
use std::time::Instant;

use super::super::model::{ExchangeSpec, ExchangeStatus, MarketSpec, Seg, SymbolStyle};

/// Spot + futures price for `symbol` on this exchange, plus the latency of
/// the first call (spot, or futures on a perpetual-only venue).
pub async fn fetch_status(client: &Client, spec: &ExchangeSpec, symbol: &str) -> ExchangeStatus {
    let started = Instant::now();
    let spot = match spec.spot.as_ref() {
        Some(m) => fetch_price(client, m, symbol).await,
        None => None,
    };
    let mut latency_ms = started.elapsed().as_millis() as u32;
    let futures = match spec.futures.as_ref() {
        Some(m) => fetch_price(client, m, symbol).await,
        None => None,
    };
    if spec.spot.is_none() {
        latency_ms = started.elapsed().as_millis() as u32;
    }
    let first = if spec.spot.is_some() { spot } else { futures };
    ExchangeStatus {
        healthy: first.is_some(),
        latency_ms: Some(latency_ms),
        spot_price: spot,
        futures_price: futures,
    }
}

async fn fetch_price(client: &Client, market: &MarketSpec, symbol: &str) -> Option<f64> {
    let formatted = format_symbol(symbol, market.style, market.suffix)?;
    let url = resolve_base(market.url).replace("{sym}", &formatted);
    let req = match market.post {
        Some(body) => client.post(url).header("Content-Type", "application/json").body(body),
        None => client.get(url),
    };
    let res = req.send().await.ok()?;
    if !res.status().is_success() {
        return None;
    }
    let body: Value = res.json().await.ok()?;
    extract(&body, market.price_path, &formatted).and_then(value_to_f64)
}

/// The Binance futures ticker follows the same base override as the order
/// path: a testnet build must manage positions on testnet prices, not on
/// production prices that can sit percent away from the testnet book.
fn resolve_base(url: &str) -> String {
    use crate::app::endpoints::{binance_futures_base, DEFAULT_BINANCE_FUTURES_BASE};
    match url.strip_prefix(DEFAULT_BINANCE_FUTURES_BASE) {
        Some(rest) => format!("{}{rest}", binance_futures_base()),
        None => url.to_string(),
    }
}

/// Formats a canonical `BASE`+`USDT` symbol for the exchange. Signal
/// symbols are USDT-quoted, so the quote split is a suffix strip; USDC and
/// USD venues read the same base coin's perpetual.
pub(crate) fn format_symbol(symbol: &str, style: SymbolStyle, suffix: &str) -> Option<String> {
    let base = symbol.strip_suffix("USDT").filter(|b| !b.is_empty())?;
    let xbt = if base == "BTC" { "XBT" } else { base };
    let core = match style {
        SymbolStyle::Plain => format!("{base}USDT"),
        SymbolStyle::Dash => format!("{base}-USDT"),
        SymbolStyle::Underscore => format!("{base}_USDT"),
        SymbolStyle::UnderscoreLower => format!("{base}_USDT").to_lowercase(),
        SymbolStyle::PlainLower => format!("{base}usdt").to_lowercase(),
        SymbolStyle::KucoinPerp => format!("{xbt}USDTM"),
        SymbolStyle::WooPerp => format!("PERP_{base}_USDT"),
        SymbolStyle::WooSpot => format!("SPOT_{base}_USDT"),
        SymbolStyle::KrakenPerp => format!("PF_{xbt}USD"),
        SymbolStyle::DeribitUsdc => format!("{base}_USDC-PERPETUAL"),
        SymbolStyle::BitfinexPerp => format!("t{base}F0:USTF0"),
        SymbolStyle::BitfinexSpot if base.len() > 3 => format!("t{base}:UST"),
        SymbolStyle::BitfinexSpot => format!("t{base}UST"),
        SymbolStyle::Base => base.to_string(),
    };
    Some(format!("{core}{suffix}"))
}

fn extract<'a>(value: &'a Value, path: &[Seg], symbol: &str) -> Option<&'a Value> {
    let mut cur = value;
    for seg in path {
        cur = match seg {
            Seg::Key(k) => cur.get(k)?,
            Seg::First => cur.get(0)?,
            Seg::SymbolKey => cur.get(symbol)?,
            Seg::Index(i) => cur.get(*i)?,
        };
    }
    Some(cur)
}

/// Ticker prices come as either JSON strings ("62266.7") or numbers (62250.4);
/// accept both.
fn value_to_f64(value: &Value) -> Option<f64> {
    match value {
        Value::String(s) => s.parse::<f64>().ok(),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::CATALOG;
    use super::*;

    #[test]
    fn every_one_of_the_15_venues_has_a_perpetual_price() {
        for v in crate::exchange::venue::caps::VENUES {
            let spec = CATALOG.iter().find(|e| e.id == v.id).unwrap_or_else(|| panic!("{} missing from the catalog", v.id));
            assert!(spec.futures.is_some(), "{}", v.id);
            assert_eq!(spec.name, v.name, "{}", v.id);
        }
        // The price-data / paper venues read plain public REST tickers.
        for (id, name, _) in crate::exchange::venue::caps::PAPER_VENUES {
            let spec = CATALOG.iter().find(|e| e.id == id).unwrap_or_else(|| panic!("{id} missing from the catalog"));
            assert!(spec.futures.is_some(), "{id}");
            assert_eq!(spec.name, name, "{id}");
        }
    }

    #[test]
    fn symbols_are_written_the_way_each_venue_reads_them() {
        let f = |style| format_symbol("BTCUSDT", style, "").unwrap();
        assert_eq!(f(SymbolStyle::PlainLower), "btcusdt");
        assert_eq!(f(SymbolStyle::KucoinPerp), "XBTUSDTM");
        assert_eq!(format_symbol("ETHUSDT", SymbolStyle::KucoinPerp, ""), Some("ETHUSDTM".into()));
        assert_eq!(f(SymbolStyle::WooPerp), "PERP_BTC_USDT");
        assert_eq!(f(SymbolStyle::WooSpot), "SPOT_BTC_USDT");
        assert_eq!(f(SymbolStyle::KrakenPerp), "PF_XBTUSD");
        assert_eq!(f(SymbolStyle::DeribitUsdc), "BTC_USDC-PERPETUAL");
        assert_eq!(f(SymbolStyle::BitfinexPerp), "tBTCF0:USTF0");
        assert_eq!(f(SymbolStyle::BitfinexSpot), "tBTCUST");
        assert_eq!(format_symbol("DOGEUSDT", SymbolStyle::BitfinexSpot, ""), Some("tDOGE:UST".into()));
        assert_eq!(f(SymbolStyle::Base), "BTC");
        assert_eq!(format_symbol("USDT", SymbolStyle::Base, ""), None);
    }

    /// Answers recorded from each public API on 22 Oct 2026 (trimmed).
    #[test]
    fn price_paths_read_recorded_answers() {
        let cases: [(&str, bool, &str, f64); 13] = [
            ("bitget", true, r#"{"code":"00000","data":[{"symbol":"BTCUSDT","lastPr":"81781.9"}]}"#, 81781.9),
            ("gate", true, r#"[{"last":"81774.7","contract":"BTC_USDT"}]"#, 81774.7),
            ("kucoinfutures", true, r#"{"code":"200000","data":{"symbol":"XBTUSDTM","price":"81790.6"}}"#, 81790.6),
            ("kucoinfutures", false, r#"{"code":"200000","data":{"price":"81821","size":"0.0025"}}"#, 81821.0),
            ("htx", true, r#"{"status":"ok","tick":{"close":"81788.8"}}"#, 81788.8),
            ("htx", false, r#"{"status":"ok","tick":{"close":81820.9}}"#, 81820.9),
            ("bingx", true, r#"{"code":0,"data":{"symbol":"BTC-USDT","price":"81769.4"}}"#, 81769.4),
            ("bingx", false, r#"{"code":0,"data":[{"symbol":"BTC_USDT","trades":[{"price":"81808.06"}]}]}"#, 81808.06),
            ("phemex", true, r#"{"error":null,"result":{"lastRp":"81767.8","symbol":"BTCUSDT"}}"#, 81767.8),
            ("woo", true, r#"{"success":true,"rows":[{"symbol":"PERP_BTC_USDT","executed_price":81766}]}"#, 81766.0),
            ("krakenfutures", true, r#"{"result":"success","ticker":{"symbol":"PF_XBTUSD","last":81718}}"#, 81718.0),
            ("deribit", true, r#"{"jsonrpc":"2.0","result":{"instrument_name":"BTC_USDC-PERPETUAL","last_price":81735.2}}"#, 81735.2),
            ("bitfinex", true, r#"[81850,3.9,81867,3.6,-1811,-0.02,81866,350.1,84962,81694,1562164542332]"#, 81866.0),
        ];
        for (id, futures, body, want) in cases {
            let spec = CATALOG.iter().find(|e| e.id == id).unwrap();
            let m = if futures { spec.futures.unwrap() } else { spec.spot.unwrap() };
            let sym = format_symbol("BTCUSDT", m.style, m.suffix).unwrap();
            let body: Value = serde_json::from_str(body).unwrap();
            assert_eq!(extract(&body, m.price_path, &sym).and_then(value_to_f64), Some(want), "{id} futures={futures}");
        }
        // Hyperliquid: a map of every coin's mid, read by the bare coin.
        let hl = CATALOG.iter().find(|e| e.id == "hyperliquid").unwrap().futures.unwrap();
        assert!(hl.post.is_some());
        let body: Value = serde_json::from_str(r##"{"#114910":"0.5","BTC":"81787.5","ETH":"3990.1"}"##).unwrap();
        assert_eq!(extract(&body, hl.price_path, &format_symbol("BTCUSDT", hl.style, "").unwrap()).and_then(value_to_f64), Some(81787.5));
    }
}
