//! Paper / Live per bot (0.3.19, one build): the mode is stored and never
//! flips silently, Live needs the typed arm every session, paper and real
//! money never share a bot, and the restart and migration rules fail safe.

use super::commands::{exchange_change_refusal, live_enable_refusal, off_mode, paper_book_refusal, saved_mode, KeepLive};
use super::engine::test_fixtures::{position, signal};
use super::engine::user_trade::{mode_refusal, TradeRequest, BOT_IS_PAPER, LIVE_FUTURES_ONLY};
use super::model::{BotConfig, BotKind, OpenPosition, TradeMode};
use super::strategy_live::{restored_blind_since, start_refusal_of, LiveState};
use super::{mapped_mode, start_mode_refusal, BotManager, LIVE_CONFIRMATION};
use crate::signal::model::Direction;

fn cfg(mode: TradeMode, armed: bool) -> BotConfig {
    let mut c = BotConfig::new_default(BotKind::Futures, "binance");
    (c.mode, c.live) = (mode, armed);
    c
}

fn pos(kind: BotKind, live: bool) -> OpenPosition {
    let mut p = position(&signal("long", 100.0, 106.0, 93.0), None);
    (p.bot_kind, p.live) = (kind, live);
    p
}

#[test]
fn a_new_bot_is_paper_and_a_paper_config_is_written_as_before() {
    let c = BotConfig::new_default(BotKind::Futures, "binance");
    assert_eq!(c.mode, TradeMode::Paper);
    assert!(!c.live);
    let v = serde_json::to_value(&c).unwrap();
    assert!(v.get("mode").is_none(), "a paper config carries no new field: {v}");
    // A config saved before modes existed reads as Paper.
    let old: BotConfig = serde_json::from_str(include_str!("../store/fixtures/v0_2_0/bot_config_futures.json")).unwrap();
    assert_eq!(old.mode, TradeMode::Paper);
    // A Live bot keeps its mode on disk; the arm is a separate flag.
    let live = serde_json::to_value(cfg(TradeMode::Live, false)).unwrap();
    assert_eq!(live["mode"], "live");
    assert_eq!(live["live"], false);
}

#[test]
fn a_live_bot_starts_only_when_armed_this_session() {
    assert_eq!(start_mode_refusal(&cfg(TradeMode::Paper, false)), None);
    assert_eq!(start_mode_refusal(&cfg(TradeMode::Live, false)), Some("liveArmRequired"));
    assert_eq!(start_mode_refusal(&cfg(TradeMode::Live, true)), None);
}

#[test]
fn a_settings_save_never_changes_the_mode_and_new_terms_disarm() {
    let prev = cfg(TradeMode::Live, true);
    let same = prev.clone();
    assert_eq!(saved_mode(Some(&prev), &same, KeepLive::Yes, false), (TradeMode::Live, true));
    let mut filter_only = prev.clone();
    filter_only.min_confidence = Some(0.8);
    assert_eq!(saved_mode(Some(&prev), &filter_only, KeepLive::Yes, false), (TradeMode::Live, true), "filters are not sizing");
    let mut bigger = prev.clone();
    bigger.capital *= 2.0;
    assert_eq!(saved_mode(Some(&prev), &bigger, KeepLive::Yes, false), (TradeMode::Live, false), "new sizing: still Live, disarmed");
    // A form that sends `live`/`mode` cannot switch real money on.
    let paper = cfg(TradeMode::Paper, false);
    let forged = cfg(TradeMode::Live, true);
    assert_eq!(saved_mode(Some(&paper), &forged, KeepLive::Yes, false), (TradeMode::Paper, false));
    assert_eq!(saved_mode(None, &forged, KeepLive::Yes, false), (TradeMode::Paper, false), "a first save is Paper");
    // A settings import: Paper, unless real positions are still open.
    assert_eq!(saved_mode(Some(&prev), &same, KeepLive::No, false), (TradeMode::Paper, false));
    assert_eq!(saved_mode(Some(&prev), &same, KeepLive::No, true), (TradeMode::Live, false));
}

#[test]
fn switching_off_goes_back_to_paper_only_once_flat() {
    assert_eq!(off_mode(false), TradeMode::Paper);
    assert_eq!(off_mode(true), TradeMode::Live, "real positions keep it a Live bot until they close");
}

#[test]
fn a_bot_holding_paper_positions_cannot_take_real_money() {
    let book = [pos(BotKind::Futures, false), pos(BotKind::Spot, true)];
    assert_eq!(paper_book_refusal(BotKind::Futures, &book), Some("liveHasPaperPositions"));
    assert_eq!(paper_book_refusal(BotKind::Spot, &book), None, "its own real rows are fine");
    assert_eq!(paper_book_refusal(BotKind::Pump, &book), None);
}

