//! Risk levels and their hard limits, as one table in code.
//!
//! The level is a GLOBAL setting: its limits bound every bot. Each level's
//! word wears its own color in the UI (cautious ice-blue → greedy red); the
//! engine only cares about the numbers here.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Cautious,
    Calm,
    Balanced,
    Ambitious,
    Greedy,
}

impl RiskLevel {
    // No level unlocks a bot kind: this level is GLOBAL, so tying one
    // untested paper-only bot (Pump) to it would raise every other bot's
    // leverage, position and loss limits along with it, the real-money Futures
    // bot and the DCA / Grid budget cap included. Pump runs at any level,
    // under that level's limits, on paper only, labelled untested.

    /// The limits row for this level.
    pub fn limits(self) -> RiskLimits {
        match self {
            // level          lev  pos  cap%   daily%  FR%/interval  LD share%
            RiskLevel::Cautious => RiskLimits::new(self, 2, 3, 2.0, 2.0, 0.05, 5.0),
            RiskLevel::Calm => RiskLimits::new(self, 3, 5, 4.0, 4.0, 0.05, 5.0),
            RiskLevel::Balanced => RiskLimits::new(self, 5, 8, 6.0, 6.0, 0.10, 10.0),
            RiskLevel::Ambitious => RiskLimits::new(self, 10, 12, 10.0, 10.0, 0.10, 10.0),
            RiskLevel::Greedy => RiskLimits::new(self, 20, 20, 15.0, 15.0, 0.15, 20.0),
        }
    }
}

/// Hard limits derived from the risk level. `fr_threshold_pct` and
/// `max_depth_share_pct` turn the "strict / normal / loose" FR-LD strictness
/// of each level into explicit numbers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskLimits {
    pub level: RiskLevel,
    pub max_leverage: u8,
    /// Max concurrent positions across ALL bots.
    pub max_concurrent_positions: u8,
    /// Per-position capital ceiling as % of the working balance.
    pub max_capital_pct: f64,
    /// Daily realized-loss kill-switch, % of working balance (positive number).
    pub daily_loss_limit_pct: f64,
    /// Skip the trade when funding runs against the position beyond this
    /// %/interval (absolute value).
    pub fr_threshold_pct: f64,
    /// Position notional may not exceed this % of the ±1% book depth.
    pub max_depth_share_pct: f64,
}

impl RiskLimits {
    const fn new(
        level: RiskLevel,
        max_leverage: u8,
        max_concurrent_positions: u8,
        max_capital_pct: f64,
        daily_loss_limit_pct: f64,
        fr_threshold_pct: f64,
        max_depth_share_pct: f64,
    ) -> Self {
        Self {
            level,
            max_leverage,
            max_concurrent_positions,
            max_capital_pct,
            daily_loss_limit_pct,
            fr_threshold_pct,
            max_depth_share_pct,
        }
    }
}

/// Persisted app config (device-local, non-sensitive — NOT the vault).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskConfig {
    pub level: RiskLevel,
    /// Simulated account equity, quote currency (e.g. USDT). The percentage
    /// caps in `RiskLimits` (per-position capital, daily loss) are taken
    /// against THIS — there is no live exchange balance in simulation, so the
    /// user declares the capital they're modelling.
    #[serde(default = "default_balance")]
    pub balance: f64,
    /// Close open positions when the daily-loss stop trips (on by default).
    #[serde(default = "default_close_on_stop")]
    pub close_on_stop: bool,
    /// The user's own daily-loss tolerance, % of balance. `None` = use the
    /// level's table value.
    ///
    /// It can only ever make the stop TIGHTER — see `effective_daily_loss_pct`.
    /// A risk control the user can loosen is not a risk control, and the level
    /// table is the ceiling the product stands behind.
    #[serde(default)]
    pub daily_loss_override_pct: Option<f64>,
    /// Portfolio risk view alert thresholds (Risk page, `riskview`).
    #[serde(default)]
    pub view_thresholds: ViewThresholds,
}

