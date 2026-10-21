//! Update channel facts since 0.3.19: ONE build. The desktop app always has
//! the real-money order path (`LIVE_TRADING_ENABLED`, lib.rs refuses to
//! compile without it), the user picks Paper or Live per bot, and the one
//! identifier reads the one signed feed. So in-app Install can no longer put
//! a paper build over a LIVE install (the gap these tests pinned in 0.3.18),
//! and the live-exposure guard that stood in front of it is gone.

use serde_json::Value;

use crate::app::update::{install_refusal, updates_off, UPDATE_CONFIRMATION};
use crate::bot::model::{BotConfig, BotKind, TradeMode, LIVE_TRADING_ENABLED};
use crate::bot::strategy_live::{start_refusal_of, LiveState};
use crate::bot::{start_mode_refusal, BotManager};

#[test]
fn one_build_reads_one_feed_under_one_identity() {
    // The build the feed ships is the build every install runs.
    const { assert!(LIVE_TRADING_ENABLED) };
    let conf: Value = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
    let id = conf["identifier"].as_str().unwrap();
    assert_eq!(id, "com.sentinelaleph.edge");
    assert!(!updates_off(id));
    let feeds = conf["plugins"]["updater"]["endpoints"].as_array().unwrap();
    assert_eq!(feeds.len(), 1, "one signed feed");
    assert!(feeds[0].as_str().unwrap().ends_with("/latest.json"));
    // The TESTNET identity stays separate and never installs the feed.
    let testnet: Value = serde_json::from_str(include_str!("../../tauri.testnet.conf.json")).unwrap();
    assert!(updates_off(testnet["identifier"].as_str().unwrap()));
}

#[test]
fn install_needs_the_word_and_a_normal_identity_and_nothing_about_money() {
    assert_eq!(install_refusal("com.sentinelaleph.edge", UPDATE_CONFIRMATION), None);
    assert!(install_refusal("com.sentinelaleph.edge", "update").is_some_and(|e| e.starts_with("updateConfirmRequired|")));
    assert_eq!(install_refusal("com.sentinelaleph.edge.testnet", UPDATE_CONFIRMATION).as_deref(), Some("updateOffTestnet"));
}

#[test]
fn a_restarted_live_signal_bot_is_still_a_live_bot_and_waits_for_its_typed_live() {
    // What restore_configs leaves for a Live bot: the stored mode, the arm
    // cleared. Never a silent Paper bot, never real orders without LIVE.
    let bots = BotManager::new();
    let mut cfg = BotConfig::new_default(BotKind::Futures, "binance");
    cfg.mode = TradeMode::Live;
    cfg.live = false;
    bots.configure(cfg.clone());
    assert_eq!(bots.config_for(BotKind::Futures).unwrap().mode, TradeMode::Live);
    assert_eq!(start_mode_refusal(&cfg), Some("liveArmRequired"));
    // Disarmed and flat: not money in play (the vault may lock), but still Live.
    assert!(!bots.has_live_exposure());
    // The stored mode round-trips through the saved config.
    let back: BotConfig = serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
    assert_eq!(back.mode, TradeMode::Live);
}

#[test]
fn a_restarted_live_dca_bot_starts_again_only_after_its_typed_live() {
    let st = LiveState { enabled: true, ..LiveState::default() };
    assert_eq!(start_refusal_of(&st, false, true, true), Some("liveArmRequired"));
    assert_eq!(start_refusal_of(&st, true, true, true), None);
}
