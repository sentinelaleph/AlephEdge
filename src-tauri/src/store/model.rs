//! Trade-history types. PRD §5.6: every position row carries the FR and LD
//! values captured at open — the honesty contract is that skipped context is
//! never hidden. All PnL figures are NET (fees included).

use serde::{Deserialize, Serialize};

/// One closed trade, as persisted to SQLite and shown in the trade flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeRecord {
    /// SQLite rowid; 0 before insert.
    pub id: i64,
    pub signal_id: String,
    pub bot_kind: String,
    pub exchange_id: String,
    pub symbol: String,
    /// "long" | "short".
    pub direction: String,
    pub entry: f64,
    pub exit: f64,
    pub leverage: u8,
    /// Position capital (margin) in quote currency.
    pub capital: f64,
    /// NET PnL as % of capital (fees deducted).
    pub pnl_pct: f64,
    /// NET PnL in quote currency.
    pub pnl_quote: f64,
    /// Ledger exits "tp" | "sl" | "breakeven" | "horizon"; desk exits
    /// "max_loss" | "dailyStop" | "remoteKill" | "manual" (closed by the user
    /// from the desk); live "exchangeClosed" (flat on the exchange
    /// with neither protective order filled, e.g. a manual close); legacy rows may carry "invalidated" /
    /// "btcBreakCap".
    pub exit_reason: String,
    /// Funding %/interval at open, if the feed covered the symbol.
    pub fr_at_open: Option<f64>,
    /// Bid+ask ±1% depth (USD) at open, if available.
    pub ld_at_open: Option<f64>,
    /// UNIX millis.
    pub opened_at: u64,
    pub closed_at: u64,
    /// Parity with Sentinel's ledger: unlevered move % (partial-blended)
    /// − 0.10 round trip − 0.02 funding (long pays, short receives). None on
    /// rows recorded before the column existed.
    ///
    /// For LIVE trades it is computed from the real fills minus the real
    /// commission (still minus the ledger's 0.02 funding assumption, since
    /// funding payments are not fetched), so execution is measured against
    /// the ledger on the same scale.
    #[serde(default)]
    pub unlevered_net_pct: Option<f64>,
    /// True when the trade held real exchange exposure.
    #[serde(default)]
    pub live: bool,
    /// LIVE only: volume-weighted entry from the exchange's fills. None on
    /// paper trades and on live closes whose fills could not be fetched.
    #[serde(default)]
    pub fill_entry: Option<f64>,
    /// LIVE only: volume-weighted exit over every closing fill (partial legs
    /// included).
    #[serde(default)]
    pub fill_exit: Option<f64>,
    /// LIVE only: total commission paid in USDT, entry and exits. None when
    /// unknown (fills unavailable, or a fee paid in another asset).
    #[serde(default)]
    pub commission_usdt: Option<f64>,
    /// "fixed" | "risk" (legacy rows: fixed).
    #[serde(default = "fixed_mode")]
    pub sizing_mode: String,
    /// Declared risk % of capital at the stop (risk mode).
    #[serde(default)]
    pub risk_pct: Option<f64>,
    #[serde(default)]
    pub notional_usdt: Option<f64>,
    #[serde(default)]
    pub effective_leverage: Option<f64>,
    #[serde(default)]
    pub risk_capped: bool,
    /// NET PnL in USDT from the real size. Same value as `pnl_quote` (kept
    /// for compatibility); named for what it is.
    #[serde(default)]
    pub pnl_usdt: f64,
    /// NET PnL as % of the bot's capital. Same value as `pnl_pct`.
    #[serde(default)]
    pub pnl_pct_of_capital: f64,
    /// Sentinel's veto reason code, populated only when `exit_reason ==
    /// "veto"` (2026-09-18: BTC's regime turned against this signal). None
    /// for every other exit.
    #[serde(default)]
    pub veto_reason_code: Option<String>,
    /// Sentinel's human-readable veto reason, shown verbatim in the trade row
    /// tooltip. None for every other exit.
    #[serde(default)]
    pub veto_reason_text: Option<String>,
    /// Target the position exited against: "tp1" | "tp2" | "tp3" |
    /// "custom:40". None on rows recorded before targets were selectable
    /// (all of which were TP1).
    #[serde(default)]
    pub tp_target: Option<String>,
    /// The configured target when the signal lacked it and a lower one was
    /// used ("tp3"). None when no fallback happened.
    #[serde(default)]
    pub tp_fallback_from: Option<String>,
}

fn fixed_mode() -> String {
    "fixed".to_string()
}

/// Aggregate performance metrics (PRD §5.6), computed over closed trades.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PnlStats {
    pub total_trades: u32,
    pub wins: u32,
    pub losses: u32,
    /// Qualified winrate: wins / (wins + losses), TP/SL resolutions only —
    /// invalidated/expired exits count as trades but not toward winrate.
    pub win_rate: Option<f64>,
    pub net_pnl_quote: f64,
    pub avg_pnl_pct: f64,
    /// Gross profit / gross loss (absolute); None until both sides exist.
    pub profit_factor: Option<f64>,
    /// Worst peak-to-trough drop of the cumulative PnL curve, in quote.
    pub max_drawdown_quote: f64,
    /// Today's (UTC) realized net PnL in quote — feeds the daily kill-switch.
    pub today_pnl_quote: f64,
}