#[test]
fn going_live_needs_futures_a_dry_run_venue_and_the_typed_word() {
    let on = |kind, venue: &str, word: &str| live_enable_refusal(true, kind, venue, word, false);
    assert_eq!(on(BotKind::Futures, "binance", LIVE_CONFIRMATION), None);
    assert_eq!(on(BotKind::Futures, "binance", " LIVE "), None, "trimmed");
    assert!(on(BotKind::Futures, "binance", "live").is_some_and(|e| e.starts_with("liveConfirmRequired|")));
    assert!(on(BotKind::Futures, "binance", "").is_some());
    assert_eq!(on(BotKind::Spot, "binance", LIVE_CONFIRMATION).as_deref(), Some("liveFuturesBinanceOnly"));
    assert_eq!(on(BotKind::Pump, "binance", LIVE_CONFIRMATION).as_deref(), Some("liveFuturesBinanceOnly"));
    assert!(on(BotKind::Futures, "bybit", LIVE_CONFIRMATION).is_some(), "no dry run passed");
    assert!(on(BotKind::Futures, "bitget", LIVE_CONFIRMATION).is_some(), "no order path");
    // A binary without the order path (the bot farm's) never goes live.
    assert_eq!(live_enable_refusal(false, BotKind::Futures, "binance", LIVE_CONFIRMATION, false).as_deref(), Some("liveBuildDisabled"));
}

#[test]
fn a_live_bot_keeps_its_exchange_even_disarmed() {
    let live = cfg(TradeMode::Live, false);
    let mut to_okx = live.clone();
    to_okx.exchange_id = "okx".into();
    assert_eq!(exchange_change_refusal(Some(&live), &to_okx, &[]).as_deref(), Some("liveExchangeLocked|binance"));
    let paper = cfg(TradeMode::Paper, false);
    assert_eq!(exchange_change_refusal(Some(&paper), &to_okx, &[]), None);
}

#[test]
fn data_from_a_live_build_maps_to_live_bots_once_and_nothing_else_does() {
    assert_eq!(mapped_mode(TradeMode::Paper, false, false), TradeMode::Paper, "a paper user stays paper");
    assert_eq!(mapped_mode(TradeMode::Paper, true, false), TradeMode::Live, "saved while live");
    assert_eq!(mapped_mode(TradeMode::Paper, false, true), TradeMode::Live, "holds a real row");
    assert_eq!(mapped_mode(TradeMode::Live, false, false), TradeMode::Live);
}

#[test]
fn unreadable_real_rows_count_as_money_in_play() {
    let bots = BotManager::new();
    assert!(!bots.has_live_exposure());
    bots.set_live_unreadable(vec!["futures sig-9".into()]);
    assert!(bots.has_live_exposure());
    assert_eq!(bots.status().live_unreadable, vec!["futures sig-9".to_string()]);
}

#[test]
fn an_armed_bot_or_a_real_position_is_money_in_play() {
    let bots = BotManager::new();
    bots.configure(cfg(TradeMode::Live, false));
    assert!(!bots.has_live_exposure(), "a disarmed, flat Live bot");
    bots.configure(cfg(TradeMode::Live, true));
    assert!(bots.has_live_exposure());
    let bots = BotManager::new();
    bots.add_position(pos(BotKind::Futures, true));
    assert!(bots.has_live_exposure());
}

fn trade(mode: TradeMode, kind: BotKind, word: Option<&str>) -> TradeRequest {
    TradeRequest {
        kind,
        symbol: "SOLUSDT".into(),
        direction: Direction::Long,
        notional_usdt: None,
        take_profit: 110.0,
        stop_loss: 95.0,
        mode,
        confirmation: word.map(str::to_string),
        confirmed_margin_usdt: None,
    }
}

#[test]
fn a_new_trade_matches_its_bot_paper_to_paper_and_live_to_an_armed_live_bot() {
    use super::engine::manual::PAPER_ONLY;
    let paper_bot = cfg(TradeMode::Paper, false);
    let live_bot = cfg(TradeMode::Live, true);
    let disarmed = cfg(TradeMode::Live, false);
    let p = trade(TradeMode::Paper, BotKind::Futures, None);
    let l = trade(TradeMode::Live, BotKind::Futures, Some(LIVE_CONFIRMATION));
    assert_eq!(mode_refusal(&paper_bot, &p), Ok(()), "Paper is the default");
    assert_eq!(mode_refusal(&live_bot, &p), Err(PAPER_ONLY.into()), "no paper trade on a Live bot");
    assert_eq!(mode_refusal(&disarmed, &p), Err(PAPER_ONLY.into()), "armed or not");
    assert_eq!(mode_refusal(&live_bot, &l), Ok(()));
    assert_eq!(mode_refusal(&paper_bot, &l), Err(BOT_IS_PAPER.into()));
    assert_eq!(mode_refusal(&disarmed, &l), Err("liveArmRequired".into()));
    let no_word = trade(TradeMode::Live, BotKind::Futures, None);
    assert!(mode_refusal(&live_bot, &no_word).is_err_and(|e| e.starts_with("liveConfirmRequired|")));
    let spot = trade(TradeMode::Live, BotKind::Spot, Some(LIVE_CONFIRMATION));
    assert_eq!(mode_refusal(&live_bot, &spot), Err(LIVE_FUTURES_ONLY.into()));
    // A request without the field (TradingView, an older UI) is Paper.
    let r: TradeRequest = serde_json::from_str(
        r#"{"kind":"futures","symbol":"SOLUSDT","direction":"long","takeProfit":110,"stopLoss":95}"#,
    )
    .unwrap();
    assert_eq!(r.mode, TradeMode::Paper);
}

