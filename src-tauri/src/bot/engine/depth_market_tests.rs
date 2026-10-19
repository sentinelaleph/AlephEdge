//! The depth precheck reads the bot's own market's book: spot for the Spot
//! signal bot, USDT-M futures for Futures and Pump (Aleph Edge 0.3.18). The
//! thresholds are the level's, unchanged; only the book differs.

use std::cell::RefCell;

use crate::bot::model::BotKind;
use crate::market::model::DepthMarket;
use crate::risk::model::RiskLevel;
use crate::signal::model::{OpenStatus, Signal};

use super::pipeline::{self, depth_market, EntryFeeds};
use super::test_fixtures::{cfg, signal};

/// A fake Sentinel: records each depth request and answers from `book`.
struct Feeds {
    asked: RefCell<Vec<(String, DepthMarket)>>,
    book: Book,
}

impl EntryFeeds for Feeds {
    fn is_invalidated(&self, _: &str) -> bool {
        false
    }
    fn is_regime_vetoed(&self, _: &str) -> bool {
        false
    }
    fn open_status(&self, _: &Signal, _: u64) -> OpenStatus {
        OpenStatus::Open
    }
    fn futures_supported(&self, _: &str) -> bool {
        true
    }
    fn membership_active(&self) -> bool {
        true
    }
    fn open_count_for(&self, _: BotKind) -> usize {
        0
    }
    async fn live_price(&self, _: &str, _: &str, _: BotKind) -> Option<f64> {
        Some(100.0)
    }
    async fn funding(&self, _: &str) -> Result<Option<f64>, ()> {
        Ok(Some(0.0))
    }
    async fn depth(&self, symbol: &str, market: DepthMarket) -> Result<(f64, f64), ()> {
        self.asked.borrow_mut().push((symbol.to_string(), market));
        (self.book)(market)
    }
}

/// Spot-only pair: a spot book, no perp (SCUSDT on 17 Oct).
fn spot_only(m: DepthMarket) -> Result<(f64, f64), ()> {
    match m {
        DepthMarket::Spot => Ok((3_000.0, 5_000.0)),
        DepthMarket::Futures => Err(()),
    }
}

type Book = fn(DepthMarket) -> Result<(f64, f64), ()>;
/// (precheck's `ld_at_open` or skip key, depth requests made)
type Outcome = (Result<Option<f64>, String>, Vec<(String, DepthMarket)>);

fn run(kind: BotKind, level: RiskLevel, notional: f64, book: Book) -> Outcome {
    let feeds = Feeds { asked: RefCell::new(Vec::new()), book };
    let mut c = cfg();
    c.kind = kind;
    let sig = signal("long", 100.0, 101.0, 98.0);
    let budget = pipeline::signal_bot_budget(0, 10_000.0, notional);
    let out = tauri::async_runtime::block_on(pipeline::precheck_with(&feeds, &level.limits(), &c, &sig, &budget, 0));
    (out.map(|ctx| ctx.ld_at_open).map_err(|s| s.key.to_string()), feeds.asked.into_inner())
}

#[test]
fn each_kind_reads_its_own_book() {
    assert_eq!(depth_market(BotKind::Spot), DepthMarket::Spot);
    assert_eq!(depth_market(BotKind::Futures), DepthMarket::Futures);
    assert_eq!(depth_market(BotKind::Pump), DepthMarket::Futures);
}

#[test]
fn the_spot_bot_asks_the_spot_book_and_trades_a_spot_only_pair_on_it() {
    // Cautious = strict data: before 0.3.18 this pair was ldUnavailable.
    let (out, asked) = run(BotKind::Spot, RiskLevel::Cautious, 100.0, spot_only);
    assert_eq!(asked, vec![("SOLUSDT".to_string(), DepthMarket::Spot)]);
    assert_eq!(out, Ok(Some(8_000.0)));
}

#[test]
fn futures_and_pump_ask_the_futures_book_and_stay_strict_without_one() {
    for kind in [BotKind::Futures, BotKind::Pump] {
        let (out, asked) = run(kind, RiskLevel::Cautious, 100.0, spot_only);
        assert_eq!(asked, vec![("SOLUSDT".to_string(), DepthMarket::Futures)], "{kind:?}");
        assert_eq!(out, Err("ldUnavailable".to_string()), "{kind:?}");
        // A loose level still trades without depth, as before.
        let (out, _) = run(kind, RiskLevel::Greedy, 100.0, spot_only);
        assert_eq!(out, Ok(None), "{kind:?}");
    }
}

#[test]
fn the_depth_share_threshold_is_the_level_s_on_either_book() {
    // Cautious allows 5% of the 8,000 USD book = 400 USD.
    let (out, _) = run(BotKind::Spot, RiskLevel::Cautious, 400.0, spot_only);
    assert_eq!(out, Ok(Some(8_000.0)));
    let (out, _) = run(BotKind::Spot, RiskLevel::Cautious, 401.0, spot_only);
    assert_eq!(out, Err("ldTooThin".to_string()));
    let (out, _) = run(BotKind::Futures, RiskLevel::Cautious, 401.0, |_| Ok((3_000.0, 5_000.0)));
    assert_eq!(out, Err("ldTooThin".to_string()));
}
