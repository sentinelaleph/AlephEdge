//! Pure bounds checks and the derived preview the form renders. Rust is the
//! authority: the TypeScript side mirrors only the error codes, never the
//! bounds, and renders the preview as served.

use serde::Serialize;

use super::costs::CostModel;
use super::dca::{self, LadderRung};
use super::grid;
use super::limits::{
    leverage_ceiling, MAX_DCA_SO, MAX_GRID_LEVELS, MAX_NAME_CHARS, MIN_GRID_LEVELS,
    MIN_ORDER_NOTIONAL,
};
use super::liquidation::grid_liq_price;
use super::model::{
    DcaParams, GridParams, GridRange, MarketKind, Side, StartCondition, StrategyConfig,
    StrategyKind, StrategyParams,
};

/// A refused config: `code` is the i18n key under `strategy.errors.*`.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StrategyError {
    pub code: &'static str,
    pub field: Option<&'static str>,
}

impl StrategyError {
    fn new(code: &'static str, field: &'static str) -> Self {
        Self {
            code,
            field: Some(field),
        }
    }
}

impl std::fmt::Display for StrategyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.field {
            Some(field) => write!(f, "{}|{}", self.code, field),
            None => write!(f, "{}", self.code),
        }
    }
}

type V = Result<(), StrategyError>;

fn need(ok: bool, code: &'static str, field: &'static str) -> V {
    if ok {
        Ok(())
    } else {
        Err(StrategyError::new(code, field))
    }
}

fn pos(v: f64) -> bool {
    v.is_finite() && v > 0.0
}

/// Every bound and every structural rule. `max_leverage` = the risk level's
/// ceiling.
pub fn validate(cfg: &StrategyConfig, max_leverage: u8) -> V {
    let name = cfg.name.trim();
    need(
        !name.is_empty() && name.chars().count() <= MAX_NAME_CHARS,
        "nameInvalid",
        "name",
    )?;
    need(cfg.exchange_id == "binance", "exchangeUnsupported", "exchangeId")?;
    need(
        cfg.symbol.len() > 4
            && cfg.symbol.len() <= 20
            && cfg.symbol.ends_with("USDT")
            && cfg.symbol.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()),
        "symbolInvalid",
        "symbol",
    )?;
    need(pos(cfg.budget), "budgetInvalid", "budget")?;
    let spot = cfg.market == MarketKind::Spot;
    if spot {
        need(cfg.side == Side::Long, "spotShortRefused", "side")?;
        need(cfg.leverage == 1, "spotLeverage", "leverage")?;
    }
    need(cfg.leverage >= 1, "leverageInvalid", "leverage")?;
    need(
        cfg.leverage <= leverage_ceiling(max_leverage, spot),
        "leverageAboveCeiling",
        "leverage",
    )?;
    if let Some(dd) = cfg.max_drawdown_pct {
        need(pos(dd) && dd <= 100.0, "drawdownInvalid", "maxDrawdownPct")?;
    }
    if let StartCondition::PriceCross { price, .. } = cfg.start {
        need(pos(price), "startPriceInvalid", "start")?;
    }
    let r = &cfg.restart;
    need(r.cooldown_min <= 7 * 24 * 60, "cooldownInvalid", "restart")?;
    need(r.max_cycles.map_or(true, |m| m >= 1), "maxCyclesInvalid", "maxCycles")?;
    if let Some((lo, hi)) = r.price_band {
        need(pos(lo) && pos(hi) && lo < hi, "priceBandInvalid", "priceBand")?;
    }
    if let Some(d) = cfg.params.max_duration_min() {
        need(d >= 1, "durationInvalid", "maxDurationMin")?;
    }
    match &cfg.params {
        StrategyParams::Dca(p) => validate_dca(cfg, p),
        StrategyParams::Grid(p) => validate_grid(cfg, p),
    }
}

fn side_sign(side: Side) -> f64 {
    if side == Side::Short {
        -1.0
    } else {
        1.0
    }
}

