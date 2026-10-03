//! Exchange adapter types. Our own design (camelCase), not a wire mirror.
//!
//! The 6 near-identical public-ticker adapters (PRD §3.4 Tier 1) are expressed
//! as DATA — `ExchangeSpec` rows in `providers::catalog` — rather than one
//! copy-pasted file each: a single generic fetcher (`providers::rest`) reads
//! any of them. Only Binance's signed permission check needs bespoke code.

use serde::{Deserialize, Serialize};

/// A snapshot of one exchange's reachability + live prices for a symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeStatus {
    pub healthy: bool,
    pub latency_ms: Option<u32>,
    pub spot_price: Option<f64>,
    pub futures_price: Option<f64>,
}

/// Secret-free catalog entry for the UI (exchange picker, health strip).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeInfo {
    pub id: String,
    pub name: String,
    pub supports_futures: bool,
}

/// How an exchange writes a `BASE`/`USDT` pair in its ticker URL.
#[derive(Debug, Clone, Copy)]
pub enum SymbolStyle {
    /// BTCUSDT (Binance, Bybit, MEXC spot, CoinDCX).
    Plain,
    /// BTC-USDT (OKX, Mercado).
    Dash,
    /// BTC_USDT (MEXC futures).
    Underscore,
    /// btc_usdt (Bitso).
    UnderscoreLower,
}

/// One step of a price-extraction path through the JSON response.
#[derive(Debug, Clone, Copy)]
pub enum Seg {
    Key(&'static str),
    /// First element of an array.
    First,
    /// Index a map by the exchange-formatted symbol (e.g. CoinDCX's
    /// `current_prices` is keyed by market name).
    SymbolKey,
}

/// How to reach and read one market (spot or futures) of an exchange.
#[derive(Debug, Clone, Copy)]
pub struct MarketSpec {
    /// URL template containing `{sym}` for the formatted symbol.
    pub url: &'static str,
    pub style: SymbolStyle,
    /// Appended after the formatted symbol, e.g. "-SWAP" for OKX perpetuals.
    pub suffix: &'static str,
    /// Path to the last-price value (string or number) in the response.
    pub price_path: &'static [Seg],
}

/// A Tier-1 exchange: id, display name, and its spot/futures market specs.
#[derive(Debug, Clone, Copy)]
pub struct ExchangeSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub spot: MarketSpec,
    /// None for spot-only exchanges (none in the current USDT-only set).
    pub futures: Option<MarketSpec>,
}

impl ExchangeSpec {
    pub fn supports_futures(&self) -> bool {
        self.futures.is_some()
    }

    pub fn info(&self) -> ExchangeInfo {
        ExchangeInfo {
            id: self.id.to_string(),
            name: self.name.to_string(),
            supports_futures: self.supports_futures(),
        }
    }
}

/// Real, API-verified permissions for a just-added key — never the user's
/// self-declared guess (PRD §5.5: "withdraw izinli anahtar tespit edilirse
/// uyarı").
#[derive(Debug, Clone, Copy)]
pub struct BinancePermissions {
    pub withdraw_enabled: bool,
    /// `enableFutures`: false = the key cannot place or close futures orders.
    pub futures_enabled: bool,
}

/// The connected exchange's USDT-M futures wallet snapshot, fetched with the
/// vaulted key. Read-only — the key never leaves the device; this is a plain
/// account view (balance + open positions), not an order path.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesAccount {
    pub total_wallet_balance: f64,
    pub available_balance: f64,
    pub total_unrealized_pnl: f64,
    pub positions: Vec<FuturesPosition>,
}

/// One open USDT-M position (only non-zero positions are returned).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesPosition {
    pub symbol: String,
    pub position_amt: f64,
    pub entry_price: f64,
    pub unrealized_pnl: f64,
}

/// Why a Binance call didn't produce a verified answer.
#[derive(Debug, Clone, PartialEq)]
pub enum BinanceKeyCheckError {
    /// Binance rejected the key/secret pair itself — almost always a typo.
    InvalidCredentials,
    /// Couldn't reach Binance at all; nothing was sent.
    NetworkUnavailable,
    /// Binance refused the request, with its own error code (-2019 margin,
    /// -2013 no such order, -1111 precision …). Nothing was executed.
    Rejected { code: i64, msg: String },
    /// Signed calls are on hold after a 429/418.
    RateLimited,
    /// Sent, but the outcome is unknown (5xx, timeout after send, unreadable
    /// 200). An order may have executed: look before acting.
    Unknown,
}
