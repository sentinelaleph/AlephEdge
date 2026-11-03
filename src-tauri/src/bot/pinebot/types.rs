//! What the Pine bot's screens read (bot card status, results table) and
//! what a pending real entry keeps for its lookup. Wire shapes, camelCase.

use serde::{Deserialize, Serialize};

use crate::bot::engine::user_trade::Level;
use crate::bot::model::{OpenPosition, PineAction};

/// One fired trigger on a result row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiredTrigger {
    pub id: String,
    /// The script's title (empty when it gave none; the screen then shows
    /// "plotshape #n").
    pub title: String,
    pub action: PineAction,
}

/// A run error as the bot card shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PineRunError {
    /// The engine's error kind ("syntax", "runtime", "limit", ...), or
    /// "timeout" when the run went past its wall time.
    pub kind: String,
    pub message: String,
    pub line: u32,
    pub col: u32,
    /// Open time of the bar the run was for (ms).
    pub bar_time: u64,
    pub at_ms: u64,
}

/// One symbol's last run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PineSymbolStatus {
    pub symbol: String,
    /// When the last run finished (ms), and the bar it was for (open ms).
    pub last_run_at: Option<u64>,
    pub last_bar_time: Option<u64>,
    pub bars_used: Option<usize>,
    pub run_ms: Option<u64>,
    /// Mapped triggers that fired on the last run.
    pub last_fired: Vec<FiredTrigger>,
    pub last_error: Option<PineRunError>,
    /// Why the last bar was not run or acted on (`pineBarMissing`,
    /// `pineStale`, `pineBarsUnavailable`, ...), cleared by the next run.
    pub last_note: Option<String>,
}

/// The Pine bot's card.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PineStatus {
    pub configured: bool,
    pub running: bool,
    /// `paper` / `live`, when configured.
    pub mode: Option<String>,
    /// Armed for real orders this session.
    pub armed: bool,
    pub script_hash: Option<String>,
    pub timeframe: Option<String>,
    /// `pineHalted|3` while new entries are stopped after failed runs.
    pub halted: Option<String>,
    pub failed_in_a_row: u32,
    pub opens_today: u32,
    pub max_opens_per_day: u32,
    /// Real entries whose outcome is still being looked up.
    pub pending: usize,
    pub pilot_left: u32,
    /// Close time (ms) of the next bar the bot runs on.
    pub next_bar_close: Option<u64>,
    pub symbols: Vec<PineSymbolStatus>,
}

/// One action row (spec section 10): what a bar's triggers did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PineResult {
    /// `pine:<hash>:<symbol>:<bar close ms>`.
    pub key: String,
    pub symbol: String,
    pub script_hash: String,
    pub timeframe: String,
    /// The bar: its open time, its close time (ms) and its close price.
    pub bar_time: u64,
    pub bar_close_ms: u64,
    pub bar_close: f64,
    pub handled_at: u64,
    pub triggers: Vec<FiredTrigger>,
    /// `opened`, `closed`, `flipped`, `livePending|code`, or a refusal code.
    pub outcome: String,
    /// What else held something back (`pineConflict` after a close, ...).
    pub note: Option<String>,
    /// `long` / `short` when an open was planned.
    pub side: Option<String>,
    pub bot_name: String,
    /// `paper` / `live`.
    pub mode: String,
    pub position_id: Option<String>,
    pub requested_qty: Option<f64>,
    pub filled_qty: Option<f64>,
    pub fill_price: Option<f64>,
    /// (fill − bar close) / bar close in %, positive = worse for the trade.
    pub slippage_pct: Option<f64>,
    /// Stop and target percents the entry used.
    pub stop_pct: Option<f64>,
    pub target_pct: Option<f64>,
}

/// A real entry whose outcome stayed unknown: what was sent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinePending {
    pub planned: OpenPosition,
    pub tp: Level,
    pub sl: Level,
    pub sent_at: u64,
}