fn validate_dca(cfg: &StrategyConfig, p: &DcaParams) -> V {
    need(cfg.side != Side::Neutral, "neutralDcaRefused", "side")?;
    let notional_form = p.base_order.is_some() || p.safety_order.is_some();
    let weight_form = p.base_weight.is_some() || p.safety_weight.is_some();
    need(notional_form != weight_form, "orderFormExclusive", "baseOrder")?;
    if notional_form {
        need(p.base_order.is_some_and(pos), "baseOrderInvalid", "baseOrder")?;
        if p.max_so > 0 {
            need(p.safety_order.is_some_and(pos), "safetyOrderInvalid", "safetyOrder")?;
        }
    } else {
        need(p.base_weight.map_or(true, pos), "weightInvalid", "baseWeight")?;
        need(p.safety_weight.map_or(true, pos), "weightInvalid", "safetyWeight")?;
    }
    need(p.max_so <= MAX_DCA_SO, "maxSoAboveCap", "maxSo")?;
    if p.max_so > 0 {
        need(pos(p.so_step_pct) && p.so_step_pct < 100.0, "soStepInvalid", "soStepPct")?;
    }
    need(p.step_scale.is_finite() && p.step_scale >= 1.0 && p.step_scale <= 10.0, "stepScaleInvalid", "stepScale")?;
    need(
        p.volume_scale.is_finite() && p.volume_scale >= 1.0 && p.volume_scale <= 10.0,
        "volumeScaleInvalid",
        "volumeScale",
    )?;
    need(pos(p.tp_pct) && p.tp_pct <= 100.0, "tpInvalid", "tpPct")?;
    // A short's take profit sits at avg x (1 - tp%): 100% is a price of 0,
    // an order that can never fill.
    need(cfg.side != Side::Short || p.tp_pct < 100.0, "tpInvalid", "tpPct")?;
    if let Some(t) = p.trailing_pct {
        need(pos(t) && t < 50.0, "trailingInvalid", "trailingPct")?;
        // The trail arms at the take profit and exits t% off the peak: with
        // t >= tp% that exit can sit below the average entry, so a cycle that
        // reached its target closes at a loss.
        need(t < p.tp_pct, "trailingAboveTp", "trailingPct")?;
    }
    if let Some(sl) = p.sl_pct {
        need(pos(sl) && sl < 100.0, "slInvalid", "slPct")?;
    }
    let s = side_sign(cfg.side);
    let devs = dca::deviations(p);
    if s > 0.0 {
        need(devs.last().map_or(true, |d| *d < 100.0), "ladderBelowZero", "soStepPct")?;
    }
    let lev = f64::from(cfg.leverage.max(1));
    let (bo, sos) = dca::ladder_notionals(p, cfg.budget, lev, 1.0);
    need(
        dca::required_capital(bo, &sos, lev) <= cfg.budget * (1.0 + 1e-9),
        "ladderExceedsBudget",
        "budget",
    )?;
    let smallest = sos.iter().copied().fold(bo, f64::min);
    need(smallest >= MIN_ORDER_NOTIONAL, "budgetBelowMinNotional", "budget")?;
    // The last safety order must sit inside the liquidation price of the
    // fully filled ladder (0.5% margin), or averaging down ends in a
    // liquidation the user never saw.
    let costs = CostModel::for_market(cfg.market);
    let rungs = dca::build_ladder(p, 100.0, s, cfg.budget, lev, costs.slippage, costs.mmr);
    let beyond = |price: f64, liq: f64| {
        if s > 0.0 {
            price <= liq * 1.005
        } else {
            price >= liq * 0.995
        }
    };
    if let (Some(last), Some(liq)) = (rungs.last(), rungs.last().and_then(|r| r.liq_price)) {
        need(!beyond(last.price, liq), "ladderBeyondLiquidation", "maxSo")?;
    }
    // Every safety order must also be reached before the liquidation price
    // of the position held up to it: margin grows with each fill, so the
    // EARLY rungs liquidate closest to the anchor (5x, one SO at -25%:
    // the base alone liquidates near -19.6%, before the SO can fill).
    for w in rungs.windows(2) {
        if let Some(liq) = w[0].liq_price {
            need(!beyond(w[1].price, liq), "ladderBeyondLiquidation", "maxSo")?;
        }
    }
    // The stop follows the average entry. If the stop of the position held
    // up to a rung sits at or past the next safety order, the stop fires
    // before that order can fill and the rest of the ladder is dead weight.
    if let Some(sl) = p.sl_pct {
        for w in rungs.windows(2) {
            let stop = dca::sl_price(w[0].avg_entry, sl, s);
            let reached_first = if s > 0.0 { stop >= w[1].price } else { stop <= w[1].price };
            need(!reached_first, "slInsideLadder", "slPct")?;
        }
    }
    Ok(())
}

