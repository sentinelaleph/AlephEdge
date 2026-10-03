//! Bot types and configuration (PRD §5.1–5.2). Two kinds ship in F3
//! (Futures, Spot); Pump arrives in F5 with its radar feed.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::signal::model::ManagementPlan;

/// PRD §8: live order sending stays behind a feature flag. It is the cargo
/// feature `live` (off by default, so the public build is simulation-only);
/// a live build is a deliberate `--features live`. While false, fills are
/// simulated at the live market price — the full pipeline (prechecks, risk
/// limits, management plan, persistence) is real. The paper runner compiles
/// this file too and has no such feature: it is never live.
pub const LIVE_TRADING_ENABLED: bool = cfg!(feature = "live");

/// Taker fees used for NET PnL simulation, per side, as a fraction of
/// notional: Binance USDⓈ-M VIP0 taker 0.05%, the same 0.10% round trip
/// Sentinel's ledger charges. Live trades are settled on the real commission
/// from userTrades; this is the simulation and the fallback.
pub const FUTURES_FEE_RATE: f64 = 0.0005;

/// Most positions one bot may hold (the risk level caps the total).
pub const MAX_BOT_POSITIONS: u8 = 20;
pub const SPOT_FEE_RATE: f64 = 0.001;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BotKind {
    Futures,
    Spot,
    /// Most aggressive type (PRD §5.1): trades USDT-M futures but only on
    /// signals where Sentinel's pump/dump engine is a strong contributor;
    /// gated to the Ambitious+ risk levels.
    Pump,
}

impl BotKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BotKind::Futures => "futures",
            BotKind::Spot => "spot",
            BotKind::Pump => "pump",
        }
    }

    /// Inverse of `as_str` (the stored `bot_kind` column).
    pub fn parse(s: &str) -> Option<Self> {
        [BotKind::Futures, BotKind::Spot, BotKind::Pump]
            .into_iter()
            .find(|k| k.as_str() == s)
    }

    /// Pump and Futures both trade the leveraged USDT-M futures market.
    pub fn uses_futures_market(self) -> bool {
        matches!(self, BotKind::Futures | BotKind::Pump)
    }
}

/// How a position is sized (see engine/sizing.rs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SizingMode {
    /// notional = capital × leverage — the pre-2026-09-17 behaviour. The
    /// serde default, so a saved config without the field never changes size
    /// silently.
    #[default]
    Fixed,
    /// notional sized so a stop loses `risk_per_trade_pct` of capital;
    /// leverage is the ceiling.
    Risk,
}

impl SizingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            SizingMode::Fixed => "fixed",
            SizingMode::Risk => "risk",
        }
    }
}

/// Which take-profit a bot exits at (owner request 2026-09-22). Sentinel
/// publishes three targets; `Custom` is a percent distance from the actual
/// fill in the trade's direction (40 = +40% long, −40% short). Resolved per
/// position at entry by `engine::take_profit::resolve_tp`.
///
/// Wire shape: `{"kind":"tp1"}` … `{"kind":"custom","pct":40.0}`. The serde
/// default is `Tp1`, so every config saved before this existed keeps exiting
/// at TP1 exactly as before.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TakeProfitTarget {
    #[default]
    Tp1,
    Tp2,
    Tp3,
    Custom {
        pct: f64,
    },
}

/// Bounds for `TakeProfitTarget::Custom::pct`, in percent.
pub const MIN_CUSTOM_TP_PCT: f64 = 0.1;
pub const MAX_CUSTOM_TP_PCT: f64 = 1000.0;

impl TakeProfitTarget {
    /// Whether a custom percent is usable at all (finite, within bounds).
    /// The short-side "price stays above 0" rule is checked at resolution,
    /// where the direction is known.
    pub fn pct_in_bounds(self) -> bool {
        match self {
            TakeProfitTarget::Custom { pct } => {
                pct.is_finite() && (MIN_CUSTOM_TP_PCT..=MAX_CUSTOM_TP_PCT).contains(&pct)
            }
            _ => true,
        }
    }
}

/// A new entry only on a signal at most this many minutes old (owner rule,
/// 2026-09-23: "en fazla 4 saatlik sinyallere işlem yapabilir"). Measured on
/// the 13–24 Sep book: 15 of 23 entries came from signals older than 4h and
/// earned −0.04 R each, against +0.30 R for the fresh ones. 0 = no limit.
pub const DEFAULT_MAX_SIGNAL_AGE_MIN: u32 = 240;

fn default_max_signal_age_min() -> u32 {
    DEFAULT_MAX_SIGNAL_AGE_MIN
}

