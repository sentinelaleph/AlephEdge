//! What-if before a bot starts (pure): the candidate DCA / Grid bot or New
//! trade as one more exposure of the paper book, so the form can show the
//! portfolio stress at BTC -20% before and after it.

use serde::{Deserialize, Serialize};

use super::exposure::{Body, Dir, Exposure, Source, shock_bar};
use crate::bot::model::{BotKind, OpenPosition};
use crate::bot::strategy::costs::CostModel;
use crate::bot::strategy::cycle::{step_bar, CycleStart, CycleState};
use crate::bot::strategy::model::{MarketKind, Side, StrategyConfig, StrategyParams};
use crate::signal::model::Direction;

/// What the New trade preview already knows (bot_preview_trade).
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TradeCandidate {
    pub bot_kind: BotKind,
    pub symbol: String,
    pub direction: Direction,
    pub entry: f64,
    pub take_profit: f64,
    pub stop_loss: f64,
    pub capital: f64,
    pub notional_usdt: f64,
    pub effective_leverage: f64,
    pub leverage: u8,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Candidate {
    /// A DCA / Grid config at the preview's reference price.
    #[serde(rename_all = "camelCase")]
    Strategy { config: Box<StrategyConfig>, price: f64 },
    #[serde(rename_all = "camelCase")]
    Trade { trade: TradeCandidate },
}

/// Paper book stress at one BTC move, compact.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Brief {
    pub pnl: f64,
    pub pct_of_capital: Option<f64>,
    pub capital: f64,
    pub included: u32,
    pub excluded: u32,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WhatIf {
    pub btc_move_pct: f64,
    /// The book the candidate joins (New trade and DCA / Grid are paper).
    pub live: bool,
    pub before: Option<Brief>,
    pub after: Option<Brief>,
    /// The candidate alone at that move, USDT.
    pub candidate_pnl: Option<f64>,
    pub candidate_beta: Option<super::beta::BetaInfo>,
    /// Why after/candidate are unavailable (`riskview.errors.*` / backtest code).
    pub error: Option<String>,
    pub sample: bool,
}

impl Candidate {
    pub fn symbol(&self) -> String {
        match self {
            Candidate::Strategy { config, .. } => config.symbol.trim().to_uppercase(),
            Candidate::Trade { trade } => trade.symbol.trim().to_uppercase(),
        }
    }

    pub fn market(&self) -> MarketKind {
        match self {
            Candidate::Strategy { config, .. } => config.market,
            Candidate::Trade { trade } if trade.bot_kind.uses_futures_market() => MarketKind::Futures,
            Candidate::Trade { .. } => MarketKind::Spot,
        }
    }

    /// The candidate as an exposure at its entry price, or the i18n code of
    /// why it cannot be built. `max_leverage`: the risk level's ceiling.
    pub fn exposure(&self, max_leverage: u8, t0: u64) -> Result<Exposure, String> {
        match self {
            Candidate::Strategy { config, price } => strategy_exposure(config, *price, max_leverage, t0),
            Candidate::Trade { trade } => trade_exposure(trade),
        }
    }
}

fn strategy_exposure(cfg: &StrategyConfig, price: f64, max_leverage: u8, t0: u64) -> Result<Exposure, String> {
    if !(price.is_finite() && price > 0.0) {
        return Err("whatifNoPrice".into());
    }
    let cfg = StrategyConfig { symbol: cfg.symbol.trim().to_uppercase(), ..cfg.clone() };
    crate::bot::strategy::validate::validate(&cfg, max_leverage).map_err(|e| e.to_string())?;
    let st = CycleStart {
        bot_id: "sb_whatifwhatif",
        seq: 1,
        side: cfg.side,
        budget: cfg.budget,
        leverage: f64::from(cfg.leverage.max(1)),
        size_factor: 1.0,
        costs: CostModel::for_market(cfg.market),
        max_duration_min: None,
    };
    let bar = shock_bar(price, price, t0);
    let mut cycle = match &cfg.params {
        StrategyParams::Dca(p) => CycleState::open_dca(&st, p, &bar),
        StrategyParams::Grid(p) => CycleState::open_grid(&st, p, &bar),
    }
    .map_err(|e| e.to_string())?;
    step_bar(&mut cycle, &bar, true);
    let dd = cfg.max_drawdown_pct.filter(|p| *p > 0.0).map(|pct| super::exposure::DdStop {
        pct,
        peak_equity: cfg.budget,
        budget: cfg.budget,
        realized: 0.0,
    });
    Ok(Exposure {
        id: "candidate".into(),
        source: match cfg.params {
            StrategyParams::Dca(_) => Source::Dca,
            StrategyParams::Grid(_) => Source::Grid,
        },
        label: cfg.name.trim().to_string(),
        symbol: cfg.symbol.clone(),
        market: cfg.market,
        live: false,
        scale: 1.0,
        direction: match cfg.side {
            Side::Long => Dir::Long,
            Side::Short => Dir::Short,
            Side::Neutral => Dir::Neutral,
        },
        capital: cycle.core.budget,
        mark: price,
        body: Body::Cycle { cycle: Box::new(cycle), dd },
    })
}

fn trade_exposure(t: &TradeCandidate) -> Result<Exposure, String> {
    let ok = |v: f64| v.is_finite() && v > 0.0;
    if !(ok(t.entry) && ok(t.stop_loss) && ok(t.take_profit) && ok(t.capital)) {
        return Err("whatifTradeIncomplete".into());
    }
    // Only the fields the P&L and the close step read; the rest default.
    let pos: OpenPosition = serde_json::from_value(serde_json::json!({
        "signalId": "candidate",
        "botKind": t.bot_kind,
        "exchangeId": "binance",
        "symbol": t.symbol.trim().to_uppercase(),
        "direction": t.direction,
        "entry": t.entry,
        "tp": t.take_profit,
        "sl": t.stop_loss,
        "signalEntry": t.entry,
        "leverage": t.leverage.max(1),
        "capital": t.capital,
        "frAtOpen": null,
        "ldAtOpen": null,
        "openedAt": 0,
        "notionalUsdt": t.notional_usdt,
        "effectiveLeverage": t.effective_leverage,
        "manual": true
    }))
    .map_err(|_| "whatifTradeIncomplete".to_string())?;
    Ok(Exposure {
        id: "candidate".into(),
        source: Source::Manual,
        label: t.bot_kind.as_str().into(),
        symbol: pos.symbol.clone(),
        market: if t.bot_kind.uses_futures_market() { MarketKind::Futures } else { MarketKind::Spot },
        live: false,
        scale: 1.0,
        direction: if t.direction == Direction::Long { Dir::Long } else { Dir::Short },
        capital: t.capital,
        mark: t.entry,
        body: Body::Position { pos: Box::new(pos), max_loss_pct: None },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::strategy::validate::tests::dca_cfg;

    fn trade(stop: f64) -> TradeCandidate {
        TradeCandidate {
            bot_kind: BotKind::Futures,
            symbol: " solusdt ".into(),
            direction: Direction::Long,
            entry: 100.0,
            take_profit: 120.0,
            stop_loss: stop,
            capital: 50.0,
            notional_usdt: 100.0,
            effective_leverage: 2.0,
            leverage: 2,
        }
    }

    #[test]
    fn a_trade_candidate_respects_its_stop() {
        let e = Candidate::Trade { trade: trade(95.0) }.exposure(5, 0).unwrap();
        assert_eq!(e.symbol, "SOLUSDT");
        let out = e.shock(1.0, -20.0, 0);
        assert_eq!(out.exit, Some("sl"));
        // -5% on 100 notional = -5, plus both taker fees on 100 (0.1),
        // which the mark-to-market at entry already carries.
        assert!((out.pnl + e.mtm_now() - (-5.0 - 0.1)).abs() < 1e-6, "{out:?}");
        assert!((e.mtm_now() + 0.1).abs() < 1e-9);
        assert_eq!(
            Candidate::Trade { trade: TradeCandidate { entry: 0.0, ..trade(95.0) } }.exposure(5, 0).unwrap_err(),
            "whatifTradeIncomplete"
        );
    }

    #[test]
    fn a_dca_candidate_fills_its_ladder_and_needs_a_price() {
        let cfg = dca_cfg();
        let c = Candidate::Strategy { config: Box::new(cfg.clone()), price: 100.0 };
        let e = c.exposure(5, 0).unwrap();
        assert_eq!(c.market(), MarketKind::Futures);
        // 2 safety orders 2% apart, no stop: -20% fills both.
        let out = e.shock(1.0, -20.0, 0);
        assert!(out.pnl < -50.0 && out.exit.is_none(), "{out:?}");
        assert_eq!(Candidate::Strategy { config: Box::new(cfg), price: 0.0 }.exposure(5, 0).unwrap_err(), "whatifNoPrice");
    }
}
