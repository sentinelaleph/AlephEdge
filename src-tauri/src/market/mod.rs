//! Sentinel market data (funding, liquidity depth, BTC macro) with short TTL
//! caches so the bot's precheck pipeline and the health poll never hammer the
//! services. Stale-on-error is deliberately NOT done for precheck data — a
//! trade decision must use fresh numbers or honestly report "no data".

mod client;
pub mod history;
pub mod model;

use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::Client;

use model::{BtcMacro, DepthAnalysis, DepthMarket, FundingRate};

const FUNDING_TTL: Duration = Duration::from_secs(30);
const DEPTH_TTL: Duration = Duration::from_secs(10);
const BTC_TTL: Duration = Duration::from_secs(20);
/// After an older server answered a `market=` depth request as if it had no
/// market (see `depth_for_market`), ask the old way for this long, then try
/// `market=` again (the server may have been upgraded meanwhile).
const LEGACY_DEPTH_RECHECK: Duration = Duration::from_secs(30 * 60);

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
    depth: Mutex<HashMap<(DepthMarket, String), Cached<DepthAnalysis>>>,
    /// When the server last turned out not to know `market=` on depth.
    legacy_depth_at: Mutex<Option<Instant>>,
    btc: Mutex<Option<Cached<BtcMacro>>>,
}