fn default_risk_pct() -> f64 {
    crate::bot::engine::sizing::DEFAULT_RISK_PCT
}

/// Per-bot settings (PRD §5.2). `capital` is per-position, quote currency.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotConfig {
    pub kind: BotKind,
    /// Exchange the bot trades on. F3: "binance" only (F4 widens).
    pub exchange_id: String,
    /// Max positions THIS bot may hold at once (1–20; risk level caps the
    /// total across all bots).
    pub max_positions: u8,
    /// Capital per position, quote currency (e.g. 100 = 100 USDT margin).
    pub capital: f64,
    /// Futures only; clamped to the risk level's max. Spot is always 1.
    pub leverage: u8,
    /// Optional signal filters (PRD §5.2-5).
    #[serde(default)]
    pub min_confidence: Option<f64>,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub symbols: Vec<String>,
    /// Trade ONLY signals whose combo is in this list (empty = no combo
    /// filter). Sentinel's published per-combo records differ by tens of
    /// percentage points, so this is the desk's main evidence lever.
    #[serde(default)]
    pub combos: Vec<String>,
    /// Trade ONLY signals whose PRIMARY confluence source is in this list
    /// (empty = no engine filter). Primary = the first contributing item,
    /// which is exactly what Sentinel's per-engine ledger is keyed on.
    #[serde(default)]
    pub engines: Vec<String>,
    /// Per-bot LIVE opt-in. Real orders require BOTH this and the compile-time
    /// LIVE_TRADING_ENABLED master switch — two deliberate flips, either alone
    /// does nothing. Defaults false; absent in every stored config today.
    #[serde(default)]
    pub live: bool,
    /// The user's own per-position loss tolerance in percent (e.g. 2.0 =
    /// close once unrealized NET loss reaches −2%), exit reason "max_loss".
    /// Independent of the BTC regime: tying it to a BTC-break trigger
    /// recreated the guard close measured at −163 pts. None = feature off.
    #[serde(default)]
    pub max_loss_pct: Option<f64>,
    /// Absent in configs saved before sizing existed ⇒ `fixed`.
    #[serde(default)]
    pub sizing: SizingMode,
    /// Risk mode: % of `capital` lost if the stop is hit (0.1–5.0).
    #[serde(default = "default_risk_pct")]
    pub risk_per_trade_pct: f64,
    /// Bot-wide take-profit target. Absent in older configs ⇒ TP1.
    #[serde(default)]
    pub take_profit: TakeProfitTarget,
    /// Per-symbol overrides of `take_profit`, keyed by symbol (uppercase,
    /// e.g. "SOLUSDT"). An override wins over the bot-wide target.
    #[serde(default)]
    pub take_profit_overrides: BTreeMap<String, TakeProfitTarget>,
    /// Fresh-signal mode: enter only signals at most this many minutes old.
    /// The desktop always walks signals newest first. 0 = no age limit.
    #[serde(default = "default_max_signal_age_min")]
    pub max_signal_age_min: u32,
}

impl BotConfig {
    /// A NEW bot's defaults: risk sizing at 1% per trade. (Saved configs
    /// without the field stay `fixed` — see `SizingMode`.)
    pub fn new_default(kind: BotKind, exchange_id: &str) -> Self {
        Self {
            kind,
            exchange_id: exchange_id.to_string(),
            max_positions: 3,
            capital: 100.0,
            leverage: if kind.uses_futures_market() { 3 } else { 1 },
            min_confidence: None,
            direction: None,
            symbols: Vec::new(),
            combos: Vec::new(),
            engines: Vec::new(),
            live: false,
            max_loss_pct: None,
            sizing: SizingMode::Risk,
            risk_per_trade_pct: default_risk_pct(),
            take_profit: TakeProfitTarget::Tp1,
            take_profit_overrides: BTreeMap::new(),
            max_signal_age_min: DEFAULT_MAX_SIGNAL_AGE_MIN,
        }
    }