/// Grid range at a reference start price (relative ranges are scale-free).
/// The smallest order of the config at full size (size factor 1), quote
/// notional. Orders scale linearly with the size factor, so a sized-down
/// cycle's smallest order is this times the factor.
pub fn smallest_order_notional(cfg: &StrategyConfig) -> f64 {
    let lev = f64::from(cfg.leverage.max(1));
    match &cfg.params {
        StrategyParams::Dca(p) => {
            let (bo, sos) = dca::ladder_notionals(p, cfg.budget, lev, 1.0);
            sos.iter().copied().fold(bo, f64::min)
        }
        StrategyParams::Grid(p) => {
            let (lower, upper, _) = grid_reference(p);
            let lv = grid::levels(lower, upper, p.n_grids, p.spacing);
            grid::qty_per_level(cfg.budget, lev, &lv) * lv[0]
        }
    }
}

fn grid_reference(p: &GridParams) -> (f64, f64, f64) {
    match p.range {
        GridRange::Absolute { lower, upper } => (lower, upper, (lower * upper).sqrt()),
        GridRange::Relative { .. } => {
            let (lo, hi) = grid::range_at(p.range, 100.0);
            (lo, hi, 100.0)
        }
    }
}

fn validate_grid(cfg: &StrategyConfig, p: &GridParams) -> V {
    need(
        p.n_grids >= MIN_GRID_LEVELS && p.n_grids <= MAX_GRID_LEVELS,
        "gridLevelsInvalid",
        "nGrids",
    )?;
    match p.range {
        GridRange::Absolute { lower, upper } => {
            need(pos(lower) && pos(upper) && lower < upper, "gridRangeInvalid", "range")?
        }
        GridRange::Relative {
            lower_pct,
            upper_pct,
        } => need(
            pos(lower_pct) && lower_pct < 100.0 && pos(upper_pct) && upper_pct <= 1000.0,
            "gridRangeInvalid",
            "range",
        )?,
    }
    if let Some(x) = p.stop_out_pct {
        need(pos(x) && x <= 50.0, "stopOutInvalid", "stopOutPct")?;
    }
    if p.trailing_up {
        need(cfg.side == Side::Long, "trailingUpLongOnly", "trailingUp")?;
    }
    if let Some(t) = p.take_profit_pct {
        need(pos(t), "gridTpInvalid", "takeProfitPct")?;
    }
    let (lower, upper, start) = grid_reference(p);
    if let (Some(limit), GridRange::Absolute { .. }) = (p.trail_up_limit, p.range) {
        need(limit > upper, "trailLimitInvalid", "trailUpLimit")?;
    }
    let lv = grid::levels(lower, upper, p.n_grids, p.spacing);
    let costs = CostModel::for_market(cfg.market);
    let (min_step, _) = grid::profit_per_grid_pct(&lv, costs.maker);
    need(min_step > 0.0, "gridStepBelowFees", "nGrids")?;
    let lev = f64::from(cfg.leverage.max(1));
    let q = grid::qty_per_level(cfg.budget, lev, &lv);
    need(q * lv[0] >= MIN_ORDER_NOTIONAL, "budgetBelowMinNotional", "budget")?;
    need(grid_liq_clear_at(cfg, p, start), "gridLiqInsideBand", "leverage")
}

/// Whether the worst-case inventories of a grid opened at `start` liquidate
/// outside its stop band. Relative ranges are scale-free (the same answer at
/// every start); an ABSOLUTE range validated at its geometric mid can fail at
/// the real start (a long opened near the top buys more at the open), so the
/// driver asks again with the cycle's actual start price.
pub fn grid_liq_clear_at(cfg: &StrategyConfig, p: &GridParams, start: f64) -> bool {
    let (lower, upper) = grid::range_at(p.range, start);
    if !(lower > 0.0 && lower < upper) || p.n_grids < 1 {
        return true; // refused elsewhere (gridRangeInvalid / gridBadRange)
    }
    let lv = grid::levels(lower, upper, p.n_grids, p.spacing);
    let lev = f64::from(cfg.leverage.max(1));
    let costs = CostModel::for_market(cfg.market);
    let q = grid::qty_per_level(cfg.budget, lev, &lv);
    let w = grid_worst(cfg, p, &lv, start, q, costs.mmr);
    let (lo_stop, hi_stop) = grid::stop_band(lower, upper, p.stop_out_pct, p.trailing_up);
    w.liq_bottom.map_or(true, |liq| liq < lo_stop.unwrap_or(lower))
        && w.liq_top.map_or(true, |liq| liq > hi_stop.unwrap_or(upper))
}

/// Worst-case inventories of a grid: every buy filled at the bottom, every
/// sell filled at the top, and their liquidation prices (fees ignored).
struct GridWorst {
    liq_bottom: Option<f64>,
    liq_top: Option<f64>,
}

