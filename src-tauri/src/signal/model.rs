//! Signal types.
//!
//! `Signal` mirrors the real Sentinel wire format 1:1 (see
//! `sentinel-alephv2/frontend/src/types/signal.ts`) — field names stay
//! snake_case on purpose, matching the actual JSON the backend emits, rather
//! than our usual camelCase convention for types we design ourselves.
//!
//! Parsing is deliberately tolerant: a strict enum for `mode` or a non-null
//! `tp`/`confluence` used to make one unexpected value drop the whole signal
//! silently (the stream ignored parse errors).

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Long,
    Short,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfluenceItem {
    pub source: String,
    #[serde(default)]
    pub score: f64,
    #[serde(default)]
    pub weighted: f64,
}

/// Sentinel's position-management plan. Every level is in R, where
/// R = |entry − sl| of the SIGNAL. Absent fields disable that leg.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ManagementPlan {
    #[serde(default)]
    pub breakeven_at_r: Option<f64>,
    #[serde(default)]
    pub partial_at_r: Option<f64>,
    #[serde(default)]
    pub partial_fraction: Option<f64>,
}

/// A live trade signal exactly as Sentinel emits it on the SSE stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    pub id: String,
    pub symbol: String,
    #[serde(default)]
    pub timeframe: Option<String>,
    /// "spot" | "futures": the Binance market the signal was published
    /// on (its candles, its outcome). Absent on older payloads.
    #[serde(default)]
    pub market_type: Option<String>,
    pub direction: Direction,
    /// "smc_only" | "ind_only" | "hybrid" | … — open-ended on purpose: a new
    /// mode upstream must not make every signal unparseable.
    #[serde(default)]
    pub mode: String,
    pub entry: f64,
    /// TP ladder; TP1 = tp[0]. JSON null reads as empty (refused later as
    /// incoherent geometry, never a crash).
    #[serde(default, deserialize_with = "null_as_empty")]
    pub tp: Vec<f64>,
    pub sl: f64,
    #[serde(default)]
    pub confidence: f64,
    /// "Bull" | "Bear" | "Sideways" | "Transition" — display-only.
    #[serde(default)]
    pub regime: Option<String>,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub confluence: Vec<ConfluenceItem>,
    /// Most-specific matched confluence combo ("stophunt_snap", …) or absent.
    #[serde(default)]
    pub combo: Option<String>,
    #[serde(default)]
    pub rr: f64,
    #[serde(default)]
    pub investment_score: Option<f64>,
    /// True when this signal replaces older ones for the same
    /// symbol+timeframe; buffered unfilled predecessors are dropped.
    #[serde(default)]
    pub regenerated: bool,
    #[serde(default)]
    pub regenerated_at: Option<String>,
    #[serde(default)]
    pub management_plan: Option<ManagementPlan>,
    #[serde(default)]
    pub instrumentation: Option<serde_json::Map<String, serde_json::Value>>,
    pub expires_at: String,
    pub created_at: String,
}

impl Signal {
    pub fn tp1(&self) -> Option<f64> {
        self.tp.first().copied()
    }

    /// Key used for regeneration and the one-position-per-market rule.
    pub fn market_key(&self) -> (String, String) {
        (
            self.symbol.clone(),
            self.timeframe.clone().unwrap_or_default(),
        )
    }
}

fn null_as_empty<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

/// What kind of note a queued stream event is, so the desk labels it right.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StreamNoteKind {
    /// Server-side invalidation (btc_trend_flip etc.). Legacy meaning only:
    /// blocks entry, never closes an open position (see `RegimeVeto`).
    #[default]
    Veto,
    /// Dropped because a regenerated signal replaced it.
    Superseded,
    /// A frame that could not be parsed as a Signal.
    ParseError,
    /// The NEW "veto" event (2026-09-18 owner decision): Sentinel vetoed a
    /// published signal because BTC's regime turned against it. Blocks entry
    /// AND closes any open position immediately — see `crate::bot::engine::book`.
    RegimeVeto,
    /// Dropped because Sentinel already recorded its outcome (target or stop
    /// reached) — see `signal::resolved`.
    Resolved,
}

/// Whether Sentinel still lists a signal as open (no outcome yet), as far as
/// the last pending list read can tell. `Unknown` when that read is stale or
/// does not reach back to the signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenStatus {
    Open,
    Resolved,
    Unknown,
}

/// Sentinel's NEW "veto" SSE event (2026-09-18), mirroring the wire payload
/// 1:1 like `Signal` does. `reason_code` stays a plain string rather than a
/// strict enum — an unmapped code upstream must still parse, not vanish.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeVetoEvent {
    pub signal_id: String,
    pub symbol: String,
    #[serde(default)]
    pub direction: Option<Direction>,
    pub reason_code: String,
    #[serde(default)]
    pub reason_text: String,
    /// "regime_turn" | "extreme_move" — open-ended, display-only.
    #[serde(default)]
    pub trigger: String,
    #[serde(default)]
    pub btc_price: f64,
    #[serde(default)]
    pub btc_trend: String,
    #[serde(default)]
    pub btc_strength: f64,
    #[serde(default)]
    pub btc_change_1h_pct: f64,
    #[serde(default)]
    pub vetoed_at: String,
}

/// A server-side event surfaced to the desk feed: a veto, a regeneration
/// supersede, or a parse failure (PRD §5.4 — nothing is silent).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VetoEvent {
    pub signal_id: String,
    pub symbol: String,
    /// Sentinel's `invalidation_reason`, or the parse error text; may be empty.
    pub reason: String,
    #[serde(default)]
    pub kind: StreamNoteKind,
}

/// Response from `POST /api/v1/stream/ticket`.
#[derive(Debug, Clone, Deserialize)]
pub struct StreamTicketResponse {
    pub ticket: String,
}

/// Where the stream connection is, independent of any error text.
///
/// `last_error` used to double as a progress line ("connecting to stream…"),
/// so the UI could not tell a normal connect from an outage and raised a red
/// "stream down" alert on every start. The phase carries the progress; the
/// error carries only real failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamPhase {
    /// No connection requested (signed out / not started).
    Idle,
    /// First attempt of a session, nothing has failed yet.
    Connecting,
    /// Ticket issued and the stream is open.
    Live,
    /// The last attempt failed; the supervisor is retrying on its own.
    Retrying,
    /// Cannot recover without the user (e.g. no access token: sign in again).
    Down,
}

/// Our own health projection (camelCase — a type we design, not a wire mirror).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalHealthInfo {
    pub connected: bool,
    pub phase: StreamPhase,
    /// Seconds the stream has been out of `Live` (since the connect request or
    /// since it dropped); None while live or idle.
    pub not_live_secs: Option<u32>,
    pub last_signal_secs: Option<u32>,
    /// Round-trip latency of the last ticket request.
    pub latency_ms: Option<u32>,
    /// The last real failure (ticket/stream), or a stable code such as
    /// `noAccessToken`. Never a progress message; None while live.
    pub last_error: Option<String>,
}