    /// The target for `symbol`: its override if one exists (symbol match is
    /// case-insensitive), the bot-wide target otherwise.
    pub fn take_profit_for(&self, symbol: &str) -> TakeProfitTarget {
        self.take_profit_overrides
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(symbol))
            .map_or(self.take_profit, |(_, t)| *t)
    }

    /// Refuses a risk percentage outside the declared bounds instead of
    /// silently clamping what the user typed.
    pub fn validate(&self) -> Result<(), String> {
        use crate::bot::engine::sizing::{MAX_RISK_PCT, MIN_RISK_PCT};
        // The desktop's upper bound (MAX_BOT_POSITIONS) is checked in
        // `bot_configure`; the paper runner's mirror arms run 60 by design.
        if self.max_positions == 0 {
            return Err("botMaxPositionsMin".to_string());
        }
        if !(self.capital.is_finite() && self.capital > 0.0) {
            return Err("botCapitalInvalid".to_string());
        }
        if self.leverage == 0 {
            return Err("botLeverageMin".to_string());
        }
        // Confidence is a 0–1 fraction; 1.5 (someone typed 150) would skip
        // every signal without a word.
        if let Some(c) = self.min_confidence {
            if !(0.0..=1.0).contains(&c) {
                return Err("botConfidenceRange".to_string());
            }
        }
        if let Some(m) = self.max_loss_pct {
            if !(m.is_finite() && m > 0.0) {
                return Err("botMaxLossInvalid".to_string());
            }
        }
        if let Some(d) = self.direction.as_deref() {
            if !["all", "long", "short"].iter().any(|x| d.eq_ignore_ascii_case(x)) {
                return Err("botDirectionInvalid".to_string());
            }
        }
        let r = self.risk_per_trade_pct;
        if self.sizing == SizingMode::Risk && !(MIN_RISK_PCT..=MAX_RISK_PCT).contains(&r) {
            return Err(format!("botRiskRange|{MIN_RISK_PCT}%–{MAX_RISK_PCT}%"));
        }
        let tp_bounds = || format!("{MIN_CUSTOM_TP_PCT}%–{MAX_CUSTOM_TP_PCT}%");
        if !self.take_profit.pct_in_bounds() {
            return Err(format!("botTakeProfitRange|{}", tp_bounds()));
        }
        for (symbol, target) in &self.take_profit_overrides {
            if symbol.trim().is_empty() {
                return Err("botTakeProfitSymbol".to_string());
            }
            if !target.pct_in_bounds() {
                return Err(format!("botTakeProfitRange|{symbol}: {}", tp_bounds()));
            }
        }
        Ok(())
    }
}

/// An open position. Simulated by default; `live` marks a position that holds
/// REAL exchange exposure and must be flattened with a real order, never by
/// bookkeeping alone.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPosition {
    pub signal_id: String,
    pub bot_kind: BotKind,
    /// Exchange the position lives on — decides which ticker to poll on close.
    pub exchange_id: String,
    pub symbol: String,
    /// Signal timeframe; one position per symbol+timeframe per bot.
    #[serde(default)]
    pub timeframe: Option<String>,
    /// "long" | "short".
    pub direction: String,
    /// Fill price (what the book actually paid).
    pub entry: f64,
    /// The RESOLVED take-profit (see `tp_target`): TP1 of the signal on
    /// every row opened before targets were selectable, and by default.
    pub tp: f64,
    /// The signal's original stop (the breakeven move never rewrites it).
    pub sl: f64,
    /// The SIGNAL's entry: plan levels are measured from it, as Sentinel's
    /// evaluator does, not from our fill.
    #[serde(default)]
    pub signal_entry: f64,
    /// R = |signal entry − signal sl|.
    #[serde(default)]
    pub risk_r: f64,
    #[serde(default)]
    pub plan: Option<ManagementPlan>,
    /// Stop sits at the fill entry. Armed in one evaluation, effective from
    /// the next — never the same one (Sentinel's evaluator semantics).
    #[serde(default)]
    pub breakeven_armed: bool,
    /// Fraction banked by the partial leg (0 = none yet).
    #[serde(default)]
    pub partial_fraction: f64,
    #[serde(default)]
    pub partial_price: Option<f64>,
    /// UNIX millis: expires_at + 72h. Untouched positions close here at the
    /// live price (reason "horizon"). 0 = no horizon (legacy row).
    #[serde(default)]
    pub horizon_ms: u64,
    pub leverage: u8,
    pub capital: f64,
    pub fr_at_open: Option<f64>,
    pub ld_at_open: Option<f64>,
    pub opened_at: u64,
    /// True when this position is backed by a real exchange order. Serde
    /// defaults keep previously-persisted (all simulated) positions valid.
    /// Read it through `is_live()`, never directly: this field arrives from a
    /// JSON file on disk, and on its own it used to be enough to reach the
    /// real-order paths while the master switch was off (2026-09-20 review).
    #[serde(default)]
    pub live: bool,
    /// Executed quantity from the real fill (0 for simulated positions).
    #[serde(default)]
    pub qty: f64,
    /// LIVE only: the opening order's id — anchors the real-fill window.
    #[serde(default)]
    pub entry_order_id: Option<i64>,
    /// LIVE only: the exchange-side stop (algo order) currently protecting
    /// the position. Persisted so a restart can still replace or query it.
    #[serde(default)]
    pub stop_algo_id: Option<i64>,
    /// LIVE only: the exchange-side take-profit at `tp`. None when placing it
    /// failed — the app-side TP exit still works while the app runs.
    #[serde(default)]
    pub tp_algo_id: Option<i64>,
    /// LIVE only: the exchange stop has been moved to the fill entry. A fired
    /// stop then reconciles as "breakeven", not "sl".
    #[serde(default)]
    pub stop_at_breakeven: bool,
    /// LIVE only: the entry filled but no exchange stop could be placed and
    /// the emergency flatten did not confirm. The tick loop flattens it at
    /// once, retried every tick, instead of holding a bare position.
    #[serde(default)]
    pub unprotected: bool,
    /// LIVE only: quantity the partial leg actually reduced on the exchange.
    #[serde(default)]
    pub partial_qty: f64,
    /// Sizing used at open (legacy rows: fixed).
    #[serde(default)]
    pub sizing_mode: SizingMode,
    /// Declared risk % of capital at the stop (risk mode).
    #[serde(default)]
    pub risk_pct: Option<f64>,
    /// Position notional in USDT (0 on legacy rows).
    #[serde(default)]
    pub notional_usdt: f64,
    /// notional / capital; PnL uses it (0 on legacy rows ⇒ `leverage`).
    #[serde(default)]
    pub effective_leverage: f64,
    /// The leverage cap bound the size: real risk is below the declared one.
    #[serde(default)]
    pub risk_capped: bool,
    /// Which target `tp` is: "tp1" | "tp2" | "tp3" | "custom:40". Legacy
    /// rows ⇒ "tp1", which is what they always exited at.
    #[serde(default = "default_tp_target")]
    pub tp_target: String,
    /// Set when the configured target was missing on the signal and the
    /// highest one it had was used instead: the REQUESTED target ("tp3").
    #[serde(default)]
    pub tp_fallback_from: Option<String>,
}

