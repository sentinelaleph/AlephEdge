//! Strategy-bot caps, kept out of the shared `risk/model.rs` on purpose (the
//! paper runner mounts that file). Pure; the risk level enters as its table
//! values so this file never imports the risk module.

/// Real money for DCA/Grid is not built. Any future live adapter must also
/// pass this constant, which only an explicit owner decision may flip.
pub const STRATEGY_LIVE_ALLOWED: bool = false;

/// Strategy bots per install (paper).
pub const MAX_STRATEGY_BOTS: usize = 10;
pub const MAX_DCA_SO: u8 = 25;
pub const MAX_GRID_LEVELS: u16 = 100;
pub const MIN_GRID_LEVELS: u16 = 2;
pub const MAX_NAME_CHARS: usize = 40;

/// Per-bot drawdown stop offered by default in the form, % of budget.
pub const DEFAULT_BOT_DD_STOP_PCT: f64 = 25.0;
/// Portfolio breaker: all strategy equity this % below its peak closes all.
pub const PORTFOLIO_DD_STOP_PCT: f64 = 15.0;

/// Smallest order notional accepted (Binance's common 5 USDT minimum on
/// both markets). Every order of the ladder or grid must clear it.
pub const MIN_ORDER_NOTIONAL: f64 = 5.0;

/// Price feed older than this holds fills and new cycles.
pub const PRICE_STALE_MS: u64 = 3 * 60_000;

/// Level index 0..=4 (Cautious..Greedy) -> share of the declared balance
/// all strategy budgets together may reserve, %. Owner-tunable proposal.
pub const STRATEGY_BUDGET_CAP_PCT: [f64; 5] = [20.0, 30.0, 40.0, 60.0, 80.0];

/// Budget cap for a level index (out of range -> the strictest).
pub fn budget_cap_pct(level_index: usize) -> f64 {
    STRATEGY_BUDGET_CAP_PCT
        .get(level_index)
        .copied()
        .unwrap_or(STRATEGY_BUDGET_CAP_PCT[0])
}

/// Leverage ceiling: the risk level's max leverage; spot is always 1.
pub fn leverage_ceiling(level_max_leverage: u8, spot: bool) -> u8 {
    if spot {
        1
    } else {
        level_max_leverage.max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_live_is_structurally_off() {
        // The test exists so flipping the constant is a reviewed decision.
        const _: () = assert!(!STRATEGY_LIVE_ALLOWED);
    }

    #[test]
    fn caps_by_level() {
        assert_eq!(budget_cap_pct(0), 20.0);
        assert_eq!(budget_cap_pct(4), 80.0);
        assert_eq!(budget_cap_pct(9), 20.0);
        assert_eq!(leverage_ceiling(20, true), 1);
        assert_eq!(leverage_ceiling(5, false), 5);
    }
}
