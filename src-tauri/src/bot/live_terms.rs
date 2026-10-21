//! The money a Live bot puts at risk, as the confirmation shows it and the
//! command re-checks it.
//!
//! Every real-money switch (a new Live bot, "Go live" on a Paper bot, the
//! per-session arm after a restart, a Live New trade) shows these figures
//! next to the typed LIVE, and sends back the margin it showed. The command
//! recomputes the terms from the settings in force and refuses when they no
//! longer match (`liveTermsChanged`): the user confirmed THIS money, never
//! whatever the settings say by the time the call lands.
//!
//! Pure: no Tauri, no store.

use serde::Serialize;

use super::model::{BotConfig, PILOT_NOTIONAL_USDT, PILOT_TRADES};
use super::strategy::model::StrategyConfig;
use super::strategy_live::{PILOT_CYCLES, PILOT_STRATEGY_USDT};
use crate::risk::model::RiskLimits;

/// The figures one confirmation shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTerms {
    /// Exchange the real orders go to.
    pub venue: String,
    /// Most margin the bot can have on the exchange at once (USDT): capital
    /// per position x positions for a signal bot, the TradingView bot's
    /// budget, the budget for DCA / Grid. On isolated margin this is also the
    /// most it can lose.
    pub margin_at_risk_usdt: f64,
    /// Largest ONE position (notional, USDT) at full size: capital x leverage
    /// for a signal bot, the TradingView bot's max size per alert (never more
    /// than its budget carries at the leverage cap), budget x leverage for
    /// DCA / Grid. The pilot caps the first entries lower (`pilot_*`).
    pub max_notional_usdt: f64,
    /// The margin that largest position ties up (`max_notional / leverage`).
    pub max_position_margin_usdt: f64,
    /// Leverage cap in force (the bot's own, clamped to the risk level).
    pub leverage: u8,
    /// Signal bot: positions at once. DCA / Grid: 1.
    pub positions: u8,
    /// Pilot: the first `pilot_count` real entries (signal bot) or cycles
    /// (DCA / Grid) are capped near `pilot_notional_usdt`.
    pub pilot_count: u32,
    pub pilot_notional_usdt: f64,
}

/// A signal bot's terms under `limits`. The TradingView bot's margin is its
/// budget (the alert guard's cap on all its margin, `guard::limit_refusal`)
/// and its largest position the max size per alert, bounded by what that
/// budget carries at the leverage cap.
pub fn signal_terms(cfg: &BotConfig, limits: &RiskLimits) -> LiveTerms {
    let leverage = super::engine::precheck::effective_leverage(cfg, limits);
    let lev = f64::from(leverage.max(1));
    let (margin, max_notional) = match cfg.tv.as_ref().filter(|_| cfg.kind == super::model::BotKind::Tradingview) {
        Some(tv) => (tv.budget_usdt, tv.max_size_usdt.min(tv.budget_usdt * lev)),
        None => (cfg.capital * f64::from(cfg.max_positions), cfg.capital * lev),
    };
    LiveTerms {
        venue: cfg.exchange_id.clone(),
        margin_at_risk_usdt: round_cents(margin),
        max_notional_usdt: round_cents(max_notional),
        max_position_margin_usdt: round_cents(max_notional / lev),
        leverage,
        positions: cfg.max_positions,
        pilot_count: PILOT_TRADES,
        pilot_notional_usdt: PILOT_NOTIONAL_USDT,
    }
}

/// A DCA / Grid bot's terms with real orders on `venue`.
pub fn strategy_terms(cfg: &StrategyConfig, venue: &str) -> LiveTerms {
    let leverage = cfg.leverage.max(1);
    LiveTerms {
        venue: venue.to_string(),
        margin_at_risk_usdt: round_cents(cfg.budget),
        max_notional_usdt: round_cents(cfg.budget * f64::from(leverage)),
        max_position_margin_usdt: round_cents(cfg.budget),
        leverage,
        positions: 1,
        pilot_count: PILOT_CYCLES,
        pilot_notional_usdt: PILOT_STRATEGY_USDT,
    }
}

