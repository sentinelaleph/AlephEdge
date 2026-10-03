//! Binance's tradable USDT symbols, from the public exchangeInfo endpoints.
//! Feeds the per-coin take-profit picker: the list a user can choose from is
//! what the exchange actually trades, not only the coins that happen to have
//! a Sentinel signal open right now (an empty book used to mean an empty list).

use reqwest::Client;
use serde::Deserialize;

use super::binance_http::public_get;

const SPOT_EXCHANGE_INFO: &str = "https://api.binance.com/api/v3/exchangeInfo?permissions=SPOT";

#[derive(Deserialize)]
struct ExchangeInfo {
    symbols: Vec<SymbolInfo>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SymbolInfo {
    symbol: String,
    status: String,
    quote_asset: String,
    /// Futures only; absent on spot.
    #[serde(default)]
    contract_type: Option<String>,
}

/// Trading USDT symbols, sorted. Futures: perpetuals only.
pub fn parse_usdt_symbols(body: &str, futures: bool) -> Option<Vec<String>> {
    let info: ExchangeInfo = serde_json::from_str(body).ok()?;
    let mut out: Vec<String> = info
        .symbols
        .into_iter()
        .filter(|s| s.status == "TRADING" && s.quote_asset == "USDT")
        .filter(|s| !futures || s.contract_type.as_deref() == Some("PERPETUAL"))
        .map(|s| s.symbol)
        .collect();
    out.sort();
    out.dedup();
    Some(out)
}

/// Fetches the list; `None` on any network or parse failure (the picker then
/// falls back to the symbols of recent signals).
pub async fn fetch_binance_usdt_symbols(client: &Client, futures: bool) -> Option<Vec<String>> {
    let body = if futures {
        public_get(client, "/fapi/v1/exchangeInfo").await.ok()?
    } else {
        client
            .get(SPOT_EXCHANGE_INFO)
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?
            .text()
            .await
            .ok()?
    };
    parse_usdt_symbols(&body, futures)
}

#[cfg(test)]
mod tests {
    use super::parse_usdt_symbols;

    #[test]
    fn futures_keeps_trading_usdt_perpetuals_only() {
        let body = r#"{"symbols":[
            {"symbol":"SOLUSDT","status":"TRADING","quoteAsset":"USDT","contractType":"PERPETUAL"},
            {"symbol":"BTCUSDT_260925","status":"TRADING","quoteAsset":"USDT","contractType":"CURRENT_QUARTER"},
            {"symbol":"ETHBTC","status":"TRADING","quoteAsset":"BTC","contractType":"PERPETUAL"},
            {"symbol":"OLDUSDT","status":"SETTLING","quoteAsset":"USDT","contractType":"PERPETUAL"},
            {"symbol":"ADAUSDT","status":"TRADING","quoteAsset":"USDT","contractType":"PERPETUAL"}]}"#;
        assert_eq!(
            parse_usdt_symbols(body, true).unwrap(),
            vec!["ADAUSDT", "SOLUSDT"]
        );
    }

    #[test]
    fn spot_ignores_contract_type() {
        let body = r#"{"symbols":[
            {"symbol":"SOLUSDT","status":"TRADING","quoteAsset":"USDT"},
            {"symbol":"SOLBTC","status":"TRADING","quoteAsset":"BTC"},
            {"symbol":"LUNAUSDT","status":"BREAK","quoteAsset":"USDT"}]}"#;
        assert_eq!(parse_usdt_symbols(body, false).unwrap(), vec!["SOLUSDT"]);
    }

    #[test]
    fn garbage_is_none() {
        assert!(parse_usdt_symbols("not json", true).is_none());
    }
}
