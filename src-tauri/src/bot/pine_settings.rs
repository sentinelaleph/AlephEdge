//! The Pine bot's settings (`BotKind::Pine`, docs/aleph-edge-pine-lite-spec.md
//! sections 6 and 7): the script, its timeframe and symbols, the trigger
//! map, the input overrides and the bot's own limits. Mounted from model.rs
//! (`#[path]`), so the headless bot farm compiles it too: plain data and
//! pure checks, no Tauri, no `crate::pine`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Bar timeframe of the Pine bot (decision D3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum PineTimeframe {
    #[serde(rename = "15m")]
    M15,
    #[default]
    #[serde(rename = "1h")]
    H1,
    #[serde(rename = "4h")]
    H4,
    #[serde(rename = "1d")]
    D1,
}

impl PineTimeframe {
    /// Binance's kline interval name.
    pub fn as_str(self) -> &'static str {
        match self {
            PineTimeframe::M15 => "15m",
            PineTimeframe::H1 => "1h",
            PineTimeframe::H4 => "4h",
            PineTimeframe::D1 => "1d",
        }
    }

    pub fn interval_ms(self) -> u64 {
        match self {
            PineTimeframe::M15 => 15 * 60_000,
            PineTimeframe::H1 => 3_600_000,
            PineTimeframe::H4 => 4 * 3_600_000,
            PineTimeframe::D1 => 86_400_000,
        }
    }
}

/// What a fired trigger does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PineAction {
    Buy,
    Sell,
    Close,
    #[default]
    Ignore,
}

pub const PINE_MAX_SYMBOLS: usize = 10;
pub const PINE_MAX_SCRIPT_BYTES: usize = 500 * 1024;
pub const PINE_WARMUP_DEFAULT: u32 = 1_000;
pub const PINE_WARMUP_MIN: u32 = 300;
pub const PINE_WARMUP_MAX: u32 = 5_000;
pub const PINE_OPENS_DEFAULT: u32 = 20;
pub const PINE_OPENS_MAX: u32 = 200;
pub const PINE_STALE_DEFAULT_S: u32 = 60;
pub const PINE_STALE_MIN_S: u32 = 15;
pub const PINE_STALE_MAX_S: u32 = 300;
pub const PINE_PRICE_GUARD_DEFAULT_PCT: f64 = 0.5;
pub const PINE_PRICE_GUARD_MAX_PCT: f64 = 10.0;
/// Stop distance from the fill, percent.
pub const PINE_STOP_MIN_PCT: f64 = 0.1;
pub const PINE_STOP_MAX_PCT: f64 = 50.0;
/// Take-profit distance from the fill, percent.
pub const PINE_TARGET_MIN_PCT: f64 = 0.1;
pub const PINE_TARGET_MAX_PCT: f64 = 1000.0;
/// Most trigger ids a map may hold (the engine caps plots at 64).
pub const PINE_MAX_TRIGGERS: usize = 256;

/// The Pine bot's script and limits. The bot's own `max_positions` is its
/// open-position cap and `leverage` its leverage cap (clamped to the risk
/// level); `capital` = `budget_usdt / max_positions` (margin per position),
/// so the money a Live arm confirms is the budget, as for the TradingView
/// bot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PineSettings {
    /// The name the results show.
    pub name: String,
    /// The Pine-lite script text (never uploaded).
    pub script: String,
    /// sha256 hex of `script`, recomputed on every save.
    pub script_hash: String,
    pub timeframe: PineTimeframe,
    /// 1 to 10 USDT-M futures symbols, upper case.
    pub symbols: Vec<String>,
    /// Trigger id ("alert#0", "shape#1") -> what it does; absent = Ignore.
    pub triggers: BTreeMap<String, PineAction>,
    /// Input overrides keyed by the engine's input id (`pine::InputOverrides`).
    pub inputs: serde_json::Map<String, serde_json::Value>,
    /// Closed bars each run gets (300 to 5,000).
    pub warmup_bars: u32,
    /// Most margin the bot holds at once (USDT).
    pub budget_usdt: f64,
    /// Position notional of one entry (USDT).
    pub max_size_usdt: f64,
    /// Most entries per UTC day (a flip counts its open leg).
    pub max_opens_per_day: u32,
    /// Stop, percent from the fill. Required for Live (decision D4).
    pub stop_pct: Option<f64>,
    /// Take profit, percent from the fill.
    pub take_profit_pct: Option<f64>,
    /// Optional: the title of an `input.float` whose value is the stop
    /// percent. The bot's own stop stays the floor: the tighter one is used.
    pub stop_input: Option<String>,
    /// Optional: the title of an `input.float` whose value is the target
    /// percent (used instead of `take_profit_pct` when found).
    pub target_input: Option<String>,
    /// An action more than this long after its bar closed is dropped.
    pub stale_s: u32,
    /// Refuse when the live price is more than this % from the bar's close.
    pub price_guard_pct: f64,
}

impl Default for PineSettings {
    fn default() -> Self {
        Self {
            name: "Pine".to_string(),
            script: String::new(),
            script_hash: String::new(),
            timeframe: PineTimeframe::H1,
            symbols: Vec::new(),
            triggers: BTreeMap::new(),
            inputs: serde_json::Map::new(),
            warmup_bars: PINE_WARMUP_DEFAULT,
            budget_usdt: 100.0,
            max_size_usdt: 50.0,
            max_opens_per_day: PINE_OPENS_DEFAULT,
            stop_pct: Some(2.0),
            take_profit_pct: Some(4.0),
            stop_input: None,
            target_input: None,
            stale_s: PINE_STALE_DEFAULT_S,
            price_guard_pct: PINE_PRICE_GUARD_DEFAULT_PCT,
        }
    }
}

