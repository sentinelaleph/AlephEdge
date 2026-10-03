//! The exchange catalog: every Tier-1 (USDT-quoted) exchange as an
//! `ExchangeSpec`. URLs and price paths here were verified against the live
//! public ticker APIs, not guessed.
//!
//! Exclusions (PRD Tier 2/3) follow one honesty rule — an exchange is only
//! listed if it has a REAL USDT-quoted market matching Sentinel's USDT signal
//! geometry. BTCTurk + Paribu are TRY-quoted (user decision, F4). Mercado
//! Bitcoin has a BTC-USDT pair but it's a ghost market (near-zero volume; real
//! liquidity is BRL), so a fill there is fiction. Ripio has no real USDT pair
//! (BRL/ARS only). CoinDCX (India) and Bitso (Mexico/LatAm) DO have live USDT
//! liquidity, so they're in — both spot-only (no USDT-M futures wired).

mod binance_auth;
pub mod binance_fills;
pub(crate) mod binance_http;
pub mod binance_parse;
pub mod binance_requests;
pub mod binance_rules;
pub mod binance_symbols;
pub mod klines;
mod rest;

pub use binance_auth::{check_binance_permissions, fetch_binance_futures_account};
pub use rest::fetch_status;

use super::model::{ExchangeSpec, MarketSpec, Seg, SymbolStyle};

const BINANCE_PRICE: &[Seg] = &[Seg::Key("price")];
const OKX_PRICE: &[Seg] = &[Seg::Key("data"), Seg::First, Seg::Key("last")];
const BYBIT_PRICE: &[Seg] = &[
    Seg::Key("result"),
    Seg::Key("list"),
    Seg::First,
    Seg::Key("lastPrice"),
];
const MEXC_FUT_PRICE: &[Seg] = &[Seg::Key("data"), Seg::Key("lastPrice")];
const BITSO_PRICE: &[Seg] = &[Seg::Key("payload"), Seg::Key("last")];
const COINDCX_PRICE: &[Seg] = &[Seg::SymbolKey];

/// All catalog exchanges. Order is the health-strip display order.
pub const CATALOG: &[ExchangeSpec] = &[
    ExchangeSpec {
        id: "binance",
        name: "Binance",
        spot: MarketSpec {
            url: "https://api.binance.com/api/v3/ticker/price?symbol={sym}",
            style: SymbolStyle::Plain,
            suffix: "",
            price_path: BINANCE_PRICE,
        },
        futures: Some(MarketSpec {
            url: "https://fapi.binance.com/fapi/v1/ticker/price?symbol={sym}",
            style: SymbolStyle::Plain,
            suffix: "",
            price_path: BINANCE_PRICE,
        }),
    },
    ExchangeSpec {
        id: "okx",
        name: "OKX",
        spot: MarketSpec {
            url: "https://www.okx.com/api/v5/market/ticker?instId={sym}",
            style: SymbolStyle::Dash,
            suffix: "",
            price_path: OKX_PRICE,
        },
        futures: Some(MarketSpec {
            url: "https://www.okx.com/api/v5/market/ticker?instId={sym}",
            style: SymbolStyle::Dash,
            suffix: "-SWAP",
            price_path: OKX_PRICE,
        }),
    },
    ExchangeSpec {
        id: "bybit",
        name: "Bybit",
        spot: MarketSpec {
            url: "https://api.bybit.com/v5/market/tickers?category=spot&symbol={sym}",
            style: SymbolStyle::Plain,
            suffix: "",
            price_path: BYBIT_PRICE,
        },
        futures: Some(MarketSpec {
            url: "https://api.bybit.com/v5/market/tickers?category=linear&symbol={sym}",
            style: SymbolStyle::Plain,
            suffix: "",
            price_path: BYBIT_PRICE,
        }),
    },
    ExchangeSpec {
        id: "mexc",
        name: "MEXC",
        spot: MarketSpec {
            url: "https://api.mexc.com/api/v3/ticker/price?symbol={sym}",
            style: SymbolStyle::Plain,
            suffix: "",
            price_path: BINANCE_PRICE,
        },
        futures: Some(MarketSpec {
            url: "https://contract.mexc.com/api/v1/contract/ticker?symbol={sym}",
            style: SymbolStyle::Underscore,
            suffix: "",
            price_path: MEXC_FUT_PRICE,
        }),
    },
    // ---- Tier 3 regional (spot-only, real USDT liquidity) ----
    ExchangeSpec {
        id: "coindcx",
        name: "CoinDCX",
        // `current_prices` is a flat map keyed by market name (e.g. "BTCUSDT").
        spot: MarketSpec {
            url: "https://public.coindcx.com/market_data/current_prices",
            style: SymbolStyle::Plain,
            suffix: "",
            price_path: COINDCX_PRICE,
        },
        futures: None,
    },
    ExchangeSpec {
        id: "bitso",
        name: "Bitso",
        spot: MarketSpec {
            url: "https://api.bitso.com/v3/ticker/?book={sym}",
            style: SymbolStyle::UnderscoreLower,
            suffix: "",
            price_path: BITSO_PRICE,
        },
        futures: None,
    },
];

/// Finds a catalog exchange by id.
pub fn find(id: &str) -> Option<&'static ExchangeSpec> {
    CATALOG.iter().find(|e| e.id == id)
}
