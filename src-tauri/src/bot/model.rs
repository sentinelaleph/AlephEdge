//! Bot types and configuration: the three signal-bot kinds (Futures, Spot,
//! Pump) and their per-bot settings.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::signal::model::{Direction, ManagementPlan};

/// Whether this binary contains the real-money order path: the cargo feature
/// `live`. The desktop app is ONE build and always has it (src-tauri/src/lib.rs
/// refuses to compile without it); every bot is still Paper unless the user
/// made it Live (`TradeMode`). The bot farm compiles this file too and has no
/// such feature: it is never live.
pub const LIVE_TRADING_ENABLED: bool = cfg!(feature = "live");

/// How a bot trades, chosen when the bot is made and stored with it. Paper is
/// the default and what every config saved before 0.3.19 reads as (except the
/// one-time mapping of a LIVE build's data, `BotManager::restore`).
///
/// Live is a stored property, not "real orders now": a Live signal bot also
/// needs its per-session arm (`BotConfig::live`, the typed LIVE with the money
/// at risk shown) before it may start. Paper and Live never share a bot: a
/// Live bot takes no paper trade and a Paper bot no real one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TradeMode {
    #[default]
    Paper,
    Live,
}

impl TradeMode {
    pub fn is_paper(&self) -> bool {
        *self == TradeMode::Paper
    }
}

/// Taker fees used for NET PnL simulation, per side, as a fraction of
/// notional: Binance USDⓈ-M VIP0 taker 0.05%, the same 0.10% round trip
/// Sentinel's ledger charges. Live trades are settled on the real commission
/// from userTrades; this is the simulation and the fallback.
pub const FUTURES_FEE_RATE: f64 = 0.0005;

/// Most positions one bot may hold (the risk level caps the total).
pub const MAX_BOT_POSITIONS: u8 = 20;
pub const SPOT_FEE_RATE: f64 = 0.001;
/// Smallest order Binance accepts on most pairs (MIN_NOTIONAL, USDT). Paper
/// entries below it are refused like live ones: a small account whose cap
/// sits under it (100 USDT at Cautious = 2 USDT) would otherwise see a paper
/// book of trades the exchange can never fill.
pub const EXCHANGE_MIN_ORDER_USDT: f64 = 5.0;

/// Capital a NEW bot starts with when the risk level allows it.
pub const DEFAULT_CAPITAL: f64 = 100.0;

/// A new bot's capital: the default, lowered to the per-position cap of the
/// current risk level and balance so the defaults can actually trade.
pub fn default_capital(max_capital_quote: f64) -> f64 {
    if !(max_capital_quote.is_finite() && max_capital_quote > 0.0) {
        return DEFAULT_CAPITAL;
    }
    let cap = if max_capital_quote >= 1.0 {
        max_capital_quote.floor()
    } else {
        (max_capital_quote * 100.0).floor() / 100.0
    };
    DEFAULT_CAPITAL.min(cap)
}

/// Widest stop (fraction of entry) a NEW desk bot's default risk % is fitted
/// for. Sentinel's stops are 2.5 x ATR: on the 19 pending signals of
/// 17 Oct 2026 the median was 7.1% and the widest 7.8%, so 10% leaves room.
pub const DESK_STOP_CEILING_FRAC: f64 = 0.10;