impl MarketManager {
    pub fn new() -> Self {
        Self {
            client: client::build_client(),
            funding: Mutex::new(None),
            depth: Mutex::new(HashMap::new()),
            legacy_depth_at: Mutex::new(None),
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

    /// ±1% order-book depth analysis for a symbol on `market`'s book (auth
    /// required upstream): spot for the Spot signal bot, futures for Futures
    /// and Pump (`pipeline::depth_market`). An older server that does not
    /// know `market=` is detected and asked the old way (its futures book,
    /// the pre-0.3.18 behaviour), see `depth_for_market`.
    pub async fn depth(
        &self,
        base: &str,
        token: &str,
        symbol: &str,
        market: DepthMarket,
    ) -> Result<DepthAnalysis, String> {
        let key = (market, symbol.to_string());
        if let Some(hit) = self
            .depth
            .lock()
            .expect("market mutex")
            .get(&key)
            .and_then(|c| c.fresh(DEPTH_TTL))
        {
            return Ok(hit);
        }
        let legacy = self.legacy_depth();
        let (fetched, server_legacy) = depth_for_market(market, legacy, |m| {
            client::liquidity_depth(&self.client, base, token, symbol, m)
        })
        .await;
        if server_legacy && !legacy {
            // Recorded once per recheck window: this server reads the futures
            // book for every bot (as Aleph Edge 0.3.17 did).
            eprintln!(
                "depth: server does not support market= (asked {} for {symbol}); using its futures default for {} min",
                market.as_str(),
                LEGACY_DEPTH_RECHECK.as_secs() / 60
            );
            *self.legacy_depth_at.lock().expect("market mutex") = Some(Instant::now());
        }
        let fetched = fetched?;
        self.depth.lock().expect("market mutex").insert(
            key,
            Cached {
                value: fetched.clone(),
                at: Instant::now(),
            },
        );
        Ok(fetched)
    }

    /// The server was found not to know `market=` within the recheck window.
    pub fn legacy_depth(&self) -> bool {
        self.legacy_depth_at
            .lock()
            .expect("market mutex")
            .is_some_and(|at| at.elapsed() < LEGACY_DEPTH_RECHECK)
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

/// A server older than Aleph Edge 0.3.18 answers `market=` in one of two
/// ways: it ignores the unknown query (200 with its futures book, which the
/// body's `source` names) or, behind a stricter front, refuses it (400).
/// Either way the answer is that server's no-market request, the futures
/// book, exactly what 0.3.17 read for every bot; a 400 is therefore asked
/// again without `market=`. Any other error (404 = no such book on this
/// market, 503, unreachable) is the precheck's "depth unavailable", with no
/// cross-market retry. `legacy` = already known: ask the old way at once.
/// Returns the depth and whether the server turned out to be the older one.
async fn depth_for_market<F, Fut>(
    market: DepthMarket,
    legacy: bool,
    fetch: F,
) -> (Result<DepthAnalysis, String>, bool)
where
    F: Fn(Option<DepthMarket>) -> Fut,
    Fut: Future<Output = Result<DepthAnalysis, String>>,
{
    if legacy {
        return (fetch(None).await, true);
    }
    match fetch(Some(market)).await {
        Err(e) if e == "liquidityRefused|400" => (fetch(None).await, true),
        Ok(d) if !market.matches_source(&d.source) => (Ok(d), true),
        other => (other, false),
    }
}

impl Default for MarketManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod depth_market_tests {
    use super::*;
    use std::cell::RefCell;

    fn body(source: &str) -> DepthAnalysis {
        DepthAnalysis {
            symbol: "SCUSDT".into(),
            bid_depth_usd: 3_000.0,
            ask_depth_usd: 5_000.0,
            imbalance: 0.0,
            spread_bps: 10.0,
            truncated: false,
            source: source.into(),
        }
    }

    /// Runs `depth_for_market` against a fake server: `answer` maps the
    /// request's market to its reply; returns (result, legacy, requests).
    fn run(
        market: DepthMarket,
        legacy: bool,
        answer: impl Fn(Option<DepthMarket>) -> Result<DepthAnalysis, String>,
    ) -> (Result<DepthAnalysis, String>, bool, Vec<Option<DepthMarket>>) {
        let asked = RefCell::new(Vec::new());
        let (res, was_legacy) = tauri::async_runtime::block_on(depth_for_market(market, legacy, |m| {
            asked.borrow_mut().push(m);
            let out = answer(m);
            async move { out }
        }));
        (res, was_legacy, asked.into_inner())
    }

    #[test]
    fn each_market_is_asked_for_its_own_book() {
        let (res, legacy, asked) = run(DepthMarket::Spot, false, |m| match m {
            Some(DepthMarket::Spot) => Ok(body("datahub.binance.spot.depth")),
            _ => Err("liquidityRefused|404".into()),
        });
        assert_eq!(asked, vec![Some(DepthMarket::Spot)]);
        assert!(!legacy);
        assert!((res.unwrap().bid_depth_usd - 3_000.0).abs() < 1e-9);

        let (res, legacy, asked) = run(DepthMarket::Futures, false, |_| Ok(body("datahub.binance.futures.depth")));
        assert_eq!(asked, vec![Some(DepthMarket::Futures)]);
        assert!(!legacy);
        assert!(res.is_ok());
    }

    #[test]
    fn a_spot_pair_without_a_spot_book_stays_unavailable_without_a_futures_retry() {
        let (res, legacy, asked) = run(DepthMarket::Spot, false, |_| Err("liquidityRefused|404".into()));
        assert_eq!(asked, vec![Some(DepthMarket::Spot)], "no cross-market fallback");
        assert!(!legacy);
        assert_eq!(res.err().as_deref(), Some("liquidityRefused|404"));
    }

    #[test]
    fn an_older_server_that_refuses_market_is_asked_the_old_way() {
        let (res, legacy, asked) = run(DepthMarket::Spot, false, |m| match m {
            Some(_) => Err("liquidityRefused|400".into()),
            None => Ok(body("datahub.binance.futures.depth")),
        });
        assert_eq!(asked, vec![Some(DepthMarket::Spot), None]);
        assert!(legacy);
        assert_eq!(res.unwrap().source, "datahub.binance.futures.depth");
    }

    #[test]
    fn an_older_server_that_ignores_market_is_recognised_by_the_source() {
        // The live sentinel-api before the proxy change: market= is ignored
        // and the futures book comes back; that IS the old call's answer.
        let (res, legacy, asked) = run(DepthMarket::Spot, false, |_| Ok(body("datahub.binance.futures.depth")));
        assert_eq!(asked, vec![Some(DepthMarket::Spot)]);
        assert!(legacy);
        assert!(res.is_ok());
    }

    #[test]
    fn once_known_legacy_the_old_request_is_sent_directly() {
        let (res, legacy, asked) = run(DepthMarket::Spot, true, |_| Ok(body("datahub.binance.futures.depth")));
        assert_eq!(asked, vec![None]);
        assert!(legacy);
        assert!(res.is_ok());
    }

    #[test]
    fn other_errors_are_unavailable_and_not_a_legacy_server() {
        for code in ["liquidityRefused|503", "liquidityRefused|401", "liquidityUnreachable", "liquidityMalformed"] {
            let (res, legacy, asked) = run(DepthMarket::Futures, false, |_| Err(code.into()));
            assert_eq!(asked, vec![Some(DepthMarket::Futures)], "{code}");
            assert!(!legacy, "{code}");
            assert_eq!(res.err().as_deref(), Some(code));
        }
    }

    #[test]
    fn a_fresh_manager_asks_with_market() {
        assert!(!MarketManager::new().legacy_depth());
    }
}