/// When the portfolio risk view (src/riskview) raises its warnings, % each.
/// Readouts, not limits: crossing one alerts, it never blocks or closes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ViewThresholds {
    /// Share of risk capital on the dominant side (long or short).
    pub same_direction_pct: f64,
    /// Share of risk capital in one coin.
    pub single_coin_pct: f64,
    /// Loss at a BTC -20% shock, % of the capital in bots.
    pub stress20_pct: f64,
}

impl Default for ViewThresholds {
    fn default() -> Self {
        Self { same_direction_pct: 80.0, single_coin_pct: 40.0, stress20_pct: 25.0 }
    }
}

impl ViewThresholds {
    /// Each value inside its accepted range (same direction 50..=100, one
    /// coin 10..=100, stress 1..=100), or the i18n code of the first that is
    /// not. A share below 50% on the dominant side is impossible, so such a
    /// threshold would alert on every book.
    pub fn check(&self) -> Result<(), &'static str> {
        let within = |v: f64, lo: f64| v.is_finite() && (lo..=100.0).contains(&v);
        if !within(self.same_direction_pct, 50.0) {
            return Err("viewThresholdSameDirection");
        }
        if !within(self.single_coin_pct, 10.0) {
            return Err("viewThresholdSingleCoin");
        }
        if !within(self.stress20_pct, 1.0) {
            return Err("viewThresholdStress");
        }
        Ok(())
    }
}

fn default_balance() -> f64 {
    1_000.0
}

fn default_close_on_stop() -> bool {
    true
}

impl Default for RiskConfig {
    fn default() -> Self {
        // The product defaults to its own philosophy: protection first.
        Self {
            level: RiskLevel::Cautious,
            balance: default_balance(),
            close_on_stop: default_close_on_stop(),
            daily_loss_override_pct: None,
            view_thresholds: ViewThresholds::default(),
        }
    }
}

impl RiskConfig {
    /// The daily-loss stop actually in force, % of balance.
    ///
    /// The user's own tolerance applies only when it is **stricter** than the
    /// level's table value. Entering 20% on Cautious does not buy a 20% stop —
    /// it leaves 2% standing. The UI says so; this function is what makes it
    /// true.
    ///
    /// A non-finite or non-positive override is ignored rather than trusted: a
    /// hand-edited `config.json` carrying `0` would otherwise mean "stop after
    /// losing nothing", which halts the desk on its first tick.
    pub fn effective_daily_loss_pct(&self) -> f64 {
        let table = self.level.limits().daily_loss_limit_pct;
        match self.daily_loss_override_pct {
            Some(v) if v.is_finite() && v > 0.0 => v.min(table),
            _ => table,
        }
    }
}

/// Full risk state for the UI: the level's hard limits plus the balance and
/// the resolved per-position capital ceiling (so the UI shows real numbers,
/// not just a percentage).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskState {
    pub limits: RiskLimits,
    pub balance: f64,
    pub close_on_stop: bool,
    /// `balance * max_capital_pct / 100` — max capital per position, quote.
    pub max_capital_quote: f64,
    /// Raw value the user typed, echoed back so the field is not silently
    /// rewritten under them when it exceeds the level cap.
    pub daily_loss_override_pct: Option<f64>,
    /// The stop actually in force = min(level cap, override). This — not
    /// `limits.daily_loss_limit_pct` — is what the UI must display as "your
    /// stop", and what the engine trips on.
    pub effective_daily_loss_pct: f64,
    /// True when the typed tolerance is looser than the level allows, so the
    /// UI can say the cap won rather than appearing to accept a number it did
    /// not honour.
    pub daily_loss_override_capped: bool,
    /// Portfolio risk view thresholds (warn/danger chips and alerts).
    pub view_thresholds: ViewThresholds,
}

