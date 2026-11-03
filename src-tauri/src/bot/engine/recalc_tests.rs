//! Recalculating paper trades booked before the catch-up, on the History
//! screen of 26 Oct 2026: closes booked at the restart minute although
//! Sentinel had closed the signal hours earlier.

use rusqlite::Connection;

use crate::store::model::TradeRecord;

use super::catch_up::{parse_outcome, SentinelOutcome};
use super::recalc::{recalculate, Verdict};
use super::record::trade_record;
use super::test_fixtures::{position, signal};

const OPEN: u64 = 1_792_970_000_000;
const HOUR: u64 = 3_600_000;

/// A paper trade from 100 booked at `exit` / `reason`, `closed_h` hours in.
fn booked(exit: f64, reason: &str, closed_h: u64) -> TradeRecord {
    let mut pos = position(&signal("long", 100.0, 110.0, 95.0), None);
    pos.opened_at = OPEN;
    let mut t = trade_record(&pos, exit, reason, OPEN + closed_h * HOUR);
    t.id = 7;
    t
}

fn graded(exit_type: &str, price: f64, at_h: u64, market: &str, partial: bool) -> SentinelOutcome {
    let at = crate::bot::strategy::backtest::rfc3339(OPEN + at_h * HOUR);
    parse_outcome(
        &serde_json::json!({"data": {
            "exit_type": exit_type, "exit_at": at, "exit_price": price, "market_type": market,
            "invalidation_reason": "btc_regime_turn_bearish",
            "execution": {"partial_reachable": partial}
        }})
        .to_string(),
    )
    .unwrap()
}

// RVNUSDT: booked as a stop at the restart minute (-8.39%); Sentinel had
// invalidated it hours earlier at a far smaller loss.
#[test]
fn a_missed_invalidation_is_recalculated_on_sentinels_exit() {
    let t = booked(91.6, "sl", 10);
    let Some(Verdict::Recalculated(r)) = recalculate(&t, &graded("invalidated", 96.67, 2, "futures", false)) else {
        panic!("recalculated");
    };
    assert_eq!((r.exit, r.exit_reason.as_str(), r.closed_at), (96.67, "invalidated", OPEN + 2 * HOUR));
    assert!(r.pnl_pct > t.pnl_pct, "a smaller loss than booked");
    // Same NET formula as a close at that price.
    let mut pos = position(&signal("long", 100.0, 110.0, 95.0), None);
    pos.opened_at = OPEN;
    let same = trade_record(&pos, 96.67, "invalidated", OPEN + 2 * HOUR);
    assert!((r.pnl_pct - same.pnl_pct).abs() < 1e-9 && r.unlevered_net_pct == same.unlevered_net_pct);
}

// CAPUSDT: booked as a +2.72% target; Sentinel graded breakeven earlier.
#[test]
fn a_missed_breakeven_replaces_a_late_target_when_no_partial_could_fire() {
    let t = booked(102.9, "tp", 9);
    let Some(Verdict::Recalculated(r)) = recalculate(&t, &graded("be_scratch", 100.0, 3, "futures", false)) else {
        panic!("recalculated");
    };
    assert_eq!(r.exit_reason, "breakeven");
    assert_eq!(recalculate(&t, &graded("be_scratch", 100.0, 3, "futures", true)), Some(Verdict::Kept("partial")));
}

#[test]
fn a_close_within_two_minutes_of_sentinels_matches() {
    let t = booked(96.67, "invalidated", 2);
    assert_eq!(recalculate(&t, &graded("invalidated", 96.67, 2, "futures", false)), Some(Verdict::Kept("matches")));
}

#[test]
fn what_the_row_cannot_prove_is_kept_as_booked() {
    let mut own_target = booked(91.6, "sl", 10);
    own_target.tp_target = Some("tp2".into());
    assert_eq!(recalculate(&own_target, &graded("sl_hit", 95.0, 2, "futures", false)), Some(Verdict::Kept("ownTarget")));
    // An invalidation is followed whatever the target.
    assert!(matches!(recalculate(&own_target, &graded("invalidated", 96.0, 2, "futures", false)), Some(Verdict::Recalculated(_))));

    let user_exit = booked(97.0, "max_loss", 10);
    assert_eq!(recalculate(&user_exit, &graded("invalidated", 96.0, 2, "futures", false)), Some(Verdict::Kept("userExit")));
    let other_market = booked(91.6, "sl", 10);
    assert_eq!(recalculate(&other_market, &graded("invalidated", 96.0, 2, "spot", false)), Some(Verdict::Kept("otherMarket")));
    let mut live = booked(91.6, "sl", 10);
    live.live = true;
    assert_eq!(recalculate(&live, &graded("invalidated", 96.0, 2, "futures", false)), Some(Verdict::Kept("live")));
}

#[test]
fn nothing_graded_yet_is_asked_again_and_expired_is_kept() {
    let t = booked(91.6, "sl", 10);
    assert_eq!(recalculate(&t, &graded("", 0.0, 2, "futures", false)), None);
    assert_eq!(recalculate(&t, &graded("expired_unresolved", 0.0, 2, "futures", false)), Some(Verdict::Kept("expired")));
}

fn store() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    crate::store::migrate_all(&c).unwrap();
    c
}

// The original row stays; History and the desk stats read the
// recalculation; the kill switch's booked figures do not move.
#[test]
fn the_recalculation_sits_beside_the_row_and_feeds_the_shown_stats() {
    use crate::store::recalc;
    let c = store();
    let t = booked(91.6, "sl", 10);
    crate::store::trades_insert_for_tests(&c, &t);
    let id: i64 = c.query_row("SELECT id FROM trades", [], |r| r.get(0)).unwrap();
    let pending = recalc::pending(&c, OPEN + 11 * HOUR, 10).unwrap();
    assert_eq!(pending.len(), 1);
    let Some(Verdict::Recalculated(r)) = recalculate(&pending[0], &graded("invalidated", 96.67, 2, "futures", false)) else {
        panic!()
    };
    recalc::save_recalc(&c, id, &r, OPEN + 11 * HOUR).unwrap();
    assert!(recalc::pending(&c, OPEN + 11 * HOUR, 10).unwrap().is_empty(), "checked once");

    let mut rows = crate::store::trades_list_for_tests(&c);
    recalc::mark(&c, &mut rows).unwrap();
    assert_eq!(rows[0].exit, 91.6, "the booked row is unchanged");
    assert_eq!(rows[0].recalc.as_ref().unwrap().exit, 96.67);

    let shown = recalc::shown_stats(&c, 0, Some(false)).unwrap();
    let booked_stats = crate::store::trades_stats_for_tests(&c, Some(false));
    assert!(shown.net_pnl_quote > booked_stats.net_pnl_quote, "shown on the recalculated close; booked stays");
    assert_eq!(crate::store::bot_stats::by_bot(&c, 0, Some(false)).unwrap()[0].stats.net_pnl_quote, shown.net_pnl_quote);
}

#[test]
fn a_kept_trade_is_not_checked_again() {
    use crate::store::recalc;
    let c = store();
    crate::store::trades_insert_for_tests(&c, &booked(91.6, "sl", 10));
    let id: i64 = c.query_row("SELECT id FROM trades", [], |r| r.get(0)).unwrap();
    recalc::save_kept(&c, id, "matches", 1).unwrap();
    assert!(recalc::pending(&c, u64::MAX, 10).unwrap().is_empty());
    let mut rows = crate::store::trades_list_for_tests(&c);
    recalc::mark(&c, &mut rows).unwrap();
    assert!(rows[0].recalc.is_none());
}