/// A new desk bot's risk % for `capital`: the 1% default, raised to the
/// smallest 0.1 step at which a stop of `DESK_STOP_CEILING_FRAC` (plus the
/// round-trip cost) still sizes at the exchange minimum, never above the 5%
/// ceiling. At 100 USDT capital this stays 1%; at the fresh-install 20 USDT
/// (Cautious, 1000 USDT balance) it is 2.6%: 0.52 USDT at risk per trade,
/// 0.05% of the balance. Below ~10 USDT even 5% cannot clear the minimum on a
/// wide stop; those signals are refused with `belowExchangeMinimum`, which the
/// bot's notes name.
pub fn default_risk_pct_for(capital: f64) -> f64 {
    use crate::bot::engine::sizing::{DEFAULT_RISK_PCT, MAX_RISK_PCT, ROUND_TRIP_COST_FRAC};
    if !(capital.is_finite() && capital > 0.0) {
        return DEFAULT_RISK_PCT;
    }
    let needed = EXCHANGE_MIN_ORDER_USDT * (DESK_STOP_CEILING_FRAC + ROUND_TRIP_COST_FRAC) / capital * 100.0;
    // Round UP to 0.1 (a tiny epsilon keeps 2.53 from becoming 2.6000000001 -> 2.7).
    let stepped = ((needed * 10.0) - 1e-9).ceil() / 10.0;
    stepped.clamp(DEFAULT_RISK_PCT, MAX_RISK_PCT)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BotKind {
    Futures,
    Spot,
    /// Trades USDT-M futures only on signals where Sentinel's pump engine
    /// contributes together with a structure break and an order block or FVG
    /// (`precheck_rules::is_pump_signal`). Open at every risk level, paper
    /// only and marked untested (no replay or forward test exists); the risk
    /// level does not unlock it, so choosing it loosens no other bot's limits.
    Pump,
    /// Trades the user's own TradingView alerts (tvlink), never Sentinel's
    /// signals: USDT-M futures, Paper or Live. Its alert limits are in
    /// `BotConfig::tv`. Real money only on a venue whose order path passed
    /// its test-network run (Binance), like the Futures bot.
    Tradingview,
}

impl BotKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BotKind::Futures => "futures",
            BotKind::Spot => "spot",
            BotKind::Pump => "pump",
            BotKind::Tradingview => "tradingview",
        }
    }

    /// Inverse of `as_str` (the stored `bot_kind` column).
    pub fn parse(s: &str) -> Option<Self> {
        [BotKind::Futures, BotKind::Spot, BotKind::Pump, BotKind::Tradingview]
            .into_iter()
            .find(|k| k.as_str() == s)
    }

    /// Pump, Futures and the TradingView bot trade the leveraged USDT-M
    /// futures market.
    pub fn uses_futures_market(self) -> bool {
        matches!(self, BotKind::Futures | BotKind::Pump | BotKind::Tradingview)
    }

    /// The kinds whose entries may be real orders: the Futures bot and the
    /// TradingView bot (both USDT-M futures). Spot and Pump are never live.
    pub fn live_capable(self) -> bool {
        matches!(self, BotKind::Futures | BotKind::Tradingview)
    }
}

/// Alert limits of the TradingView bot (`BotKind::Tradingview`), enforced
/// by the backend on every alert (tvlink), not only shown by the form. The
/// bot's own `max_positions` is its open-position cap, `leverage` its
/// leverage cap (clamped to the risk level), `symbols` its allow list, and
/// `capital` = `budget_usdt / max_positions` (margin per position), so the
/// money a Live arm confirms (`live_terms::signal_terms`) is the budget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TvSettings {
    /// The name the alert results show.
    pub name: String,
    /// Most margin the bot holds at once (USDT).
    pub budget_usdt: f64,
    /// Largest `size_usdt` (position notional) one alert may ask for; also
    /// the size of an alert without `size_usdt`.
    pub max_size_usdt: f64,
    /// Most alert entries per UTC day (a flip counts its open leg).
    pub max_opens_per_day: u32,
    /// An alert older than this on the server's clock is never executed.
    pub stale_s: u32,
    /// Refuse when the live price is more than this % from the alert's
    /// `price` (`{{close}}`).
    pub price_guard_pct: f64,
    /// Refuse an entry without `price` (the price guard cannot run).
    pub require_price: bool,
    /// Argon2id hash of the optional passphrase, with its salt and cost
    /// (bot/engine/tv_passphrase.rs); the passphrase itself is never stored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passphrase_hash: Option<String>,
    /// 0.3.19's format, read only: salted SHA-256 (hex) and its salt. The
    /// first alert that matches it replaces it with `passphrase_hash`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passphrase_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passphrase_salt: Option<String>,
    /// From the form only: Some(text) sets the passphrase, Some("") clears
    /// it, None keeps it. Never written to disk or sent back.
    #[serde(skip_serializing)]
    pub passphrase: Option<String>,
}

