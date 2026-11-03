//! The exchange catalog: every exchange whose prices the app reads, as an
//! `ExchangeSpec`. URLs and price paths here were verified against the live
//! public ticker APIs, not guessed. Signals are USDT pairs; venues without
//! USDT perpetuals read the same coin's USDC (Hyperliquid, Deribit) or USD
//! (Kraken Futures) perpetual. These plain REST tickers are the only data
//! path of the seven price-data / paper venues (`venue::caps::PAPER_VENUES`):
//! a ticker that does not answer marks the venue unhealthy, nothing is
//! filled in.
//!
//! Exclusions follow one honesty rule — an exchange is only listed if it has a
//! REAL USDT-quoted market matching Sentinel's USDT signal geometry. BTCTurk +
//! Paribu are TRY-quoted, so they are out. Mercado
//! Bitcoin has a BTC-USDT pair but it's a ghost market (near-zero volume; real
//! liquidity is BRL), so a fill there is fiction. Ripio has no real USDT pair
//! (BRL/ARS only). CoinDCX (India) and Bitso (Mexico/LatAm) DO have live USDT
//! liquidity, so they're in — both spot-only (no USDT-M futures wired).

mod binance_auth;
pub mod binance_fills;
pub mod binance_income;
pub(crate) mod binance_http;
pub mod binance_sign;
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
const BITGET_PRICE: &[Seg] = &[Seg::Key("data"), Seg::First, Seg::Key("lastPr")];
const GATE_PRICE: &[Seg] = &[Seg::First, Seg::Key("last")];
const KUCOIN_PRICE: &[Seg] = &[Seg::Key("data"), Seg::Key("price")];
const HTX_PRICE: &[Seg] = &[Seg::Key("tick"), Seg::Key("close")];
const BINGX_SPOT_PRICE: &[Seg] = &[Seg::Key("data"), Seg::First, Seg::Key("trades"), Seg::First, Seg::Key("price")];
const BINGX_SWAP_PRICE: &[Seg] = &[Seg::Key("data"), Seg::Key("price")];
const PHEMEX_PRICE: &[Seg] = &[Seg::Key("result"), Seg::Key("lastRp")];
const COINEX_PRICE: &[Seg] = &[Seg::Key("data"), Seg::First, Seg::Key("last")];
const WOO_PRICE: &[Seg] = &[Seg::Key("rows"), Seg::First, Seg::Key("executed_price")];
const HYPERLIQUID_PRICE: &[Seg] = &[Seg::SymbolKey];
const KRAKEN_PRICE: &[Seg] = &[Seg::Key("ticker"), Seg::Key("last")];
const DERIBIT_PRICE: &[Seg] = &[Seg::Key("result"), Seg::Key("last_price")];
/// Bitfinex ticker array: [BID, BID_SIZE, ASK, ASK_SIZE, CHANGE, CHANGE_%, LAST, …].
const BITFINEX_PRICE: &[Seg] = &[Seg::Index(6)];

const fn get(url: &'static str, style: SymbolStyle, price_path: &'static [Seg]) -> MarketSpec {
    MarketSpec { url, style, suffix: "", price_path, post: None }
}

