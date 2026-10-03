//! Generic public-ticker fetcher: reads spot + futures last-price for any
//! `ExchangeSpec` and measures the spot call's round-trip as the health
//! signal. One code path serves every catalog exchange.

use reqwest::Client;
use serde_json::Value;
use std::time::Instant;

use super::super::model::{ExchangeSpec, ExchangeStatus, MarketSpec, Seg, SymbolStyle};

/// Spot + futures price for `symbol` on this exchange, plus spot latency.
pub async fn fetch_status(client: &Client, spec: &ExchangeSpec, symbol: &str) -> ExchangeStatus {
    let started = Instant::now();
    let spot = fetch_price(client, &spec.spot, symbol).await;
    let latency_ms = started.elapsed().as_millis() as u32;
    let futures = match spec.futures.as_ref() {
        Some(m) => fetch_price(client, m, symbol).await,
        None => None,
    };
    ExchangeStatus {
        healthy: spot.is_some(),
        latency_ms: Some(latency_ms),
        spot_price: spot,
        futures_price: futures,
    }
}

async fn fetch_price(client: &Client, market: &MarketSpec, symbol: &str) -> Option<f64> {
    let formatted = format_symbol(symbol, market.style, market.suffix)?;
    let url = resolve_base(market.url).replace("{sym}", &formatted);
    let res = client.get(url).send().await.ok()?;
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

/// Formats a canonical `BASE`+`USDT` symbol for the exchange. All catalog
/// symbols are USDT-quoted, so the quote split is a suffix strip.
fn format_symbol(symbol: &str, style: SymbolStyle, suffix: &str) -> Option<String> {
    let base = symbol.strip_suffix("USDT")?;
    let core = match style {
        SymbolStyle::Plain => format!("{base}USDT"),
        SymbolStyle::Dash => format!("{base}-USDT"),
        SymbolStyle::Underscore => format!("{base}_USDT"),
        SymbolStyle::UnderscoreLower => format!("{base}_USDT").to_lowercase(),
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
