//! Live isolation: the DCA / Grid SIMULATION (this subtree) never reaches the
//! exchange; it is the decision book, and its rows stay paper rows. Real money
//! for these bots goes only through `bot/strategy_live.rs` (live builds, typed
//! LIVE per bot), which mirrors the simulated position with market orders.
//! These tests hold under `--features live` too.

use std::path::Path;

use super::limits::STRATEGY_LIVE_ALLOWED;
use super::model::StrategyConfig;

/// Forbidden references, assembled so this file does not match itself.
fn forbidden() -> Vec<String> {
    vec![
        ["live", "_orders"].concat(),
        ["binance", "_requests"].concat(),
        ["binance", "_auth"].concat(),
        ["X-MBX", "-APIKEY"].concat(),
        ["sign", "_request"].concat(),
        ["exchange", "_close_position"].concat(),
        ["exchange", "_close_all"].concat(),
        ["Live", "Venue"].concat(),
    ]
}

#[test]
fn strategy_sources_never_reference_signed_or_real_order_code() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bot/strategy");
    let mut scanned = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for f in forbidden() {
            assert!(!text.contains(&f), "{} references {f}", path.display());
        }
        scanned += 1;
    }
    assert!(scanned >= 15, "scanned {scanned} files");
    // the kline provider is keyless too
    let klines = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/exchange/providers/klines.rs")).unwrap();
    for f in [["binance", "_auth"].concat(), ["binance", "_requests"].concat(), ["live", "_orders"].concat()] {
        assert!(!klines.contains(&f));
    }
}

#[test]
fn no_strategy_config_can_carry_a_live_flag() {
    const _: () = assert!(!STRATEGY_LIVE_ALLOWED);
    // A config that smuggles "live": true parses (unknown fields are ignored)
    // but has nowhere to keep it: re-serialised, the flag is gone.
    let mut v = serde_json::to_value(super::commands::default_config(
        super::model::StrategyKind::Dca,
        "binance".into(),
        "BTCUSDT".into(),
    ))
    .unwrap();
    v["live"] = serde_json::json!(true);
    let cfg: StrategyConfig = serde_json::from_value(v).unwrap();
    let back = serde_json::to_value(&cfg).unwrap();
    assert!(back.get("live").is_none());
}

#[test]
fn strategy_rows_are_paper_by_schema() {
    let c = rusqlite::Connection::open_in_memory().unwrap();
    crate::store::strategy::migrate(&c).unwrap();
    let sql: String = c
        .query_row("SELECT sql FROM sqlite_master WHERE name = 'strategy_bots'", [], |r| r.get(0))
        .unwrap();
    assert!(sql.contains("CHECK(paper = 1)"));
}

#[test]
fn backtest_candles_come_from_datahub_only() {
    // The backtest and its history client never reach an exchange: no kline
    // provider, no exchange module, no exchange host.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = ["bot/strategy/backtest.rs", "bot/strategy/backtest_commands.rs", "market/history.rs"];
    let banned = [
        ["providers", "::klines"].concat(),
        ["crate::", "exchange"].concat(),
        ["Exchange", "Manager"].concat(),
        ["api.", "binance"].concat(),
        ["fapi.", "binance"].concat(),
    ];
    for f in files {
        let text = std::fs::read_to_string(root.join(f)).unwrap();
        for b in &banned {
            assert!(!text.contains(b.as_str()), "{f} references {b}");
        }
    }
    let client = std::fs::read_to_string(root.join("market/history.rs")).unwrap();
    assert!(client.contains("/api/v1/market/candles/history"));
}

/// Owner decision 2026-10-01: the signal bots' daily stop neither counts
/// strategy P&L nor stops, closes or holds strategy bots. Their own budget
/// cap and the portfolio breaker govern them.
#[test]
fn signal_daily_stop_is_separate_from_strategy_bots() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bot");
    let engine = std::fs::read_to_string(root.join("engine/mod.rs")).unwrap().replace("\r\n", "\n");
    let start = engine.find("async fn kill_switch_phase").expect("kill_switch_phase");
    let end = start + engine[start..].find("\n}\n").expect("fn end");
    let body = &engine[start..end];
    for f in ["day_pnl_quote(", "StrategyManager", "strategy::engine", "force_close_all"] {
        assert!(!body.contains(f), "daily stop references {f}");
    }
    assert!(body.contains("force_close_kinds"), "daily stop closes signal bots only");
    for file in ["strategy/engine.rs", "strategy/commands.rs"] {
        let text = std::fs::read_to_string(root.join(file)).unwrap();
        assert!(!text.contains("kill_switch_tripped"), "{file} reads the signal daily stop");
    }
}