pub const TV_STALE_DEFAULT_S: u32 = 30;
pub const TV_STALE_MIN_S: u32 = 5;
pub const TV_STALE_MAX_S: u32 = 120;
pub const TV_PRICE_GUARD_DEFAULT_PCT: f64 = 0.5;
pub const TV_PRICE_GUARD_MAX_PCT: f64 = 10.0;
pub const TV_MAX_OPENS_PER_DAY: u32 = 200;

impl Default for TvSettings {
    fn default() -> Self {
        Self {
            name: "TradingView".to_string(),
            budget_usdt: 100.0,
            max_size_usdt: 50.0,
            max_opens_per_day: 10,
            stale_s: TV_STALE_DEFAULT_S,
            price_guard_pct: TV_PRICE_GUARD_DEFAULT_PCT,
            require_price: true,
            passphrase_hash: None,
            passphrase_sha256: None,
            passphrase_salt: None,
            passphrase: None,
        }
    }
}

impl TvSettings {
    /// Refuses values outside the declared ranges (no silent clamp).
    pub fn validate(&self) -> Result<(), String> {
        let pos = |v: f64| v.is_finite() && v > 0.0;
        if self.name.trim().is_empty() || self.name.chars().count() > 40 {
            return Err("tvBotNameInvalid".to_string());
        }
        if !pos(self.budget_usdt) {
            return Err("tvBudgetInvalid".to_string());
        }
        if !pos(self.max_size_usdt) || self.max_size_usdt < EXCHANGE_MIN_ORDER_USDT {
            return Err(format!("tvMaxSizeInvalid|{EXCHANGE_MIN_ORDER_USDT}"));
        }
        if !(1..=TV_MAX_OPENS_PER_DAY).contains(&self.max_opens_per_day) {
            return Err(format!("tvOpensPerDayRange|1–{TV_MAX_OPENS_PER_DAY}"));
        }
        if !(TV_STALE_MIN_S..=TV_STALE_MAX_S).contains(&self.stale_s) {
            return Err(format!("tvStaleRange|{TV_STALE_MIN_S}–{TV_STALE_MAX_S}"));
        }
        if !(self.price_guard_pct.is_finite() && self.price_guard_pct > 0.0 && self.price_guard_pct <= TV_PRICE_GUARD_MAX_PCT) {
            return Err(format!("tvPriceGuardRange|0–{TV_PRICE_GUARD_MAX_PCT}"));
        }
        Ok(())
    }
}

/// How a position is sized (see engine/sizing.rs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SizingMode {
    /// notional = capital × leverage. The serde default, so a saved config
    /// without the field never changes size silently.
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

/// Which take-profit a bot exits at. Sentinel
/// publishes three targets; `Custom` is a percent distance from the actual
/// fill in the trade's direction (40 = +40% long, −40% short). Resolved per
/// position at entry by `engine::take_profit::resolve_tp`.
///
/// Wire shape: `{"kind":"tp1"}` … `{"kind":"custom","pct":40.0}`. The serde
/// default is `Tp1`, so a config saved without the field keeps exiting at
/// TP1.
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

/// A new entry only on a signal at most this many minutes old (by default,
/// trade only signals at most 4 hours old: older entries earned less than
/// fresh ones). 0 = no limit.
pub const DEFAULT_MAX_SIGNAL_AGE_MIN: u32 = 240;

fn default_max_signal_age_min() -> u32 {
    DEFAULT_MAX_SIGNAL_AGE_MIN
}

fn default_risk_pct() -> f64 {
    crate::bot::engine::sizing::DEFAULT_RISK_PCT
}

