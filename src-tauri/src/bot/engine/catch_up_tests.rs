//! Paper catch-up: the minutes the book missed are replayed adverse-first,
//! and a paper position follows the close Sentinel graded. The scenarios are
//! the History screen of 26 Oct 2026, where every close of a night with the
//! app shut was booked at the restart price and minute.

use crate::bot::strategy::backtest::Candle;

use super::catch_up::{
    closed_minutes, forced_close, is_sentinel_id, parse_outcome, price_at, replay, SentinelOutcome,
    MINUTE_MS,
};
use super::test_fixtures::{plan, position, signal};

const T0: u64 = 1_793_000_000_000 - 1_793_000_000_000 % MINUTE_MS;

fn k(i: u64, o: f64, h: f64, l: f64, c: f64) -> Candle {
    Candle { open_ms: T0 + i * MINUTE_MS, o, h, l, c }
}

fn long_managed() -> crate::bot::model::OpenPosition {
    // Entry 100, stop 95 (1R = 5), TP1 110; breakeven at +0.5R (102.5),
    // half off at +1R (105).
    position(&signal("long", 100.0, 110.0, 95.0), Some(plan(0.5, 1.0, 0.5)))
}

// RVNUSDT: the stop was touched while the app was closed and price came
// back. Booked at the stop and at that minute, not at the restart price.
#[test]
fn a_stop_touched_in_the_gap_is_booked_at_its_level_and_minute() {
    let mut pos = long_managed();
    let candles = [k(0, 100.0, 101.0, 99.0, 100.5), k(1, 100.5, 100.6, 94.2, 96.0), k(2, 96.0, 99.0, 95.5, 98.0)];
    let (exit, at) = replay(&mut pos, &candles, None, u64::MAX).expect("stopped");
    assert_eq!((exit.reason, exit.price, at), ("sl", 95.0, T0 + MINUTE_MS));
}

#[test]
fn stop_and_target_in_one_minute_is_the_stop() {
    let mut pos = long_managed();
    let (exit, _) = replay(&mut pos, &[k(0, 100.0, 111.0, 94.0, 108.0)], None, u64::MAX).unwrap();
    assert_eq!(exit.reason, "sl");
}

// CAPUSDT: the app booked +2.72% take-profit; Sentinel graded 0.00
// breakeven. Breakeven armed in one minute, the dip to entry came before
// the rally, so the replay books breakeven.
#[test]
fn breakeven_armed_earlier_beats_a_later_target() {
    let mut pos = long_managed();
    let candles = [k(0, 100.0, 103.0, 100.5, 102.8), k(1, 102.8, 111.0, 99.9, 110.5)];
    let (exit, at) = replay(&mut pos, &candles, None, u64::MAX).unwrap();
    assert_eq!((exit.reason, exit.price, at), ("breakeven", 100.0, T0 + MINUTE_MS));
}

#[test]
fn the_breakeven_armed_in_a_minute_does_not_protect_that_minute() {
    let mut pos = long_managed();
    // Arms at 103 and dips to 99.9 in the same minute: the stop is still 95.
    assert!(replay(&mut pos, &[k(0, 100.0, 103.0, 99.9, 101.0)], None, u64::MAX).is_none());
    assert!(pos.breakeven_armed, "armed for the next minute");
}

#[test]
fn a_target_in_the_gap_banks_the_partial_then_exits_at_tp() {
    let mut pos = long_managed();
    let (exit, _) = replay(&mut pos, &[k(0, 100.0, 106.0, 100.2, 105.5), k(1, 105.5, 110.4, 104.0, 110.0)], None, u64::MAX).unwrap();
    assert_eq!((exit.reason, exit.price), ("tp", 110.0));
    assert_eq!(pos.partial_price, Some(105.0));
}

