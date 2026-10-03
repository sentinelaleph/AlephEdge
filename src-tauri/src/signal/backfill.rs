//! Reconnect backfill: the SSE stream only carries signals published while we
//! are connected, so every (re)connect first pulls Sentinel's public pending
//! list. Without it a signal published during a network blip or before app
//! start is invisible, and the paper book silently diverges from the ledger.

use std::time::Duration;

use reqwest::Client;

use super::model::{RegimeVetoEvent, Signal};

const BACKFILL_TIMEOUT: Duration = Duration::from_secs(10);
const BACKFILL_LIMIT: u32 = 300;

/// One row of the pending list: a parsed signal or the reason it failed,
/// with whatever id/symbol could be read for the visible note.
pub enum Row {
    /// Boxed: a `Signal` is far larger than the error arm, and a 300-row page
    /// would otherwise reserve that size for every row.
    Ok(Box<Signal>),
    Bad {
        id: String,
        symbol: String,
        error: String,
    },
}

/// The pending list and Sentinel's `total`. When `total` exceeds the rows
/// returned, the page holds only the newest pending signals.
pub struct PendingPage {
    pub rows: Vec<Row>,
    pub total: Option<u64>,
}

/// Puts a page's parsed rows in publication order (oldest first), by parsed
/// `created_at` — not text order, which breaks on mixed offsets or fraction
/// lengths. Sentinel serves the pending list NEWEST first; ingested in that
/// order the newest signal of a batch was buffered first and so read back as
/// the OLDEST, which turned the "freshest signal first" rule upside down for
/// every backfill and sweep. Unparseable timestamps sort first (oldest);
/// rows that failed to parse keep their place at the front.
pub fn chronological(rows: &mut [Row]) {
    rows.sort_by_key(|row| match row {
        Row::Ok(sig) => (1u8, super::time::parse_rfc3339_ms(&sig.created_at).unwrap_or(0)),
        Row::Bad { .. } => (0, 0),
    });
}

/// GET {base}/api/public/signals/pending?limit=300 (public, no auth).
pub async fn fetch_pending(client: &Client, base: &str) -> Result<PendingPage, String> {
    let res = client
        .get(format!(
            "{base}/api/public/signals/pending?limit={BACKFILL_LIMIT}"
        ))
        .header("Accept", "application/json")
        .timeout(BACKFILL_TIMEOUT)
        .send()
        .await
        .map_err(|_| "backfillPendingUnreachable".to_string())?;
    if !res.status().is_success() {
        return Err(format!("backfillPendingFailed|{}", res.status().as_u16()));
    }
    let body = res
        .text()
        .await
        .map_err(|_| "backfillPendingUnreadable".to_string())?;
    parse_pending_page(&body)
}

/// `parse_pending_body` plus the `total` field.
pub fn parse_pending_page(body: &str) -> Result<PendingPage, String> {
    let rows = parse_pending_body(body)?;
    let total = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("total").and_then(|t| t.as_u64()));
    Ok(PendingPage { rows, total })
}

/// Parses `{"signals":[...], "data":[...], "total":N}`. Rows are parsed one
/// by one so a single malformed signal cannot discard the whole page.
pub fn parse_pending_body(body: &str) -> Result<Vec<Row>, String> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("backfillPendingMalformed|{e}"))?;
    let rows = ["signals", "data"]
        .iter()
        .filter_map(|k| v.get(*k).and_then(|a| a.as_array()))
        .find(|a| !a.is_empty())
        .cloned()
        .unwrap_or_default();
    Ok(rows.into_iter().map(parse_row).collect())
}

/// Sentinel's regime-veto codes (`db/regime_veto_repo.go`); the vetoed list
/// also carries legacy guard codes, which only block entry and are handled by
/// the invalidation path, never closed on.
pub const REGIME_VETO_CODES: &[&str] = &[
    "btc_regime_turn_bullish",
    "btc_regime_turn_bearish",
    "btc_extreme_move_up",
    "btc_extreme_move_down",
];

