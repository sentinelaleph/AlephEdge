//! Closed-trade stats per signal bot (Dashboard, Signal bots). Desktop only;
//! the arithmetic is `trades::aggregate`, the same as the totals, so the
//! per-bot rows always add up to the desk figures.

use std::collections::BTreeMap;

use rusqlite::Connection;
use serde::Serialize;

use super::model::PnlStats;
use super::trades;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BotStats {
    pub bot_kind: String,
    pub stats: PnlStats,
}

/// Stats per `bot_kind` over real (`Some(true)`), simulated (`Some(false)`)
/// or all trades, bots in name order.
pub fn by_bot(conn: &Connection, utc_day_start_ms: u64, live: Option<bool>) -> Result<Vec<BotStats>, String> {
    let filter = match live {
        Some(true) => " WHERE live = 1",
        Some(false) => " WHERE live = 0",
        None => "",
    };
    let mut stmt = conn
        .prepare(&format!(
            "SELECT bot_kind, pnl_quote, pnl_pct, exit_reason, closed_at FROM trades{filter} ORDER BY closed_at ASC"
        ))
        .map_err(|_| "storeQueryFailed".to_string())?;
    let rows: Vec<(String, f64, f64, String, u64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        .map_err(|_| "storeQueryFailed".to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "storeRowMalformed".to_string())?;
    let mut per: BTreeMap<String, Vec<(f64, f64, String, u64)>> = BTreeMap::new();
    for (kind, q, pct, reason, at) in rows {
        per.entry(kind).or_default().push((q, pct, reason, at));
    }
    Ok(per
        .into_iter()
        .map(|(bot_kind, rows)| BotStats { stats: trades::aggregate(&rows, utc_day_start_ms), bot_kind })
        .collect())
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
