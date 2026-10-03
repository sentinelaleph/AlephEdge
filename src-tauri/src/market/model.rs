//! Sentinel market-data wire mirrors (snake_case on purpose — these match the
//! real JSON the Sentinel services emit, same convention as `signal::model`).
//! Sources: `sentinel-alephv2/frontend/src/services/api/{arbitrage,liquidity}.ts`
//! and `app/dashboard/components/BTCPulse.tsx`.

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

/// USD resting within ±1% of mid on each side of the Binance USDT-M futures
/// book, from `/api/v1/market/depth/{symbol}` (DataHub).
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