#[test]
fn past_the_horizon_it_closes_at_the_first_open_after_it() {
    let mut pos = long_managed();
    pos.horizon_ms = T0 + 2 * MINUTE_MS;
    let candles = [k(0, 100.0, 101.0, 99.0, 100.0), k(1, 100.0, 101.0, 99.0, 100.0), k(2, 101.0, 102.0, 100.0, 101.5)];
    let (exit, at) = replay(&mut pos, &candles, None, u64::MAX).unwrap();
    assert_eq!((exit.reason, exit.price, at), ("horizon", 101.0, T0 + 2 * MINUTE_MS));
}

#[test]
fn the_replay_stops_where_sentinel_closed() {
    let mut pos = long_managed();
    let candles = [k(0, 100.0, 101.0, 99.0, 100.0), k(1, 100.0, 100.5, 94.0, 95.0)];
    assert!(replay(&mut pos, &candles, None, T0 + MINUTE_MS).is_none(), "the stop came after Sentinel's close");
}

#[test]
fn a_short_is_replayed_on_its_highs_first() {
    let mut pos = position(&signal("short", 100.0, 90.0, 105.0), None);
    let (exit, _) = replay(&mut pos, &[k(0, 100.0, 105.2, 89.0, 95.0)], None, u64::MAX).unwrap();
    assert_eq!((exit.reason, exit.price), ("sl", 105.0));
}

fn outcome(exit_type: &str, market: &str, sl: f64, tp: f64, planned: bool) -> SentinelOutcome {
    parse_outcome(
        &serde_json::json!({"data": {
            "result": "veto_closed", "exit_type": exit_type, "exit_at": "2026-10-26T01:40:00Z",
            "exit_price": 98.4, "market_type": market, "stop_loss": sl, "take_profit": [tp, 120.0],
            "management_plan": if planned { serde_json::json!({"breakeven_at_r": 0.5}) } else { serde_json::Value::Null },
            "invalidation_reason": "btc_regime_turn_bearish"
        }})
        .to_string(),
    )
    .expect("permalink parses")
}

#[test]
fn an_invalidation_is_followed_at_sentinels_price_and_minute() {
    let pos = long_managed();
    let f = forced_close(&pos, &outcome("invalidated", "spot", 95.0, 110.0, true), None, "spot").unwrap();
    assert_eq!((f.reason, f.price, f.code.as_deref()), ("invalidated", Some(98.4), Some("btc_regime_turn_bearish")));
    assert_eq!(f.at_ms, crate::signal::time::parse_rfc3339_ms("2026-10-26T01:40:00Z").unwrap());
}

#[test]
fn an_invalidation_is_followed_even_on_the_users_own_target() {
    let mut pos = long_managed();
    pos.tp_target = "tp2".into();
    pos.tp = 120.0;
    assert_eq!(forced_close(&pos, &outcome("invalidated", "spot", 95.0, 110.0, true), Some(3.0), "spot").unwrap().reason, "invalidated");
}

#[test]
fn another_market_keeps_the_minute_but_not_the_price() {
    let pos = long_managed();
    let f = forced_close(&pos, &outcome("invalidated", "spot", 95.0, 110.0, true), None, "futures").unwrap();
    assert_eq!(f.price, None, "priced from the position's own candles");
}

#[test]
fn stop_target_and_breakeven_are_followed_only_on_sentinels_plan() {
    let pos = long_managed();
    for (exit_type, reason) in [("tp_hit", "tp"), ("sl_hit", "sl"), ("be_scratch", "breakeven")] {
        assert_eq!(forced_close(&pos, &outcome(exit_type, "spot", 95.0, 110.0, true), None, "spot").unwrap().reason, reason);
    }
    // A user loss cap, another stop, another target or no plan: its own rules.
    assert!(forced_close(&pos, &outcome("sl_hit", "spot", 95.0, 110.0, true), Some(2.0), "spot").is_none());
    assert!(forced_close(&pos, &outcome("sl_hit", "spot", 94.0, 110.0, true), None, "spot").is_none());
    assert!(forced_close(&pos, &outcome("tp_hit", "spot", 95.0, 111.0, true), None, "spot").is_none());
    assert!(forced_close(&pos, &outcome("tp_hit", "spot", 95.0, 110.0, false), None, "spot").is_none());
    let mut tp2 = long_managed();
    tp2.tp_target = "tp2".into();
    assert!(forced_close(&tp2, &outcome("tp_hit", "spot", 95.0, 110.0, true), None, "spot").is_none());
}