/// GET {base}/api/public/signals/vetoed (public): the vetoes issued while
/// the stream was down, as veto events. Legacy guard rows are skipped.
pub async fn fetch_vetoed(client: &Client, base: &str) -> Result<Vec<RegimeVetoEvent>, String> {
    let res = client
        .get(format!("{base}/api/public/signals/vetoed?limit={BACKFILL_LIMIT}"))
        .header("Accept", "application/json")
        .timeout(BACKFILL_TIMEOUT)
        .send()
        .await
        .map_err(|_| "backfillVetoedUnreachable".to_string())?;
    if !res.status().is_success() {
        return Err(format!("backfillVetoedFailed|{}", res.status().as_u16()));
    }
    let body = res
        .text()
        .await
        .map_err(|_| "backfillVetoedUnreadable".to_string())?;
    parse_vetoed_body(&body)
}

/// Parses `{"data":[{id, symbol, direction, invalidation_reason,
/// invalidated_at}, ...]}` into regime-veto events.
pub fn parse_vetoed_body(body: &str) -> Result<Vec<RegimeVetoEvent>, String> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("backfillVetoedMalformed|{e}"))?;
    let rows = v.get("data").and_then(|a| a.as_array()).cloned().unwrap_or_default();
    Ok(rows
        .iter()
        .filter_map(|r| {
            let text = |k: &str| r.get(k).and_then(|s| s.as_str()).map(str::to_string);
            let code = text("invalidation_reason")?;
            if !REGIME_VETO_CODES.contains(&code.as_str()) {
                return None;
            }
            Some(RegimeVetoEvent {
                signal_id: text("id")?,
                symbol: text("symbol").unwrap_or_default(),
                direction: r
                    .get("direction")
                    .and_then(|d| serde_json::from_value(d.clone()).ok()),
                reason_text: veto_text(&code).to_string(),
                reason_code: code,
                trigger: String::new(),
                btc_price: 0.0,
                btc_trend: String::new(),
                btc_strength: 0.0,
                btc_change_1h_pct: 0.0,
                vetoed_at: text("invalidated_at").unwrap_or_default(),
            })
        })
        .collect())
}

/// The desk note for a backfilled veto (the SSE frame carries its own text).
fn veto_text(code: &str) -> &'static str {
    match code {
        "btc_regime_turn_bullish" => "BTC turned bullish",
        "btc_regime_turn_bearish" => "BTC turned bearish",
        "btc_extreme_move_up" => "BTC extreme move up",
        _ => "BTC extreme move down",
    }
}

/// Parses one signal JSON value, keeping id/symbol for the error note.
pub fn parse_row(value: serde_json::Value) -> Row {
    let text = |k: &str| {
        value
            .get(k)
            .and_then(|s| s.as_str())
            .unwrap_or("—")
            .to_string()
    };
    let (id, symbol) = (text("id"), text("symbol"));
    match serde_json::from_value::<Signal>(value) {
        Ok(sig) => Row::Ok(Box::new(sig)),
        Err(e) => Row::Bad {
            id,
            symbol,
            error: e.to_string(),
        },
    }
}

#[cfg(test)]
mod vetoed_tests {
    use super::*;

    #[test]
    fn vetoed_list_keeps_regime_vetoes_and_drops_guard_codes() {
        let body = r#"{"data":[
            {"id":"a","symbol":"XUSDT","direction":"short","invalidation_reason":"btc_regime_turn_bullish","invalidated_at":"2026-09-22T10:00:00Z"},
            {"id":"b","symbol":"YUSDT","direction":"long","invalidation_reason":"btc_break_guard"},
            {"symbol":"ZUSDT","invalidation_reason":"btc_extreme_move_down"}
        ]}"#;
        let v = parse_vetoed_body(body).unwrap();
        assert_eq!(v.len(), 1, "guard code and id-less row dropped");
        assert_eq!(v[0].signal_id, "a");
        assert_eq!(v[0].reason_code, "btc_regime_turn_bullish");
        assert_eq!(v[0].reason_text, "BTC turned bullish");
    }
}
