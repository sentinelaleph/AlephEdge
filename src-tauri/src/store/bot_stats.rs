//! Closed-trade stats per signal bot (Dashboard, Signal bots). Desktop only;
//! the arithmetic is `trades::aggregate`, the same as the totals, so the
//! per-bot rows always add up to the desk figures.

use rusqlite::Connection;
use serde::Serialize;

use super::model::PnlStats;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BotStats {
    pub bot_kind: String,
    pub stats: PnlStats,
}

/// Stats per `bot_kind` over real (`Some(true)`), simulated (`Some(false)`)
/// or all trades, bots in name order.
pub fn by_bot(conn: &Connection, utc_day_start_ms: u64, live: Option<bool>) -> Result<Vec<BotStats>, String> {
    // On the figures the desk shows: a recalculated paper trade counts as
    // recalculated (`store::recalc`).
    super::recalc::shown_by_bot(conn, utc_day_start_ms, live)
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::super::trades;
    use super::by_bot;

    fn trade(kind: &str, pnl: f64, reason: &str, at: u64, live: bool) -> super::super::model::TradeRecord {
        let mut t: super::super::model::TradeRecord = serde_json::from_value(serde_json::json!({
            "id": 0, "signalId": format!("{kind}-{at}"), "botKind": kind, "exchangeId": "binance",
            "symbol": "SOLUSDT", "direction": "long", "entry": 100.0, "exit": 101.0, "leverage": 1,
            "capital": 100.0, "pnlPct": pnl, "pnlQuote": pnl, "exitReason": reason,
            "frAtOpen": null, "ldAtOpen": null, "openedAt": at - 1, "closedAt": at
        }))
        .unwrap();
        t.live = live;
        t
    }

    #[test]
    fn per_bot_rows_add_up_to_the_desk_totals() {
        let conn = Connection::open_in_memory().unwrap();
        trades::migrate(&conn).unwrap();
        super::super::recalc::migrate(&conn).unwrap();
        for t in [
            trade("spot", 2.0, "tp", 10, false),
            trade("spot", -1.0, "sl", 20, false),
            trade("futures", -3.0, "sl", 30, false),
            trade("futures", 5.0, "tp", 40, true),
        ] {
            trades::insert(&conn, &t).unwrap();
        }
        let paper = by_bot(&conn, 25, Some(false)).unwrap();
        let kinds: Vec<_> = paper.iter().map(|b| b.bot_kind.as_str()).collect();
        assert_eq!(kinds, ["futures", "spot"]);
        let spot = &paper[1].stats;
        assert_eq!((spot.total_trades, spot.wins, spot.losses), (2, 1, 1));
        assert!((spot.net_pnl_quote - 1.0).abs() < 1e-9);
        assert_eq!(spot.today_pnl_quote, 0.0, "both spot trades closed before the day start");
        let futures = &paper[0].stats;
        assert!((futures.net_pnl_quote + 3.0).abs() < 1e-9, "the live trade is not paper");
        let total = trades::stats_scoped(&conn, 25, Some(false)).unwrap();
        let sum: f64 = paper.iter().map(|b| b.stats.net_pnl_quote).sum();
        assert!((sum - total.net_pnl_quote).abs() < 1e-9);
    }
}