fn grid_worst(cfg: &StrategyConfig, _p: &GridParams, lv: &[f64], start: f64, q: f64, mmr: f64) -> GridWorst {
    let sides = grid::initial_orders(lv, start);
    let buys: Vec<f64> = lv.iter().zip(&sides).filter(|(_, s)| **s == 1).map(|(l, _)| *l).collect();
    let sells: Vec<f64> = lv.iter().zip(&sides).filter(|(_, s)| **s == -1).map(|(l, _)| *l).collect();
    let budget = cfg.budget;
    let (mut bottom, mut top) = (None, None);
    match cfg.side {
        Side::Long => {
            let qty = q * (sells.len() + buys.len()) as f64;
            let cost = q * sells.len() as f64 * start + q * buys.iter().sum::<f64>();
            if qty > 0.0 {
                bottom = grid_liq_price(qty, cost / qty, budget, mmr);
            }
        }
        Side::Short => {
            let qty = q * (sells.len() + buys.len()) as f64;
            let cost = q * buys.len() as f64 * start + q * sells.iter().sum::<f64>();
            if qty > 0.0 {
                top = grid_liq_price(-qty, cost / qty, budget, mmr);
            }
        }
        Side::Neutral => {
            if !buys.is_empty() {
                let qty = q * buys.len() as f64;
                bottom = grid_liq_price(qty, q * buys.iter().sum::<f64>() / qty, budget, mmr);
            }
            if !sells.is_empty() {
                let qty = q * sells.len() as f64;
                top = grid_liq_price(-qty, q * sells.iter().sum::<f64>() / qty, budget, mmr);
            }
        }
    }
    GridWorst {
        liq_bottom: bottom,
        liq_top: top,
    }
}