impl RiskState {
    pub fn from_config(config: &RiskConfig) -> Self {
        let limits = config.level.limits();
        let effective = config.effective_daily_loss_pct();
        Self {
            max_capital_quote: config.balance * limits.max_capital_pct / 100.0,
            daily_loss_override_pct: config.daily_loss_override_pct,
            effective_daily_loss_pct: effective,
            daily_loss_override_capped: config
                .daily_loss_override_pct
                .is_some_and(|v| v.is_finite() && v > 0.0 && v > limits.daily_loss_limit_pct),
            limits,
            balance: config.balance,
            close_on_stop: config.close_on_stop,
            view_thresholds: config.view_thresholds,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(level: RiskLevel, override_pct: Option<f64>) -> RiskConfig {
        RiskConfig {
            level,
            balance: 1_000.0,
            close_on_stop: true,
            daily_loss_override_pct: override_pct,
            view_thresholds: ViewThresholds::default(),
        }
    }

    #[test]
    fn no_override_uses_the_level_table() {
        // Cautious is 2.0%: the tightest level is the spot-trader case.
        assert_eq!(
            cfg(RiskLevel::Cautious, None).effective_daily_loss_pct(),
            2.0
        );
        assert_eq!(
            cfg(RiskLevel::Greedy, None).effective_daily_loss_pct(),
            15.0
        );
    }

    #[test]
    fn a_stricter_tolerance_is_honoured() {
        assert_eq!(
            cfg(RiskLevel::Greedy, Some(2.0)).effective_daily_loss_pct(),
            2.0
        );
        assert_eq!(
            cfg(RiskLevel::Balanced, Some(1.5)).effective_daily_loss_pct(),
            1.5
        );
    }

    #[test]
    fn a_looser_tolerance_can_never_widen_the_stop() {
        // The whole point. If typing a bigger number moved the stop out, the
        // level table would be a suggestion rather than a limit.
        assert_eq!(
            cfg(RiskLevel::Cautious, Some(20.0)).effective_daily_loss_pct(),
            2.0
        );
        assert!(
            RiskState::from_config(&cfg(RiskLevel::Cautious, Some(20.0)))
                .daily_loss_override_capped
        );
    }

    #[test]
    fn nonsense_values_fall_back_to_the_table_rather_than_halting_the_desk() {
        // 0 would mean "stop after losing nothing": the desk would trip on its
        // first tick and look broken rather than protected.
        assert_eq!(
            cfg(RiskLevel::Calm, Some(0.0)).effective_daily_loss_pct(),
            4.0
        );
        assert_eq!(
            cfg(RiskLevel::Calm, Some(-3.0)).effective_daily_loss_pct(),
            4.0
        );
        assert_eq!(
            cfg(RiskLevel::Calm, Some(f64::NAN)).effective_daily_loss_pct(),
            4.0
        );
    }

    #[test]
    fn view_thresholds_default_and_bounds() {
        let d = ViewThresholds::default();
        assert_eq!((d.same_direction_pct, d.single_coin_pct, d.stress20_pct), (80.0, 40.0, 25.0));
        assert!(d.check().is_ok());
        assert_eq!(ViewThresholds { same_direction_pct: 40.0, ..d }.check(), Err("viewThresholdSameDirection"));
        assert_eq!(ViewThresholds { single_coin_pct: 101.0, ..d }.check(), Err("viewThresholdSingleCoin"));
        assert_eq!(ViewThresholds { stress20_pct: f64::NAN, ..d }.check(), Err("viewThresholdStress"));
        // A config saved before the field existed loads the defaults.
        let old: RiskConfig = serde_json::from_str(r#"{"level":"calm","balance":500}"#).unwrap();
        assert_eq!(old.view_thresholds, d);
        let partial: RiskConfig =
            serde_json::from_str(r#"{"level":"calm","viewThresholds":{"stress20Pct":10}}"#).unwrap();
        assert_eq!(partial.view_thresholds.stress20_pct, 10.0);
        assert_eq!(partial.view_thresholds.same_direction_pct, 80.0);
    }

    #[test]
    fn a_tolerance_inside_the_cap_is_not_reported_as_capped() {
        assert!(
            !RiskState::from_config(&cfg(RiskLevel::Balanced, Some(3.0)))
                .daily_loss_override_capped
        );
        assert!(
            !RiskState::from_config(&cfg(RiskLevel::Balanced, None)).daily_loss_override_capped
        );
    }
}
