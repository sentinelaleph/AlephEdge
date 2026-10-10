//! Exchange adapters. F4 covers the Tier-1 USDT-quoted set (Binance, OKX,
//! Bybit, MEXC) — spot + USDT-M futures public market data — via the
//! data-driven `providers` catalog. Each exchange's status is cached briefly
//! so the health poll and bot precheck never hammer the public endpoints.

mod close;
mod commands;
mod live_orders;
mod orders;
#[cfg(test)]
mod testnet_tests;
pub mod model;
pub(crate) mod providers;
pub(crate) mod venue;

pub use orders::{
    breakeven_move, has_order_path, BreakevenMove, live_venue_allowed, live_venue_allowed_in, live_venue_check_in, live_venues,
    venue_order_client_id, DRY_RUN_PASSED, VENUE_NOT_DRY_RUN,
};

pub use commands::{
    exchange_account, exchange_close_all, exchange_close_position, exchange_list, exchange_status,
    exchange_symbols,
};

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use futures_util::future::join_all;
use reqwest::Client;

use model::{
    BinanceKeyCheckError, BinancePermissions, ExchangeInfo, ExchangeStatus, FuturesAccount,
};

const CACHE_TTL: Duration = Duration::from_secs(6);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

/// Tauri-managed exchange state: one shared client + a per-(exchange, symbol)
/// status cache.
pub struct ExchangeManager {
    client: Client,
    cache: Mutex<HashMap<String, (ExchangeStatus, Instant)>>,
    /// Tradable symbol lists by "exchange:market", refreshed hourly.
    symbols: Mutex<HashMap<String, (Vec<String>, Instant)>>,
    /// Bybit / OKX order clients (venue/ccxt.rs).
    venues: venue::ccxt::CcxtVenues,
}

const SYMBOLS_TTL: Duration = Duration::from_secs(3600);

impl ExchangeManager {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .unwrap_or_default(),
            cache: Mutex::new(HashMap::new()),
            symbols: Mutex::new(HashMap::new()),
            venues: venue::ccxt::CcxtVenues::default(),
        }
    }

    /// The exchange's tradable USDT symbols for one market, for the per-coin
    /// pickers. Binance only today; any other exchange, or a failed fetch,
    /// gives an empty list and the UI falls back to recent-signal symbols.
    pub async fn symbols(&self, exchange_id: &str, futures: bool) -> Vec<String> {
        if exchange_id != "binance" {
            return Vec::new();
        }
        let key = format!("{exchange_id}:{}", if futures { "futures" } else { "spot" });
        if let Some((list, at)) = self.symbols.lock().expect("symbols mutex").get(&key) {
            if at.elapsed() < SYMBOLS_TTL {
                return list.clone();
            }
        }
        match providers::binance_symbols::fetch_binance_usdt_symbols(&self.client, futures).await {
            Some(list) if !list.is_empty() => {
                self.symbols
                    .lock()
                    .expect("symbols mutex")
                    .insert(key, (list.clone(), Instant::now()));
                list
            }
            _ => Vec::new(),
        }
    }

    /// The catalog exchanges, for the UI picker and health strip.
    pub fn list(&self) -> Vec<ExchangeInfo> {
        providers::CATALOG.iter().map(|e| e.info()).collect()
    }

    pub fn is_known(&self, exchange_id: &str) -> bool {
        providers::find(exchange_id).is_some()
    }

    pub fn supports_futures(&self, exchange_id: &str) -> bool {
        providers::find(exchange_id).is_some_and(|e| e.supports_futures())
    }

    /// Status for one exchange + symbol, served from cache when fresh. Unknown
    /// exchange id ⇒ an unhealthy status (never panics).
    pub async fn status(&self, exchange_id: &str, symbol: &str) -> ExchangeStatus {
        let key = format!("{exchange_id}:{symbol}");
        if let Some(fresh) = self.cached(&key) {
            return fresh;
        }
        let status = match providers::find(exchange_id) {
            Some(spec) => providers::fetch_status(&self.client, spec, symbol).await,
            None => unhealthy(),
        };
        self.cache
            .lock()
            .expect("exchange mutex")
            .insert(key, (status.clone(), Instant::now()));
        status
    }

    /// Every catalog exchange's status for `symbol`, fetched concurrently.
    pub async fn all_status(&self, symbol: &str) -> Vec<(ExchangeInfo, ExchangeStatus)> {
        let futures = providers::CATALOG
            .iter()
            .map(|spec| async move { (spec.info(), self.status(spec.id, symbol).await) });
        join_all(futures).await
    }

    /// Verifies a Binance key's real withdraw permission (see providers).
    pub async fn check_binance_permissions(
        &self,
        api_key: &str,
        api_secret: &str,
    ) -> Result<BinancePermissions, BinanceKeyCheckError> {
        providers::check_binance_permissions(&self.client, api_key, api_secret).await
    }

    /// The connected Binance account's USDT-M futures snapshot (balance + open
    /// positions), fetched read-only with the vaulted key.
    pub(crate) async fn bn_futures_account(
        &self,
        api_key: &str,
        api_secret: &str,
    ) -> Result<FuturesAccount, BinanceKeyCheckError> {
        providers::fetch_binance_futures_account(&self.client, api_key, api_secret).await
    }

    fn cached(&self, key: &str) -> Option<ExchangeStatus> {
        let guard = self.cache.lock().expect("exchange mutex");
        let (status, at) = guard.get(key)?;
        (at.elapsed() < CACHE_TTL).then(|| status.clone())
    }
}

impl Default for ExchangeManager {
    fn default() -> Self {
        Self::new()
    }
}

fn unhealthy() -> ExchangeStatus {
    ExchangeStatus {
        healthy: false,
        latency_ms: None,
        spot_price: None,
        futures_price: None,
    }
}
