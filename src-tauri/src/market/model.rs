//! Sentinel market-data wire mirrors (snake_case on purpose — these match the
//! real JSON the Sentinel services emit, same convention as `signal::model`):
//! the bodies of `/api/v1/market/funding-rates`, `/api/v1/market/depth/{symbol}`
//! and `/api/v1/market/btc-macro` (routes in `market::client`).

use serde::{Deserialize, Serialize};

/// One perpetual's funding, from DataHub's all-perp funding snapshot
/// (`/api/v1/market/funding-rates`, Binance USDT-M premiumIndex).
/// `rate` is in PERCENT per funding interval (0.01 = 0.01%).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingRate {
    pub symbol: String,
    pub rate: f64,
    #[serde(default)]
    pub next_funding: String,
    #[serde(default)]
    pub annualized: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FundingRatesResponse {
    #[serde(default)]
    pub rates: Vec<FundingRate>,
    #[serde(default)]
    pub available: Option<bool>,
    /// DataHub served its last good snapshot (at most 10 minutes old)
    /// because Binance failed.
    #[serde(default)]
    pub stale: bool,
}

/// Which Binance book a depth check reads: the bot's own market (spot for the
/// Spot signal bot, USDT-M futures for Futures and Pump; see
/// `bot::engine::pipeline::depth_market`). DataHub's `market=` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DepthMarket {
    Spot,
    Futures,
}

impl DepthMarket {
    pub fn as_str(self) -> &'static str {
        match self {
            DepthMarket::Spot => "spot",
            DepthMarket::Futures => "futures",
        }
    }

    /// Whether a depth body is from this market's book. DataHub names it in
    /// `source` (`datahub.binance.spot.depth` / `datahub.binance.futures.depth`);
    /// a body without a source (older payloads) is taken as it is.
    pub fn matches_source(self, source: &str) -> bool {
        source.is_empty() || source.contains(&format!(".{}.", self.as_str()))
    }
}

/// USD resting within ±1% of mid on each side of a Binance book (USDT-M
/// futures by default, spot with `market=spot`), from
/// `/api/v1/market/depth/{symbol}` (DataHub).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepthAnalysis {
    pub symbol: String,
    pub bid_depth_usd: f64,
    pub ask_depth_usd: f64,
    #[serde(default)]
    pub imbalance: f64,
    #[serde(default)]
    pub spread_bps: f64,
    /// The ±1% band reached past the 500 levels DataHub reads, so the sums
    /// are a lower bound.
    #[serde(default)]
    pub truncated: bool,
    /// The book DataHub read, e.g. `datahub.binance.spot.depth`.
    #[serde(default)]
    pub source: String,
}

/// BTC macro pulse from `/api/v1/market/btc-macro`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BtcMacro {
    pub price: f64,
    #[serde(default)]
    pub change_pct: f64,
    /// "bullish" | "bearish" | "neutral".
    pub trend: String,
    #[serde(default)]
    pub trend_strength: f64,
    #[serde(default)]
    pub market_phase: String,
    /// Sentinel's own "BTC is breaking down" declaration — the exact flag its
    /// pipeline uses to veto new alt longs. This is a statement, not an
    /// inference: when true, Sentinel has stopped publishing longs itself.
    #[serde(default)]
    pub btcd_veto_long: bool,
    /// "pumping" | "dumping" | "neutral" — the impulse detector.
    #[serde(default)]
    pub pulse_alert: String,
}
