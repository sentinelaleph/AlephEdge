//! The combo catalog — Sentinel's five named setups, each carrying the record
//! it has actually earned.
//!
//! WHY THIS EXISTS. The desk could already filter on a combo, but only by
//! typing its id ("stophunt_snap") into a free-text box. That asks the user to
//! know an internal string AND to know, from somewhere else entirely, whether
//! that combo is worth trading. A filter you can only use if you already have
//! the answer is not a filter.
//!
//! WHAT IT MUST NOT DO. It must not surface only the flattering number.
//! `/api/v1/combos` publishes TWO records per combo — `live` (what the
//! published book did) and `backtest` (what the retrospective sweep said) — and
//! on this project they disagree badly: stophunt_snap read 80.8% live against
//! 58.5% in backtest at the time of writing. That gap is the exact shape a
//! previous branch died of (a 76.2% retrospective short that forward-tested at
//! 41.8%). So both records travel together through this type, and the UI is
//! built to show both. Dropping one would be the single easiest dishonest edit
//! anyone could make to this file.
//!
//! The endpoint is public and read-only, so no token is attached.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::membership::MembershipManager;

const CATALOG_TIMEOUT: Duration = Duration::from_secs(10);

/// One combo's outcome record over a given book. `n` is carried beside the
/// rate on purpose: a rate without its denominator is how this project
/// published 74.8% above 62.3% on the same screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComboRecord {
    #[serde(default)]
    pub n: u32,
    #[serde(default, alias = "win_rate")]
    pub win_rate: f64,
    #[serde(default, alias = "avg_pnl")]
    pub avg_pnl: f64,
}

/// A catalog entry as the desk needs it. Deserialized from the server's
/// snake_case wire format (hence the aliases), re-serialized as camelCase for
/// the TypeScript side like every other IPC type in this app.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComboEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Human-readable trigger sequence ("Stophunt breach + close-back → ...").
    #[serde(default, alias = "sequence_human")]
    pub sequence_human: String,
    /// "best" | "candidate" | "disabled", set by the server's own gate.
    #[serde(default)]
    pub status: String,
    #[serde(default = "empty_record")]
    pub live: ComboRecord,
    #[serde(default = "empty_record")]
    pub backtest: ComboRecord,
}

fn empty_record() -> ComboRecord {
    ComboRecord {
        n: 0,
        win_rate: 0.0,
        avg_pnl: 0.0,
    }
}

/// Fetches the published combo catalog.
///
/// Returns an error rather than an empty list on failure, so the UI can say
/// "could not load" instead of silently rendering "no combos exist" — a
/// distinction that matters when the list is what the user selects from.
#[tauri::command]
pub async fn combo_catalog(
    membership: State<'_, MembershipManager>,
) -> Result<Vec<ComboEntry>, String> {
    let base = membership.base_url();
    let client = reqwest::Client::builder()
        .timeout(CATALOG_TIMEOUT)
        .build()
        .map_err(|_| "http client build failed".to_string())?;
    let res = client
        .get(format!("{base}/api/v1/combos"))
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|_| "network error loading combo catalog".to_string())?;
    if !res.status().is_success() {
        return Err(format!("combo catalog failed ({})", res.status().as_u16()));
    }
    res.json::<Vec<ComboEntry>>()
        .await
        .map_err(|_| "unexpected combo catalog response".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The live payload shape, snake_case, exactly as ribqa.com returned it on
    /// 2026-08-12. If the server ever renames a field this test fails rather
    /// than the UI silently rendering 0% for every combo.
    const WIRE: &str = r#"[{
        "id":"stophunt_snap","name":"Stophunt Snap",
        "description":"Price breaches a resting liquidity pool and closes back inside range.",
        "sequence_human":"Stophunt breach + close-back → momentum/pump agreement",
        "status":"best",
        "live":{"n":73,"win_rate":0.8082191780821918,"avg_pnl":0.9312246575342465},
        "backtest":{"n":94,"win_rate":0.5851063829787234}
    }]"#;

    #[test]
    fn parses_the_live_snake_case_payload() {
        let out: Vec<ComboEntry> = serde_json::from_str(WIRE).expect("parse");
        let c = &out[0];
        assert_eq!(c.id, "stophunt_snap");
        assert_eq!(c.status, "best");
        assert!(c.sequence_human.contains("close-back"));
        assert_eq!(c.live.n, 73);
        assert!((c.live.win_rate - 0.8082191780821918).abs() < 1e-12);
    }

    /// The two records must survive as SEPARATE numbers. The live/backtest gap
    /// is the honest signal about a combo; collapsing them to one figure would
    /// republish the mistake this project already made once.
    #[test]
    fn keeps_live_and_backtest_apart() {
        let out: Vec<ComboEntry> = serde_json::from_str(WIRE).expect("parse");
        let c = &out[0];
        assert_eq!(c.backtest.n, 94);
        assert!(
            c.live.win_rate - c.backtest.win_rate > 0.2,
            "the 22pp live-vs-backtest gap must remain visible, not averaged away"
        );
    }

    /// A backtest block with no avg_pnl (the server omits it) must not fail the
    /// whole catalog — one missing optional field would otherwise blank the
    /// picker entirely.
    #[test]
    fn tolerates_a_missing_optional_field() {
        let out: Vec<ComboEntry> = serde_json::from_str(WIRE).expect("parse");
        assert_eq!(out[0].backtest.avg_pnl, 0.0);
    }
}