// ---------------------------------------------------------------- preview

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DcaPreview {
    pub rungs: Vec<LadderRung>,
    pub last_so_price: Option<f64>,
    /// Price move the full ladder covers from the anchor, %.
    pub max_coverage_pct: f64,
    /// Liquidation price of the fully filled ladder (None at 1x long).
    pub liq_price: Option<f64>,
    /// Distance from the last safety order to that liquidation price, %.
    pub liq_distance_pct: Option<f64>,
    pub total_notional: f64,
    /// Loss of the fully filled ladder at the stop (or the margin at
    /// liquidation); None when neither exists.
    pub worst_loss_quote: Option<f64>,
    /// "sl" | "liq" | "none".
    pub worst_loss_basis: &'static str,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GridLevelRow {
    pub index: u16,
    pub price: f64,
    /// "buy" | "sell" | "none" at the reference price.
    pub side: &'static str,
    pub qty: f64,
    pub notional: f64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GridPreview {
    pub lower: f64,
    pub upper: f64,
    pub levels: Vec<GridLevelRow>,
    pub qty_per_level: f64,
    pub profit_per_grid_min_pct: f64,
    pub profit_per_grid_max_pct: f64,
    pub buy_orders: u32,
    pub sell_orders: u32,
    /// Initial position bought (long) or sold (short) at start, base units.
    pub initial_base_qty: f64,
    pub initial_quote: f64,
    pub stop_lower: Option<f64>,
    pub stop_upper: Option<f64>,
    pub liq_price_bottom: Option<f64>,
    pub liq_price_top: Option<f64>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreviewDto {
    pub kind: StrategyKind,
    pub reference_price: f64,
    /// Margin the bot reserves (full DCA ladder / whole grid budget).
    pub required_capital: f64,
    /// Fees, slippage and maintenance margin the simulation charges.
    pub costs: CostModel,
    pub dca: Option<DcaPreview>,
    pub grid: Option<GridPreview>,
    /// i18n codes under `strategy.warnings.*`.
    pub warnings: Vec<&'static str>,
    /// Refusal code when the config would not pass `validate`.
    pub error: Option<StrategyError>,
}

/// Derived summary at `price` (the last price, or the grid's own range).
pub fn preview(cfg: &StrategyConfig, price: f64, max_leverage: u8) -> PreviewDto {
    let error = validate(cfg, max_leverage).err();
    let costs = CostModel::for_market(cfg.market);
    let lev = f64::from(cfg.leverage.max(1));
    let mut warnings = Vec::new();
    if cfg.leverage > 1 {
        warnings.push("leveraged");
    }
    if cfg.max_drawdown_pct.is_none() {
        warnings.push("noDrawdownStop");
    }
    match &cfg.params {
        StrategyParams::Dca(p) => {
            let s = side_sign(cfg.side);
            let rungs = dca::build_ladder(p, price, s, cfg.budget, lev, costs.slippage, costs.mmr);
            let (bo, sos) = dca::ladder_notionals(p, cfg.budget, lev, 1.0);
            let last = rungs.last().cloned();
            let liq = last.as_ref().and_then(|r| r.liq_price);
            let last_so_price = (p.max_so > 0).then(|| last.as_ref().map(|r| r.price)).flatten();
            let total_notional = bo + sos.iter().sum::<f64>();
            let (worst_loss_quote, basis) = match (p.sl_pct, liq) {
                (Some(sl), _) => (Some(total_notional * sl / 100.0 + total_notional * (costs.taker + costs.slippage)), "sl"),
                (None, Some(_)) => (Some(total_notional / lev), "liq"),
                (None, None) => (None, "none"),
            };
            if p.sl_pct.is_none() {
                warnings.push("noStopLoss");
            }
            DcaPreview {
                liq_distance_pct: match (last_so_price, liq) {
                    (Some(so), Some(l)) => Some((l / so - 1.0).abs() * 100.0),
                    _ => None,
                },
                max_coverage_pct: dca::deviations(p).last().copied().unwrap_or(0.0),
                rungs,
                last_so_price,
                liq_price: liq,
                total_notional,
                worst_loss_quote,
                worst_loss_basis: basis,
            }
            .into_dto(cfg, price, dca::required_capital(bo, &sos, lev), warnings, error)
        }
        StrategyParams::Grid(p) => {
            let (lower, upper) = grid::range_at(p.range, price);
            let lv = grid::levels(lower, upper, p.n_grids.max(1), p.spacing);
            let q = grid::qty_per_level(cfg.budget, lev, &lv);
            let sides = grid::initial_orders(&lv, price);
            let (pmin, pmax) = grid::profit_per_grid_pct(&lv, costs.maker);
            let buys = sides.iter().filter(|s| **s == 1).count() as u32;
            let sells = sides.iter().filter(|s| **s == -1).count() as u32;
            let initial_base_qty = match cfg.side {
                Side::Long => q * f64::from(sells),
                Side::Short => q * f64::from(buys),
                Side::Neutral => 0.0,
            };
            let (stop_lower, stop_upper) = grid::stop_band(lower, upper, p.stop_out_pct, p.trailing_up);
            let w = grid_worst(cfg, p, &lv, price, q, costs.mmr);
            let rows = lv
                .iter()
                .zip(&sides)
                .enumerate()
                .map(|(i, (l, s))| GridLevelRow {
                    index: i as u16,
                    price: *l,
                    side: match s {
                        1 => "buy",
                        -1 => "sell",
                        _ => "none",
                    },
                    qty: q,
                    notional: q * l,
                })
                .collect();
            if p.stop_out_pct.is_none() {
                warnings.push("noStopOut");
            }
            // A relative range is validated scale-free, so its trail limit can
            // only be compared with the band once a price is known.
            if let (Some(limit), true, GridRange::Relative { .. }) = (p.trail_up_limit, p.trailing_up, p.range) {
                if limit <= upper {
                    warnings.push("trailLimitInsideRange");
                }
            }
            // An absolute range passes at its mid; started at this price the
            // driver would hold it until the liquidation leaves the band.
            if matches!(p.range, GridRange::Absolute { .. }) && !grid_liq_clear_at(cfg, p, price) {
                warnings.push("gridLiqInsideBandNow");
            }
            GridPreview {
                lower,
                upper,
                levels: rows,
                qty_per_level: q,
                profit_per_grid_min_pct: pmin,
                profit_per_grid_max_pct: pmax,
                buy_orders: buys,
                sell_orders: sells,
                initial_base_qty,
                initial_quote: initial_base_qty * price,
                stop_lower,
                stop_upper,
                liq_price_bottom: w.liq_bottom,
                liq_price_top: w.liq_top,
            }
            .into_dto(cfg, price, cfg.budget, warnings, error)
        }
    }
}

trait IntoDto {
    fn into_dto(
        self,
        cfg: &StrategyConfig,
        price: f64,
        required: f64,
        warnings: Vec<&'static str>,
        error: Option<StrategyError>,
    ) -> PreviewDto;
}

impl IntoDto for DcaPreview {
    fn into_dto(self, cfg: &StrategyConfig, price: f64, required: f64, warnings: Vec<&'static str>, error: Option<StrategyError>) -> PreviewDto {
        PreviewDto {
            kind: StrategyKind::Dca,
            reference_price: price,
            required_capital: required,
            costs: CostModel::for_market(cfg.market),
            dca: Some(self),
            grid: None,
            warnings,
            error,
        }
    }
}

impl IntoDto for GridPreview {
    fn into_dto(self, cfg: &StrategyConfig, price: f64, required: f64, warnings: Vec<&'static str>, error: Option<StrategyError>) -> PreviewDto {
        PreviewDto {
            kind: StrategyKind::Grid,
            reference_price: price,
            required_capital: required,
            costs: CostModel::for_market(cfg.market),
            dca: None,
            grid: Some(self),
            warnings,
            error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::strategy::model::{RestartPolicy, Spacing};

    pub(crate) fn dca_cfg() -> StrategyConfig {
        StrategyConfig {
            schema_version: 1,
            name: "DCA".into(),
            exchange_id: "binance".into(),
            market: MarketKind::Futures,
            symbol: "BTCUSDT".into(),
            side: Side::Long,
            budget: 1000.0,
            leverage: 1,
            start: StartCondition::Immediately,
            restart: RestartPolicy::default(),
            max_drawdown_pct: None,
            pause_on_btc_break: true,
            portfolio_breaker: true,
            params: StrategyParams::Dca(DcaParams {
                base_order: Some(100.0),
                safety_order: Some(100.0),
                base_weight: None,
                safety_weight: None,
                max_so: 2,
                so_step_pct: 2.0,
                step_scale: 1.0,
                volume_scale: 1.0,
                tp_pct: 2.0,
                trailing_pct: None,
                sl_pct: None,
                max_duration_min: None,
            }),
            preset_id: None,
        }
    }

    pub(crate) fn grid_cfg() -> StrategyConfig {
        StrategyConfig {
            side: Side::Neutral,
            params: StrategyParams::Grid(GridParams {
                range: GridRange::Absolute { lower: 90.0, upper: 110.0 },
                n_grids: 10,
                spacing: Spacing::Arith,
                stop_out_pct: Some(3.0),
                trailing_up: false,
                trail_up_limit: None,
                take_profit_pct: None,
                max_duration_min: None,
            }),
            ..dca_cfg()
        }
    }

    fn code(cfg: &StrategyConfig, lev: u8) -> Option<&'static str> {
        validate(cfg, lev).err().map(|e| e.code)
    }

    fn with_dca(f: impl FnOnce(&mut DcaParams)) -> StrategyConfig {
        let mut c = dca_cfg();
        if let StrategyParams::Dca(p) = &mut c.params {
            f(p);
        }
        c
    }

    fn with_grid(f: impl FnOnce(&mut GridParams)) -> StrategyConfig {
        let mut c = grid_cfg();
        if let StrategyParams::Grid(p) = &mut c.params {
            f(p);
        }
        c
    }

    // Owner's backtest 2 Oct: TP 1.5% with a 5% trail turned 17 target hits
    // into trail exits, 9 of them losses (-7.9% over 180 days).
    #[test]
    fn a_trail_at_or_beyond_the_take_profit_is_refused() {
        let tp = |c: &StrategyConfig| match &c.params {
            StrategyParams::Dca(p) => p.tp_pct,
            _ => unreachable!(),
        };
        let base = dca_cfg();
        let at = tp(&base);
        assert_eq!(code(&with_dca(|p| p.trailing_pct = Some(at)), 2), Some("trailingAboveTp"));
        assert_eq!(code(&with_dca(|p| p.trailing_pct = Some(at * 3.0)), 2), Some("trailingAboveTp"));
        assert_eq!(code(&with_dca(|p| p.trailing_pct = Some(at / 2.0)), 2), None);
    }

    #[test]
    fn a_stop_that_fires_before_the_last_safety_order_is_refused() {
        let coverage = |c: &StrategyConfig| match &c.params {
            StrategyParams::Dca(p) => *dca::deviations(p).last().unwrap(),
            _ => unreachable!(),
        };
        let cov = coverage(&dca_cfg());
        // A stop shallower than the first safety order fires before it fills.
        assert_eq!(code(&with_dca(|p| p.sl_pct = Some(0.5)), 2), Some("slInsideLadder"));
        assert_eq!(code(&with_dca(|p| p.sl_pct = Some(cov * 0.6)), 2), Some("slInsideLadder"));
        // The form's default when the stop is switched on: coverage + 5.
        assert_eq!(code(&with_dca(|p| p.sl_pct = Some((cov + 5.0).min(90.0))), 2), None);
        let short = {
            let mut c = with_dca(|p| p.sl_pct = Some(0.5));
            c.side = Side::Short;
            c
        };
        assert_eq!(code(&short, 2), Some("slInsideLadder"));
    }

    #[test]
    fn restart_errors_point_at_their_own_field() {
        let mut c = dca_cfg();
        c.restart.max_cycles = Some(0);
        assert_eq!(validate(&c, 2).err().and_then(|e| e.field), Some("maxCycles"));
        let mut c = dca_cfg();
        c.restart.price_band = Some((10.0, 5.0));
        assert_eq!(validate(&c, 2).err().and_then(|e| e.field), Some("priceBand"));
    }

    #[test]
    fn valid_defaults_pass() {
        assert_eq!(code(&dca_cfg(), 2), None);
        assert_eq!(code(&grid_cfg(), 2), None);
    }

    #[test]
    fn every_error_code() {
        let mut c = dca_cfg();
        c.side = Side::Neutral;
        assert_eq!(code(&c, 2), Some("neutralDcaRefused"));
        let mut c = dca_cfg();
        c.market = MarketKind::Spot;
        c.side = Side::Short;
        assert_eq!(code(&c, 2), Some("spotShortRefused"));
        let mut c = dca_cfg();
        c.market = MarketKind::Spot;
        c.leverage = 2;
        assert_eq!(code(&c, 5), Some("spotLeverage"));
        let mut c = dca_cfg();
        c.leverage = 3;
        assert_eq!(code(&c, 2), Some("leverageAboveCeiling"));
        assert_eq!(code(&c, 5), None);
        let mut c = dca_cfg();
        c.name = "x".repeat(41);
        assert_eq!(code(&c, 2), Some("nameInvalid"));
        let mut c = dca_cfg();
        c.exchange_id = "okx".into();
        assert_eq!(code(&c, 2), Some("exchangeUnsupported"));
        let mut c = dca_cfg();
        c.symbol = "btcusdt".into();
        assert_eq!(code(&c, 2), Some("symbolInvalid"));
        assert_eq!(code(&with_dca(|p| p.base_weight = Some(1.0)), 2), Some("orderFormExclusive"));
        assert_eq!(code(&with_dca(|p| p.max_so = 26), 2), Some("maxSoAboveCap"));
        assert_eq!(code(&with_dca(|p| p.step_scale = 0.9), 2), Some("stepScaleInvalid"));
        assert_eq!(code(&with_dca(|p| p.volume_scale = 0.5), 2), Some("volumeScaleInvalid"));
        assert_eq!(code(&with_dca(|p| p.tp_pct = 0.0), 2), Some("tpInvalid"));
        assert_eq!(code(&with_dca(|p| p.so_step_pct = 60.0), 2), Some("ladderBelowZero"));
        assert_eq!(code(&with_dca(|p| p.base_order = Some(2000.0)), 2), Some("ladderExceedsBudget"));
        assert_eq!(code(&with_dca(|p| p.base_order = Some(4.0)), 2), Some("budgetBelowMinNotional"));
        let mut c = with_dca(|p| {
            p.base_order = Some(1000.0);
            p.safety_order = Some(1000.0);
            p.so_step_pct = 30.0;
        });
        c.leverage = 5;
        assert_eq!(code(&c, 5), Some("ladderBeyondLiquidation"));
        assert_eq!(code(&with_grid(|p| p.n_grids = 101), 2), Some("gridLevelsInvalid"));
        assert_eq!(code(&with_grid(|p| p.n_grids = 1), 2), Some("gridLevelsInvalid"));
        assert_eq!(
            code(&with_grid(|p| p.range = GridRange::Absolute { lower: 110.0, upper: 90.0 }), 2),
            Some("gridRangeInvalid")
        );
        assert_eq!(code(&with_grid(|p| p.trailing_up = true), 2), Some("trailingUpLongOnly"));
        assert_eq!(
            code(&with_grid(|p| {
                p.range = GridRange::Absolute { lower: 99.0, upper: 101.0 };
                p.n_grids = 100;
            }), 2),
            Some("gridStepBelowFees")
        );
        let mut c = grid_cfg();
        c.budget = 40.0;
        assert_eq!(code(&c, 2), Some("budgetBelowMinNotional"));
        let mut c = grid_cfg();
        c.leverage = 20;
        c.side = Side::Long;
        assert_eq!(code(&c, 20), Some("gridLiqInsideBand"));
    }

    #[test]
    fn a_short_take_profit_at_a_zero_price_is_refused() {
        let mut c = with_dca(|p| p.tp_pct = 100.0);
        c.side = Side::Short;
        let StrategyParams::Dca(p) = &c.params else { unreachable!() };
        assert_eq!(dca::tp_price(100.0, p.tp_pct, -1.0), 0.0, "the TP can never fill");
        assert_eq!(code(&c, 2), Some("tpInvalid"));
        c.side = Side::Long;
        assert_eq!(code(&c, 2), None, "a long's 100% TP is a real price");
    }

    #[test]
    fn preview_warns_where_a_grid_only_fails_at_the_real_price() {
        let mut c = grid_cfg();
        c.side = Side::Long;
        c.leverage = 10;
        assert_eq!(code(&c, 10), None, "passes at the range mid");
        assert!(preview(&c, 109.0, 10).warnings.contains(&"gridLiqInsideBandNow"));
        assert!(!preview(&c, 99.5, 10).warnings.contains(&"gridLiqInsideBandNow"));

        let mut c = with_grid(|p| {
            p.range = GridRange::Relative { lower_pct: 10.0, upper_pct: 10.0 };
            p.trailing_up = true;
            p.trail_up_limit = Some(105.0);
        });
        c.side = Side::Long;
        assert_eq!(code(&c, 2), None);
        assert!(preview(&c, 100.0, 2).warnings.contains(&"trailLimitInsideRange"));
        assert!(!preview(&c, 50.0, 2).warnings.contains(&"trailLimitInsideRange"));
    }

    #[test]
    fn preview_matches_hand_computed_values() {
        let pv = preview(&dca_cfg(), 100.0, 2);
        assert!(pv.error.is_none());
        assert_eq!(pv.costs, crate::bot::strategy::costs::FUTURES_SIM, "the form shows the costs it is charged");
        assert!((pv.required_capital - 300.0).abs() < 1e-12);
        let d = pv.dca.unwrap();
        assert_eq!(d.rungs.len(), 3);
        assert!((d.last_so_price.unwrap() - 96.0).abs() < 1e-12);
        assert!((d.max_coverage_pct - 4.0).abs() < 1e-12);
        assert_eq!(d.liq_price, None);
        assert_eq!(d.worst_loss_basis, "none");
        assert!(pv.warnings.contains(&"noStopLoss"));
        let sum: f64 = d.rungs.iter().map(|r| r.notional).sum();
        assert!((sum - d.total_notional).abs() < 1e-9);

        let pv = preview(&grid_cfg(), 100.0, 2);
        let g = pv.grid.unwrap();
        assert_eq!(g.levels.len(), 11);
        assert_eq!(g.buy_orders, 5);
        assert_eq!(g.sell_orders, 5);
        let want_min = (110.0 / 108.0 - 1.0 - 0.0004) * 100.0;
        let want_max = (92.0 / 90.0 - 1.0 - 0.0004) * 100.0;
        assert!((g.profit_per_grid_min_pct - want_min).abs() < 1e-9);
        assert!((g.profit_per_grid_max_pct - want_max).abs() < 1e-9);
        assert_eq!(g.initial_base_qty, 0.0);
        assert!((pv.required_capital - 1000.0).abs() < 1e-12);
    }

    #[test]
    fn an_early_rung_liquidating_before_the_next_safety_order_is_refused() {
        // 5x long, base 1000 + one SO 1000 at -25%. The full ladder's liq
        // (~68.9) sits below the SO (75), but the base alone liquidates at
        // ~80.4: the price never reaches the SO.
        let mk = |side: Side| {
            let mut c = with_dca(|p| {
                p.base_order = Some(1000.0);
                p.safety_order = Some(1000.0);
                p.max_so = 1;
                p.so_step_pct = 25.0;
            });
            c.side = side;
            c.leverage = 5;
            c
        };
        let long = mk(Side::Long);
        let rungs = match &long.params {
            StrategyParams::Dca(p) => dca::build_ladder(p, 100.0, 1.0, 1000.0, 5.0, 0.0002, 0.005),
            _ => unreachable!(),
        };
        assert!(rungs[1].price > rungs[1].liq_price.unwrap() * 1.005, "full-ladder check alone passes");
        assert!(rungs[1].price < rungs[0].liq_price.unwrap(), "base liquidates first");
        assert_eq!(code(&long, 5), Some("ladderBeyondLiquidation"));
        assert_eq!(code(&mk(Side::Short), 5), Some("ladderBeyondLiquidation"));
        // the same ladder at 2x is reachable
        let mut ok = mk(Side::Long);
        ok.leverage = 2;
        assert_eq!(code(&ok, 5), None);
    }

    #[test]
    fn preview_shows_the_short_ladder_liquidation() {
        let mut c = dca_cfg();
        c.side = Side::Short;
        c.leverage = 2;
        let d = preview(&c, 100.0, 2).dca.unwrap();
        let liq = d.liq_price.unwrap();
        assert!(liq > 104.0, "{liq}");
        assert_eq!(d.worst_loss_basis, "liq");
        assert!(d.liq_distance_pct.unwrap() > 0.0);
    }
}