#[test]
fn a_live_dca_or_grid_bot_starts_only_armed_unhalted_unlocked_on_a_dry_run_venue() {
    let paper = LiveState::default();
    assert_eq!(start_refusal_of(&paper, false, false, false), None, "a Paper bot starts on paper");
    let live = LiveState { enabled: true, ..LiveState::default() };
    assert_eq!(start_refusal_of(&live, true, true, true), None);
    assert_eq!(start_refusal_of(&live, false, true, true), Some("liveArmRequired"));
    assert_eq!(start_refusal_of(&live, true, true, false), Some("liveVaultLocked"));
    assert_eq!(start_refusal_of(&live, true, false, true), Some(crate::exchange::VENUE_NOT_DRY_RUN));
    let halted = LiveState { halted: Some("liveMismatch".into()), ..live.clone() };
    assert_eq!(start_refusal_of(&halted, true, true, true), Some("liveHaltedStart"));
}

#[test]
fn a_restart_counts_the_downtime_as_blind_so_missed_orders_are_never_sent() {
    let now = 10_000_000;
    // Synced a minute before the restart: blind from before the grace.
    let since = restored_blind_since(now - 60_000, now);
    assert!(now - since > 120_000, "past the grace on the very first pass");
    // Synced long ago: blind from then.
    assert_eq!(restored_blind_since(1_000, now), 1_000);
}

#[test]
fn a_spot_tradingview_close_never_reaches_a_real_position() {
    let mut real = pos(BotKind::Futures, true);
    real.signal_id = "manual-1-0000000a".into();
    real.symbol = "BTCUSDT".into();
    let mut paper = real.clone();
    (paper.signal_id, paper.live) = ("manual-2-0000000b".into(), false);
    let rows = vec![real.clone()];
    assert_eq!(crate::tvlink::tasks::paper_close_target(&rows, BotKind::Futures, "BTCUSDT"), None);
    let rows = vec![real, paper];
    assert_eq!(
        crate::tvlink::tasks::paper_close_target(&rows, BotKind::Futures, "BTCUSDT").as_deref(),
        Some("manual-2-0000000b")
    );
}

#[test]
fn a_live_tradingview_bot_comes_back_from_a_restart_live_but_unarmed() {
    let dir = std::env::temp_dir().join(format!("aleph-tvbot-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = crate::store::StoreManager::new();
    let mut tv = BotConfig::new_default(BotKind::Tradingview, "binance");
    (tv.mode, tv.live) = (TradeMode::Live, true);
    store.set_meta(&dir, &super::config_key(BotKind::Tradingview), &serde_json::to_string(&tv).unwrap()).unwrap();
    let plan = super::restore::plan(&store, &dir);
    let back = plan.configs.iter().find(|c| c.kind == BotKind::Tradingview).expect("restored");
    assert_eq!(back.mode, TradeMode::Live, "the stored mode is kept");
    assert!(!back.live, "the per-session arm never is");
    assert_eq!(start_mode_refusal(back), Some("liveArmRequired"));
    assert!(back.tv.is_some(), "its alert limits come back with it");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stop_all_live_and_a_disarm_drop_the_armed_terms() {
    let bots = BotManager::new();
    let mut tv = BotConfig::new_default(BotKind::Tradingview, "binance");
    (tv.mode, tv.live) = (TradeMode::Live, true);
    bots.configure(tv.clone());
    let terms = crate::bot::live_terms::signal_terms(&tv, &crate::risk::model::RiskLevel::Balanced.limits());
    bots.set_armed_terms(BotKind::Tradingview, terms.clone());
    assert_eq!(bots.armed_terms(BotKind::Tradingview), Some(terms));
    assert!(bots.has_live_exposure(), "an armed TradingView bot is money in play");
    // What "Stop all live" and a terms change run (`disarm_and_stop`).
    bots.disarm(BotKind::Tradingview);
    assert_eq!(bots.armed_terms(BotKind::Tradingview), None);
    let after = bots.config_for(BotKind::Tradingview).unwrap();
    assert!(!after.live && after.mode == TradeMode::Live, "disarmed, still a Live bot");
    assert!(!bots.has_live_exposure());
}
