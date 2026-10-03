//! Sentinel market data (funding, liquidity depth, BTC macro) with short TTL
//! caches so the bot's precheck pipeline and the health poll never hammer the
//! services. Stale-on-error is deliberately NOT done for precheck data — a
//! trade decision must use fresh numbers or honestly report "no data".

mod client;
pub mod history;
pub mod model;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::Client;

use model::{BtcMacro, DepthAnalysis, FundingRate};

const FUNDING_TTL: Duration = Duration::from_secs(30);
const DEPTH_TTL: Duration = Duration::from_secs(10);
const BTC_TTL: Duration = Duration::from_secs(20);

struct Cached<T> {
    value: T,
    at: Instant,
}

impl<T: Clone> Cached<T> {
    fn fresh(&self, ttl: Duration) -> Option<T> {
        (self.at.elapsed() < ttl).then(|| self.value.clone())
    }
}

pub struct MarketManager {
    client: Client,
    funding: Mutex<Option<Cached<Vec<FundingRate>>>>,
    depth: Mutex<HashMap<String, Cached<DepthAnalysis>>>,
    btc: Mutex<Option<Cached<BtcMacro>>>,
}

impl MarketManager {
    pub fn new() -> Self {
        Self {
            client: client::build_client(),
            funding: Mutex::new(None),
            depth: Mutex::new(HashMap::new()),
            btc: Mutex::new(None),
        }
    }

    /// Funding rate for one symbol, %/interval as Sentinel reports it.
    /// `Ok(None)` = feed healthy but symbol not covered. Auth required
    /// upstream since the feed moved behind the API's session check.
    pub async fn funding_for(
        &self,
        base: &str,
        token: &str,
        symbol: &str,
    ) -> Result<Option<FundingRate>, String> {
        let cached = self
            .funding
            .lock()
            .expect("market mutex")
            .as_ref()
            .and_then(|c| c.fresh(FUNDING_TTL));
        let rates = match cached {
            Some(r) => r,
            None => {
                let fetched = client::funding_rates(&self.client, base, token).await?;
                // A stale snapshot (DataHub's <=10 min fallback while Binance
                // fails) is used for this check but not cached, so the next
                // check asks again instead of holding it for FUNDING_TTL.
                if !fetched.stale {
                    *self.funding.lock().expect("market mutex") = Some(Cached {
                        value: fetched.rates.clone(),
                        at: Instant::now(),
                    });
                }
                fetched.rates
            }
        };
        Ok(rates.into_iter().find(|r| r.symbol == symbol))
    }

    /// ±1% order-book depth analysis for a symbol (auth required upstream).
    pub async fn depth(
        &self,
        base: &str,
        token: &str,
        symbol: &str,
    ) -> Result<DepthAnalysis, String> {
        if let Some(hit) = self
            .depth
            .lock()
            .expect("market mutex")
            .get(symbol)
            .and_then(|c| c.fresh(DEPTH_TTL))
        {
            return Ok(hit);
        }
        let fetched = client::liquidity_depth(&self.client, base, token, symbol).await?;
        self.depth.lock().expect("market mutex").insert(
            symbol.to_string(),
            Cached {
                value: fetched.clone(),
                at: Instant::now(),
            },
        );
        Ok(fetched)
    }

    pub async fn btc(&self, base: &str) -> Result<BtcMacro, String> {
        if let Some(hit) = self
            .btc
            .lock()
            .expect("market mutex")
            .as_ref()
            .and_then(|c| c.fresh(BTC_TTL))
        {
            return Ok(hit);
        }
        let fetched = client::btc_macro(&self.client, base).await?;
        *self.btc.lock().expect("market mutex") = Some(Cached {
            value: fetched.clone(),
            at: Instant::now(),
        });
        Ok(fetched)
    }
}

impl Default for MarketManager {
    fn default() -> Self {
        Self::new()
    }
}