fn in_range(v: Option<f64>, lo: f64, hi: f64) -> bool {
    v.is_none_or(|v| v.is_finite() && (lo..=hi).contains(&v))
}

/// A USDT pair in plain upper-case letters and digits.
pub fn pine_symbol_ok(s: &str) -> bool {
    s.len() > 4 && s.ends_with("USDT") && s.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

impl PineSettings {
    /// What fired trigger `id` does (unmapped = Ignore).
    pub fn action_of(&self, id: &str) -> PineAction {
        self.triggers.get(id).copied().unwrap_or_default()
    }

    /// Refuses values outside the declared ranges (no silent clamp). Codes
    /// are i18n keys, `key|detail`.
    pub fn validate(&self) -> Result<(), String> {
        let pos = |v: f64| v.is_finite() && v > 0.0;
        if self.name.trim().is_empty() || self.name.chars().count() > 40 {
            return Err("pineBotNameInvalid".to_string());
        }
        if self.script.trim().is_empty() {
            return Err("pineScriptEmpty".to_string());
        }
        if self.script.len() > PINE_MAX_SCRIPT_BYTES {
            return Err(format!("pineScriptTooLarge|{}", PINE_MAX_SCRIPT_BYTES / 1024));
        }
        if self.symbols.is_empty() {
            return Err("pineSymbolsMin|1".to_string());
        }
        if self.symbols.len() > PINE_MAX_SYMBOLS {
            return Err(format!("pineSymbolsMax|{PINE_MAX_SYMBOLS}"));
        }
        if let Some(bad) = self.symbols.iter().find(|s| !pine_symbol_ok(s)) {
            return Err(format!("pineSymbolInvalid|{bad}"));
        }
        if self.triggers.len() > PINE_MAX_TRIGGERS || self.triggers.keys().any(|k| k.trim().is_empty() || k.len() > 64) {
            return Err("pineTriggerMapInvalid".to_string());
        }
        if !(PINE_WARMUP_MIN..=PINE_WARMUP_MAX).contains(&self.warmup_bars) {
            return Err(format!("pineWarmupRange|{PINE_WARMUP_MIN}–{PINE_WARMUP_MAX}"));
        }
        if !pos(self.budget_usdt) {
            return Err("pineBudgetInvalid".to_string());
        }
        if !pos(self.max_size_usdt) || self.max_size_usdt < super::EXCHANGE_MIN_ORDER_USDT {
            return Err(format!("pineMaxSizeInvalid|{}", super::EXCHANGE_MIN_ORDER_USDT));
        }
        if !(1..=PINE_OPENS_MAX).contains(&self.max_opens_per_day) {
            return Err(format!("pineOpensPerDayRange|1–{PINE_OPENS_MAX}"));
        }
        if !in_range(self.stop_pct, PINE_STOP_MIN_PCT, PINE_STOP_MAX_PCT) {
            return Err(format!("pineStopRange|{PINE_STOP_MIN_PCT}–{PINE_STOP_MAX_PCT}"));
        }
        if !in_range(self.take_profit_pct, PINE_TARGET_MIN_PCT, PINE_TARGET_MAX_PCT) {
            return Err(format!("pineTargetRange|{PINE_TARGET_MIN_PCT}–{PINE_TARGET_MAX_PCT}"));
        }
        let titled = |t: &Option<String>| t.as_deref().is_some_and(|t| !t.trim().is_empty());
        if self.stop_pct.is_none() && !titled(&self.stop_input) {
            return Err("pineStopRequired".to_string());
        }
        if self.take_profit_pct.is_none() && !titled(&self.target_input) {
            return Err("pineTargetRequired".to_string());
        }
        if !(PINE_STALE_MIN_S..=PINE_STALE_MAX_S).contains(&self.stale_s) {
            return Err(format!("pineStaleRange|{PINE_STALE_MIN_S}–{PINE_STALE_MAX_S}"));
        }
        if !(self.price_guard_pct.is_finite() && self.price_guard_pct > 0.0 && self.price_guard_pct <= PINE_PRICE_GUARD_MAX_PCT) {
            return Err(format!("pinePriceGuardRange|0–{PINE_PRICE_GUARD_MAX_PCT}"));
        }
        Ok(())
    }

    /// Why this bot may not trade real money (D4): a bot-level stop is
    /// required, whatever the script's inputs say.
    pub fn live_refusal(&self) -> Option<&'static str> {
        self.stop_pct.is_none().then_some("pineLiveStopRequired")
    }

    /// The parts a Live arm confirmed besides the money: the script, its
    /// triggers and inputs, the bars it runs on and the exits. Any change
    /// disarms a Live bot.
    pub fn same_script_terms(&self, o: &PineSettings) -> bool {
        self.script_hash == o.script_hash
            && self.triggers == o.triggers
            && self.inputs == o.inputs
            && self.timeframe == o.timeframe
            && self.symbols == o.symbols
            && self.stop_pct == o.stop_pct
            && self.take_profit_pct == o.take_profit_pct
            && self.stop_input == o.stop_input
            && self.target_input == o.target_input
    }
}