fn round_cents(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Refuses a confirmation whose margin is not the one in force now
/// (`None`: the caller sent none, an older UI or a stray call).
pub fn confirmed(terms: &LiveTerms, confirmed_margin_usdt: Option<f64>) -> Result<(), String> {
    match confirmed_margin_usdt {
        Some(m) if m.is_finite() && (m - terms.margin_at_risk_usdt).abs() < 0.005 => Ok(()),
        Some(_) => Err(format!("liveTermsChanged|{:.2}", terms.margin_at_risk_usdt)),
        None => Err("liveTermsRequired".to_string()),
    }
}

/// One real entry's margin against the margin its confirmation showed (a
/// Live New trade sends the preview's margin back): refused when the plan
/// made at the open no longer puts the same money at risk.
pub fn confirmed_entry(planned_margin_usdt: f64, confirmed_margin_usdt: Option<f64>) -> Result<(), String> {
    match confirmed_margin_usdt {
        Some(m) if m.is_finite() && (m - planned_margin_usdt).abs() < 0.005 => Ok(()),
        Some(_) => Err(format!("liveTermsChanged|{:.2}", round_cents(planned_margin_usdt))),
        None => Err("liveTermsRequired".to_string()),
    }
}

/// An unattended real entry (a TradingView alert) against what the bot's
/// typed LIVE confirmed this session: `armed` None = not armed (no entry);
/// any figure that changed since (a new risk level clamps the leverage
/// differently, a new venue) refuses until LIVE is typed for the new terms.
pub fn same_terms(armed: Option<&LiveTerms>, now: &LiveTerms) -> Result<(), String> {
    match armed {
        None => Err("liveArmRequired".to_string()),
        Some(a) if a == now => Ok(()),
        Some(_) => Err(format!("liveTermsChanged|{:.2}", now.margin_at_risk_usdt)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_real_entry_opens_only_on_the_margin_its_confirmation_showed() {
        assert_eq!(confirmed_entry(12.5, Some(12.5)), Ok(()));
        assert_eq!(confirmed_entry(12.5, Some(12.504)), Ok(()));
        assert_eq!(confirmed_entry(12.5, Some(15.0)), Err("liveTermsChanged|12.50".to_string()));
        assert_eq!(confirmed_entry(12.5, Some(f64::INFINITY)), Err("liveTermsChanged|12.50".to_string()));
        assert_eq!(confirmed_entry(12.5, None), Err("liveTermsRequired".to_string()));
    }

    #[test]
    fn an_alert_entry_needs_the_terms_the_arm_confirmed() {
        let mut cfg = BotConfig::new_default(crate::bot::model::BotKind::Tradingview, "binance");
        (cfg.capital, cfg.max_positions, cfg.leverage) = (20.0, 5, 50);
        let balanced = signal_terms(&cfg, &crate::risk::model::RiskLevel::Balanced.limits());
        assert_eq!(same_terms(Some(&balanced), &balanced), Ok(()));
        assert_eq!(same_terms(None, &balanced), Err("liveArmRequired".to_string()));
        // The risk level changed after the arm: the leverage clamp moved.
        let cautious = signal_terms(&cfg, &crate::risk::model::RiskLevel::Cautious.limits());
        assert_ne!(cautious.leverage, balanced.leverage, "the levels clamp 50x differently");
        assert_eq!(same_terms(Some(&balanced), &cautious), Err("liveTermsChanged|100.00".to_string()));
        let mut moved = balanced.clone();
        moved.venue = "bybit".into();
        assert!(same_terms(Some(&balanced), &moved).is_err());
    }
    use crate::bot::model::BotKind;
    use crate::risk::model::RiskLevel;

    #[test]
    fn a_signal_bot_risks_its_capital_on_every_position_at_the_clamped_leverage() {
        let mut cfg = BotConfig::new_default(BotKind::Futures, "binance");
        (cfg.capital, cfg.max_positions, cfg.leverage) = (40.0, 3, 50);
        let limits = RiskLevel::Cautious.limits();
        let t = signal_terms(&cfg, &limits);
        assert_eq!(t.margin_at_risk_usdt, 120.0);
        assert_eq!(t.leverage, limits.max_leverage, "never above the risk level's cap");
        // One position: its own capital at the leverage, never every position's.
        assert_eq!(t.max_notional_usdt, 40.0 * f64::from(limits.max_leverage));
        assert_eq!(t.max_position_margin_usdt, 40.0);
        assert_eq!((t.pilot_count, t.pilot_notional_usdt), (PILOT_TRADES, PILOT_NOTIONAL_USDT));
        assert_eq!(t.venue, "binance");
    }

    // Testnet run, 21 Oct: budget 100, 3x, max size 50, 3 positions showed
    // "Largest position 299.97 USDT" (capital 33.33 x 3 positions x 3x).
    #[test]
    fn the_tradingview_bots_largest_position_is_its_max_size_per_alert() {
        let mut cfg = BotConfig::new_default(BotKind::Tradingview, "binance");
        (cfg.max_positions, cfg.leverage, cfg.capital) = (3, 3, 33.33);
        {
            let tv = cfg.tv.as_mut().unwrap();
            (tv.budget_usdt, tv.max_size_usdt) = (100.0, 50.0);
        }
        let t = signal_terms(&cfg, &RiskLevel::Balanced.limits());
        assert_eq!(t.leverage, 3);
        assert_eq!(t.margin_at_risk_usdt, 100.0, "the budget, not 33.33 x 3");
        assert_eq!(t.max_notional_usdt, 50.0);
        assert_eq!(t.max_position_margin_usdt, 16.67);
        // A max size the budget cannot carry is bounded by the budget.
        cfg.tv.as_mut().unwrap().max_size_usdt = 500.0;
        let t = signal_terms(&cfg, &RiskLevel::Balanced.limits());
        assert_eq!((t.max_notional_usdt, t.max_position_margin_usdt), (300.0, 100.0));
        // The re-check uses the same terms: a new max size is new terms.
        let armed = t.clone();
        cfg.tv.as_mut().unwrap().max_size_usdt = 60.0;
        assert!(same_terms(Some(&armed), &signal_terms(&cfg, &RiskLevel::Balanced.limits())).is_err());
        assert_eq!(confirmed(&armed, Some(100.0)), Ok(()));
    }

    #[test]
    fn a_strategy_bot_risks_its_budget() {
        let cfg = crate::bot::strategy::commands::default_config(
            crate::bot::strategy::model::StrategyKind::Dca,
            "binance".into(),
            "BTCUSDT".into(),
        );
        let t = strategy_terms(&cfg, "binance");
        assert_eq!(t.margin_at_risk_usdt, (cfg.budget * 100.0).round() / 100.0);
        assert_eq!(t.max_notional_usdt, ((cfg.budget * f64::from(cfg.leverage.max(1))) * 100.0).round() / 100.0);
        assert_eq!((t.positions, t.pilot_count), (1, PILOT_CYCLES));
    }

    #[test]
    fn only_the_margin_in_force_confirms() {
        let mut cfg = BotConfig::new_default(BotKind::Futures, "binance");
        (cfg.capital, cfg.max_positions) = (25.0, 2);
        let t = signal_terms(&cfg, &RiskLevel::Balanced.limits());
        assert_eq!(confirmed(&t, Some(50.0)), Ok(()));
        assert_eq!(confirmed(&t, Some(50.004)), Ok(()), "a display rounding");
        assert_eq!(confirmed(&t, Some(75.0)), Err("liveTermsChanged|50.00".to_string()));
        assert_eq!(confirmed(&t, Some(f64::NAN)), Err("liveTermsChanged|50.00".to_string()));
        assert_eq!(confirmed(&t, None), Err("liveTermsRequired".to_string()));
    }
}