/// Per-bot settings. `capital` is per-position, quote currency.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotConfig {
    pub kind: BotKind,
    /// Catalog id of the exchange the bot trades on (`exchange::providers`).
    pub exchange_id: String,
    /// Max positions THIS bot may hold at once (1–20; risk level caps the
    /// total across all bots).
    pub max_positions: u8,
    /// Capital per position, quote currency (e.g. 100 = 100 USDT margin).
    pub capital: f64,
    /// Futures only; clamped to the risk level's max. Spot is always 1.
    pub leverage: u8,
    /// Optional signal filters.
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
    /// Armed for real orders THIS session: set only by the typed LIVE
    /// (`bot_set_live`) on a bot whose `mode` is Live, never restored from disk
    /// (`BotManager::restore_configs` clears it). Real orders need this, the
    /// Live mode and the binary's order path (`LIVE_TRADING_ENABLED`).
    #[serde(default)]
    pub live: bool,
    /// Paper or Live, stored with the bot. Written only when Live, so a paper
    /// config is byte for byte what it was before modes existed.
    #[serde(default, skip_serializing_if = "TradeMode::is_paper")]
    pub mode: TradeMode,
    /// The user's own per-position loss tolerance in percent of the bot's
    /// CAPITAL, after leverage and fees (e.g. 2.0 = close once the unrealized
    /// NET loss reaches −2% of capital: a 0.57% price move at fixed 3x; in
    /// risk sizing a value at or above the risk % never fires, the stop is
    /// checked first), exit reason "max_loss".
    /// Independent of the BTC regime: a BTC-break trigger here would repeat
    /// the regime guard's close, which loses. None = feature off.
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
    /// The TradingView bot's alert limits (`BotKind::Tradingview` only).
    /// Left out of the JSON when absent, so every other config is written
    /// byte for byte as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tv: Option<TvSettings>,
}

impl BotConfig {
    /// A NEW bot's defaults: risk sizing at 1% per trade. (Saved configs
    /// without the field stay `fixed` — see `SizingMode`.)
    /// A NEW bot as the desk makes it (`bot_default_config`): the defaults,
    /// with the capital fitted under the risk level's per-position cap
    /// (`RiskState::max_capital_quote`) so the defaults can actually trade.
    pub fn new_for_desk(kind: BotKind, exchange_id: &str, max_capital_quote: f64) -> Self {
        let mut cfg = Self::new_default(kind, exchange_id);
        cfg.capital = default_capital(max_capital_quote);
        cfg
    }