fn default_tp_target() -> String {
    "tp1".to_string()
}

impl OpenPosition {
    /// Whether this position may touch the real venue. BOTH the compile-time
    /// master switch and the per-position flag have to say yes: the flag alone
    /// is a line in a file any local process can edit, and every close,
    /// manage and reconcile path used to trust it by itself.
    pub fn is_live(&self) -> bool {
        LIVE_TRADING_ENABLED && self.live
    }
}

/// One visible "why the bot did NOT trade" note (PRD §5.4: no silent skips).
/// `reason` is an i18n key suffix; `detail` is a preformatted value like
/// "-0.12%" that the UI interpolates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkipNote {
    pub at_ms: u64,
    pub symbol: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Full bot-desk snapshot for the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotDeskStatus {
    pub live_trading_enabled: bool,
    /// False when orders go to a non-production Binance (the testnet), so
    /// the UI never compares hosts itself.
    #[serde(default)]
    pub binance_is_production: bool,
    pub futures: Option<BotConfig>,
    pub spot: Option<BotConfig>,
    pub pump: Option<BotConfig>,
    pub futures_running: bool,
    pub spot_running: bool,
    pub pump_running: bool,
    pub open_positions: Vec<OpenPosition>,
    pub recent_skips: Vec<SkipNote>,
    /// Set when the daily loss kill-switch tripped today (UTC); bots cannot
    /// start again until the next UTC day (PRD §5.3 — losses are not hidden,
    /// they are slept on).
    pub kill_switch_tripped: bool,
    /// Sentinel's BTC regime as the engine last read it: "normal" | "break" |
    /// "unknown". "break" means new LONG entries are refused (shorts still
    /// open); "unknown" refuses both until the macro feed answers.
    pub btc_regime: String,
}

#[cfg(test)]
mod validate_tests {
    use super::*;

    fn cfg() -> BotConfig {
        BotConfig::new_default(BotKind::Futures, "binance")
    }

    #[test]
    fn the_default_config_is_valid() {
        assert!(cfg().validate().is_ok());
    }

    #[test]
    fn nonsense_numbers_are_refused_not_silently_used() {
        let bad: [fn(&mut BotConfig); 5] = [
            |c| c.max_positions = 0,
            |c| c.capital = -5.0,
            |c| c.min_confidence = Some(1.5),
            |c| c.max_loss_pct = Some(0.0),
            |c| c.direction = Some("sideways".into()),
        ];
        for (i, f) in bad.iter().enumerate() {
            let mut c = cfg();
            f(&mut c);
            assert!(c.validate().is_err(), "case {i}");
        }
        let mut ok = cfg();
        ok.direction = Some("ALL".into());
        assert!(ok.validate().is_ok(), "direction is case-insensitive");
    }
}