#[test]
fn nothing_to_follow_while_open_expired_or_before_the_fill() {
    let pos = long_managed();
    assert!(forced_close(&pos, &outcome("", "spot", 95.0, 110.0, true), None, "spot").is_none());
    assert!(forced_close(&pos, &outcome("expired_unresolved", "spot", 95.0, 110.0, true), None, "spot").is_none());
    let mut late = long_managed();
    late.opened_at = crate::signal::time::parse_rfc3339_ms("2026-10-26T02:00:00Z").unwrap();
    assert!(forced_close(&late, &outcome("invalidated", "spot", 95.0, 110.0, true), None, "spot").is_none());
}

#[test]
fn the_live_permalink_shape_parses() {
    // GET /api/public/signals/f82da3c7-… on 26 Oct 2026 (trimmed).
    let body = r#"{"data":{"id":"f82da3c7-1855-4b50-95f1-5a390564aeda","symbol":"X","market_type":"spot",
        "result":"veto_closed","exit_type":"invalidated","exit_at":"2026-10-26T01:40:00Z","exit_price":0.00819,
        "stop_loss":0.0081,"take_profit":[0.00885986,0.00905979],"management_plan":{"breakeven_at_r":0.5},
        "invalidation_reason":"btc_regime_turn_bearish","engines":[],"news":[]}}"#;
    let o = parse_outcome(body).unwrap();
    assert_eq!((o.exit_type.as_str(), o.exit_price, o.market_type.as_str()), ("invalidated", Some(0.00819), "spot"));
    assert!(parse_outcome(r#"{"error":"not found"}"#).is_none());
}

#[test]
fn price_at_takes_the_minute_holding_the_moment() {
    let candles = [k(0, 1.0, 1.0, 1.0, 1.1), k(1, 1.1, 1.2, 1.0, 1.2)];
    assert_eq!(price_at(&candles, T0 + MINUTE_MS + 30_000), Some(1.2));
    assert_eq!(price_at(&candles, T0 + 59_999), Some(1.1));
    assert_eq!(price_at(&candles, T0 + 3 * MINUTE_MS), None);
}

#[test]
fn only_closed_well_formed_minutes_are_replayed() {
    let raw = vec![k(2, 1.0, 1.0, 1.0, 1.0), k(0, 1.0, 1.0, 1.0, 1.0), k(0, 1.0, 1.0, 1.0, 1.0), k(1, 1.0, 0.5, 0.9, 1.0)];
    let kept = closed_minutes(raw, T0 + 2 * MINUTE_MS + 10);
    assert_eq!(kept.iter().map(|c| c.open_ms).collect::<Vec<_>>(), vec![T0], "forming, duplicate and high<low dropped");
}

#[test]
fn only_sentinel_ids_have_a_permalink() {
    assert!(is_sentinel_id("f82da3c7-1855-4b50-95f1-5a390564aeda"));
    assert!(!is_sentinel_id("tv-17930000-BTCUSDT"));
    assert!(!is_sentinel_id("sig-1"));
}

#[test]
fn last_seen_is_saved_only_once_set() {
    let mut pos = long_managed();
    assert!(!serde_json::to_string(&pos).unwrap().contains("lastSeenMs"), "farm rows unchanged");
    pos.last_seen_ms = T0;
    assert!(serde_json::to_string(&pos).unwrap().contains("lastSeenMs"));
    let back: crate::bot::model::OpenPosition = serde_json::from_str(&serde_json::to_string(&pos).unwrap()).unwrap();
    assert_eq!(back.last_seen_ms, T0);
}