    /// The new signal bot the desk OFFERS (`bot_default_config`, 0.3.17):
    /// `new_for_desk` plus a risk % large enough that a stop up to
    /// `DESK_STOP_CEILING_FRAC` from entry still sizes at or above the
    /// exchange minimum (`default_risk_pct_for`). `new_for_desk` itself is
    /// unchanged: the bot farm's pre-registered signal bots are built from it.
    pub fn new_tradable_for_desk(kind: BotKind, exchange_id: &str, max_capital_quote: f64) -> Self {
        let mut cfg = Self::new_for_desk(kind, exchange_id, max_capital_quote);
        cfg.risk_per_trade_pct = default_risk_pct_for(cfg.capital);
        cfg
    }

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
            mode: TradeMode::Paper,
            max_loss_pct: None,
            sizing: SizingMode::Risk,
            risk_per_trade_pct: default_risk_pct(),
            take_profit: TakeProfitTarget::Tp1,
            take_profit_overrides: BTreeMap::new(),
            max_signal_age_min: DEFAULT_MAX_SIGNAL_AGE_MIN,
            tv: (kind == BotKind::Tradingview).then(TvSettings::default),
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
        // `bot_configure`, not here.
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
        match (self.kind, &self.tv) {
            (BotKind::Tradingview, Some(tv)) => tv.validate()?,
            (BotKind::Tradingview, None) => return Err("tvSettingsMissing".to_string()),
            _ => {}
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
    pub direction: Direction,
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
    /// JSON file on disk, and on its own it must never reach the real-order
    /// paths while the master switch is off.
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
    /// Opened by the user on one signal (Execute, "Open position"), not by
    /// the bot's own loop: the bot's signal filters were skipped, its risk
    /// limits were not. Paper only. Left out of the JSON when false, so every
    /// other row (and the bot farm's) is written byte for byte as before.
    #[serde(default, skip_serializing_if = "is_false")]
    pub manual: bool,
}

fn is_false(v: &bool) -> bool {
    !*v
}

fn default_tp_target() -> String {
    "tp1".to_string()
}

impl OpenPosition {
    /// Whether this position may touch the real venue. BOTH the binary's
    /// order path and the per-position flag have to say yes (the bot farm has
    /// no order path). The flag is a line in a file any local process can
    /// edit: restore holds a real row of a Paper bot out of the book
    /// (`split_foreign_live_rows`), so only a Live bot's rows get here.
    pub fn is_live(&self) -> bool {
        LIVE_TRADING_ENABLED && self.live
    }
}

/// One visible "why the bot did NOT trade" note (no silent skips).
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
    /// The TradingView bot (alerts, tvlink), when one was made.
    #[serde(default)]
    pub tradingview: Option<BotConfig>,
    pub futures_running: bool,
    pub spot_running: bool,
    pub pump_running: bool,
    #[serde(default)]
    pub tradingview_running: bool,
    /// Real entries left in the TradingView bot's own pilot.
    #[serde(default)]
    pub tradingview_pilot_left: u32,
    pub open_positions: Vec<OpenPosition>,
    pub recent_skips: Vec<SkipNote>,
    /// Set when the daily loss kill-switch tripped today (UTC); bots cannot
    /// start again until the next UTC day (losses are not hidden, they are
    /// slept on).
    pub kill_switch_tripped: bool,
    /// Sentinel's BTC regime as the engine last read it: "normal" | "break" |
    /// "unknown". "break" means new LONG entries are refused (shorts still
    /// open); "unknown" refuses both until the macro feed answers.
    pub btc_regime: String,
    /// Real entries left in the pilot of the live Futures bot (0 = no pilot).
    #[serde(default)]
    pub pilot_left: u32,
    /// Venues real money can be switched on for now: those whose order path
    /// passed a test-network dry run (`exchange::DRY_RUN_PASSED`), or every
    /// order venue in the sandbox. Keys for the others still read the
    /// account and close positions.
    #[serde(default)]
    pub live_venues: Vec<String>,
    /// Bybit / OKX orders go to their demo / test networks
    /// (`ALEPH_EDGE_VENUE_SANDBOX=1`), not to a real account. The LIVE
    /// confirmations and the header say so.
    #[serde(default)]
    pub venue_sandbox: bool,
    /// The last market price the engine read for each open position, and
    /// what closing it there would book.
    #[serde(default)]
    pub marks: Vec<PositionMark>,
    /// What each open position would book if its current stop filled.
    #[serde(default)]
    pub at_stop: Vec<PositionAtStop>,
    /// Real-money position rows the last restore could not read ("kind
    /// signal_id"). Money in play the book does not manage: shown until a
    /// restart reads them, and counted as live exposure.
    #[serde(default)]
    pub live_unreadable: Vec<String>,
    /// Why no bot may start now (risk settings or the store unreadable), an
    /// i18n code, or None. Filled by `bot_status`.
    #[serde(default)]
    pub trading_blocked: Option<String>,
}

/// An open position at its current stop (the entry once breakeven has
/// armed): the trade record's formula at that level, so the sum over all
/// positions is what every stop hitting would book, fees included.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionAtStop {
    pub signal_id: String,
    pub bot_kind: BotKind,
    pub stop: f64,
    pub pnl_usdt: f64,
}

/// An open position at the last price the engine read for it (every tick
/// while positions are open). `pnl_pct` / `pnl_usdt` are what closing it at
/// that price would book: the trade record's own formula, fees and a banked
/// partial included, as a share of the position's capital.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionMark {
    pub signal_id: String,
    pub bot_kind: BotKind,
    pub price: f64,
    /// UNIX millis of the price read.
    pub at_ms: u64,
    pub pnl_pct: f64,
    pub pnl_usdt: f64,
}

/// Pilot: the first real entries after LIVE is switched
/// on are capped, so a mistake in setup costs little.
pub const PILOT_TRADES: u32 = 3;
/// Largest real entry while the pilot runs (raised to just above Binance's
/// minimum for the symbol when that is higher).
pub const PILOT_NOTIONAL_USDT: f64 = 50.0;

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
