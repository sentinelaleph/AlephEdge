//! Pure cost model for paper strategy cycles. The futures numbers are the
//! exact research-simulator (botsim) constants, so a preset proven there
//! decides identically here. Signal-bot fee constants in `engine/record.rs`
//! are separate and untouched.

use serde::{Deserialize, Serialize};

use super::model::{Liquidity, MarketKind};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CostModel {
    /// Fee on resting limit fills, fraction of notional.
    pub maker: f64,
    /// Fee on market orders, fraction of notional.
    pub taker: f64,
    /// Adverse slippage on market orders, fraction of price.
    pub slippage: f64,
    /// Maintenance margin rate (isolated, flat, no tiers). 0 = not applicable.
    pub mmr: f64,
    /// Funding is charged (futures only).
    pub funding: bool,
}

/// USDT-M perpetual: botsim MAKER / TAKER / SLIP / MM.
pub const FUTURES_SIM: CostModel = CostModel {
    maker: 0.0002,
    taker: 0.0005,
    slippage: 0.0002,
    mmr: 0.005,
    funding: true,
};

/// Spot: 0.1% both sides, same slippage, no funding, no leverage.
pub const SPOT_SIM: CostModel = CostModel {
    maker: 0.001,
    taker: 0.001,
    slippage: 0.0002,
    mmr: 0.0,
    funding: false,
};

impl CostModel {
    pub fn for_market(market: MarketKind) -> CostModel {
        match market {
            MarketKind::Futures => FUTURES_SIM,
            MarketKind::Spot => SPOT_SIM,
        }
    }

    pub fn rate(&self, liquidity: Liquidity) -> f64 {
        match liquidity {
            Liquidity::Maker => self.maker,
            Liquidity::Taker => self.taker,
        }
    }

    pub fn fee(&self, notional: f64, liquidity: Liquidity) -> f64 {
        notional.abs() * self.rate(liquidity)
    }

    /// Market fill price: a buy pays up, a sell gets less.
    pub fn market_fill_price(&self, px: f64, buy: bool) -> f64 {
        if buy {
            px * (1.0 + self.slippage)
        } else {
            px * (1.0 - self.slippage)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn futures_constants_are_botsim_exact() {
        assert_eq!(FUTURES_SIM.maker, 0.0002);
        assert_eq!(FUTURES_SIM.taker, 0.0005);
        assert_eq!(FUTURES_SIM.slippage, 0.0002);
        assert_eq!(FUTURES_SIM.mmr, 0.005);
        const _: () = assert!(FUTURES_SIM.funding && !SPOT_SIM.funding);
        assert_eq!(SPOT_SIM.maker, 0.001);
        assert_eq!(SPOT_SIM.taker, 0.001);
    }

    #[test]
    fn market_fill_is_adverse() {
        let c = FUTURES_SIM;
        assert!((c.market_fill_price(100.0, true) - 100.02).abs() < 1e-12);
        assert!((c.market_fill_price(100.0, false) - 99.98).abs() < 1e-12);
        assert!((c.fee(1000.0, Liquidity::Taker) - 0.5).abs() < 1e-12);
        assert!((c.fee(-1000.0, Liquidity::Maker) - 0.2).abs() < 1e-12);
    }
}