/// All catalog exchanges. Order is the health-strip display order: the 15
/// venues (order venues and price-data / paper venues), then the other
/// price-only exchanges.
/// Every URL and price path below was read from the live public API on
/// 22 Oct 2026, except CoinEx (its API answered 503 from the test network;
/// the v2 ticker path is from its documentation).
pub const CATALOG: &[ExchangeSpec] = &[
    ExchangeSpec {
        id: "binance",
        name: "Binance",
        spot: Some(get("https://api.binance.com/api/v3/ticker/price?symbol={sym}", SymbolStyle::Plain, BINANCE_PRICE)),
        futures: Some(get("https://fapi.binance.com/fapi/v1/ticker/price?symbol={sym}", SymbolStyle::Plain, BINANCE_PRICE)),
    },
    ExchangeSpec {
        id: "bybit",
        name: "Bybit",
        spot: Some(get("https://api.bybit.com/v5/market/tickers?category=spot&symbol={sym}", SymbolStyle::Plain, BYBIT_PRICE)),
        futures: Some(get("https://api.bybit.com/v5/market/tickers?category=linear&symbol={sym}", SymbolStyle::Plain, BYBIT_PRICE)),
    },
    ExchangeSpec {
        id: "okx",
        name: "OKX",
        spot: Some(get("https://www.okx.com/api/v5/market/ticker?instId={sym}", SymbolStyle::Dash, OKX_PRICE)),
        futures: Some(MarketSpec {
            url: "https://www.okx.com/api/v5/market/ticker?instId={sym}",
            style: SymbolStyle::Dash,
            suffix: "-SWAP",
            price_path: OKX_PRICE,
            post: None,
        }),
    },
    ExchangeSpec {
        id: "bitget",
        name: "Bitget",
        spot: Some(get("https://api.bitget.com/api/v2/spot/market/tickers?symbol={sym}", SymbolStyle::Plain, BITGET_PRICE)),
        futures: Some(get(
            "https://api.bitget.com/api/v2/mix/market/ticker?productType=USDT-FUTURES&symbol={sym}",
            SymbolStyle::Plain,
            BITGET_PRICE,
        )),
    },
    ExchangeSpec {
        id: "gate",
        name: "Gate",
        spot: Some(get("https://api.gateio.ws/api/v4/spot/tickers?currency_pair={sym}", SymbolStyle::Underscore, GATE_PRICE)),
        futures: Some(get("https://api.gateio.ws/api/v4/futures/usdt/tickers?contract={sym}", SymbolStyle::Underscore, GATE_PRICE)),
    },
    ExchangeSpec {
        id: "kucoinfutures",
        name: "KuCoin Futures",
        spot: Some(get("https://api.kucoin.com/api/v1/market/orderbook/level1?symbol={sym}", SymbolStyle::Dash, KUCOIN_PRICE)),
        futures: Some(get("https://api-futures.kucoin.com/api/v1/ticker?symbol={sym}", SymbolStyle::KucoinPerp, KUCOIN_PRICE)),
    },
    ExchangeSpec {
        id: "htx",
        name: "HTX",
        spot: Some(get("https://api.huobi.pro/market/detail/merged?symbol={sym}", SymbolStyle::PlainLower, HTX_PRICE)),
        futures: Some(get("https://api.hbdm.com/linear-swap-ex/market/detail/merged?contract_code={sym}", SymbolStyle::Dash, HTX_PRICE)),
    },
    ExchangeSpec {
        id: "bingx",
        name: "BingX",
        spot: Some(get("https://open-api.bingx.com/openApi/spot/v1/ticker/price?symbol={sym}", SymbolStyle::Dash, BINGX_SPOT_PRICE)),
        futures: Some(get("https://open-api.bingx.com/openApi/swap/v2/quote/price?symbol={sym}", SymbolStyle::Dash, BINGX_SWAP_PRICE)),
    },
    ExchangeSpec {
        id: "phemex",
        name: "Phemex",
        // Phemex spot tickers are scaled integers (`lastEp`); only the
        // USDT perpetual (real-valued `lastRp`) is read.
        spot: None,
        futures: Some(get("https://api.phemex.com/md/v3/ticker/24hr?symbol={sym}", SymbolStyle::Plain, PHEMEX_PRICE)),
    },
    ExchangeSpec {
        id: "coinex",
        name: "CoinEx",
        spot: Some(get("https://api.coinex.com/v2/spot/ticker?market={sym}", SymbolStyle::Plain, COINEX_PRICE)),
        futures: Some(get("https://api.coinex.com/v2/futures/ticker?market={sym}", SymbolStyle::Plain, COINEX_PRICE)),
    },
    ExchangeSpec {
        id: "woo",
        name: "WOO X",
        spot: Some(get("https://api.woox.io/v1/public/market_trades?limit=1&symbol={sym}", SymbolStyle::WooSpot, WOO_PRICE)),
        futures: Some(get("https://api.woox.io/v1/public/market_trades?limit=1&symbol={sym}", SymbolStyle::WooPerp, WOO_PRICE)),
    },
    ExchangeSpec {
        id: "hyperliquid",
        name: "Hyperliquid",
        // USDC-margined perpetuals; mid prices of every coin in one POST.
        spot: None,
        futures: Some(MarketSpec {
            url: "https://api.hyperliquid.xyz/info",
            style: SymbolStyle::Base,
            suffix: "",
            price_path: HYPERLIQUID_PRICE,
            post: Some(r#"{"type":"allMids"}"#),
        }),
    },
    ExchangeSpec {
        id: "krakenfutures",
        name: "Kraken Futures",
        // USD-margined (multi-collateral) perpetuals.
        spot: None,
        futures: Some(get("https://futures.kraken.com/derivatives/api/v3/tickers/{sym}", SymbolStyle::KrakenPerp, KRAKEN_PRICE)),
    },
    ExchangeSpec {
        id: "deribit",
        name: "Deribit",
        // USDC-margined linear perpetuals.
        spot: None,
        futures: Some(get("https://www.deribit.com/api/v2/public/ticker?instrument_name={sym}", SymbolStyle::DeribitUsdc, DERIBIT_PRICE)),
    },
    ExchangeSpec {
        id: "bitfinex",
        name: "Bitfinex",
        spot: Some(get("https://api-pub.bitfinex.com/v2/ticker/{sym}", SymbolStyle::BitfinexSpot, BITFINEX_PRICE)),
        futures: Some(get("https://api-pub.bitfinex.com/v2/ticker/{sym}", SymbolStyle::BitfinexPerp, BITFINEX_PRICE)),
    },
    // ---- price only: no order path ----
    // MEXC: its futures order API needs KYB approval (institutions only).
    ExchangeSpec {
        id: "mexc",
        name: "MEXC",
        spot: Some(get("https://api.mexc.com/api/v3/ticker/price?symbol={sym}", SymbolStyle::Plain, BINANCE_PRICE)),
        futures: Some(get("https://contract.mexc.com/api/v1/contract/ticker?symbol={sym}", SymbolStyle::Underscore, MEXC_FUT_PRICE)),
    },
    // ---- Tier 3 regional (spot-only, real USDT liquidity) ----
    ExchangeSpec {
        id: "coindcx",
        name: "CoinDCX",
        // `current_prices` is a flat map keyed by market name (e.g. "BTCUSDT").
        spot: Some(get("https://public.coindcx.com/market_data/current_prices", SymbolStyle::Plain, COINDCX_PRICE)),
        futures: None,
    },
    ExchangeSpec {
        id: "bitso",
        name: "Bitso",
        spot: Some(get("https://api.bitso.com/v3/ticker/?book={sym}", SymbolStyle::UnderscoreLower, BITSO_PRICE)),
        futures: None,
    },
];

/// Finds a catalog exchange by id.
pub fn find(id: &str) -> Option<&'static ExchangeSpec> {
    CATALOG.iter().find(|e| e.id == id)
}
