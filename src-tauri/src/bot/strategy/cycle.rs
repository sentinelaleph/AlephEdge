//! Pure cycle state shared by DCA and Grid: cash, fees, funding, the event
//! list, and the bar step that drives `path::walk_bar`. One cycle = one DCA
//! deal or one grid run, from its start bar open to its exit.
//!
//! Money semantics are the research simulator's: `cash` is the cycle's net
//! P&L so far (realised P&L minus fees minus funding paid); a cycle's result
//! in % of budget is `cash / budget * 100`.

use serde::{Deserialize, Serialize};

use super::costs::CostModel;
use super::dca::{self, DcaState};
use super::grid::{self, GridState};
use super::model::{
    Bar, BotId, ExitReason, Fill, FillKind, Liquidity, Side, SimOrder, StrategyKind,
};
use super::path::{walk_bar, Walker};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CycleCore {
    pub bot_id: BotId,
    pub seq: u32,
    pub kind: StrategyKind,
    pub side: Side,
    pub opened_at: u64,
    pub anchor_price: f64,
    /// accounting::size_down at cycle start (never > 1).
    pub size_factor: f64,
    /// Budget x size_factor: the margin pool of this cycle.
    pub budget: f64,
    pub leverage: f64,
    pub costs: CostModel,
    /// Net P&L so far: realised - fees - funding paid.
    pub cash: f64,
    pub fees: f64,
    /// Funding paid (positive) or received (negative).
    pub funding: f64,
    pub fills: u32,
    pub max_margin: f64,
    /// Peak position size (quote notional at entry prices).
    pub max_notional: f64,
    /// Worst bar-close MTM (cash + unrealised), quote.
    pub max_adverse_quote: f64,
    pub open: bool,
    pub exit: Option<ExitReason>,
    /// Legs walked over the cycle's life (causality index).
    pub legs: u32,
    /// Event time stamped on new fills (bar open while walking).
    pub now_ms: u64,
    /// Bar open of the bar being walked.
    pub bar_open_ms: u64,
    pub deadline_ms: Option<u64>,
    pub prev_close: Option<f64>,
    pub last_bar_close_ms: u64,
    /// A funding bar's rate could not be read (charged 0, flagged).
    pub funding_unknown: bool,
    /// Lowest cycle MTM (cash + unrealised, quote) the bot's drawdown stop
    /// allows, set by the driver before each bar. The DCA walk stops at the
    /// price that reaches it inside the bar, as a resting stop would, rather
    /// than at the bar close (a 4h close overshot the stop by 4.4% of budget).
    #[serde(default, skip)]
    pub dd_floor: Option<f64>,
    /// Events not yet persisted.
    #[serde(default)]
    pub events: Vec<Fill>,
}

impl CycleCore {
    /// "ae" + first 8 id chars after the "sb_" prefix + "-" + seq + "-".
    pub fn client_prefix(&self) -> String {
        client_prefix(&self.bot_id, self.seq)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn push_fill(
        &mut self,
        role: &str,
        price: f64,
        qty: f64,
        liquidity: Liquidity,
        fee: f64,
        realized: f64,
        kind: FillKind,
    ) {
        let client_id = format!("{}{role}", self.client_prefix());
        self.events.push(Fill {
            client_id,
            ts: self.now_ms,
            price,
            qty,
            liquidity,
            fee_quote: fee,
            realized_quote: realized,
            bar_open_ms: self.bar_open_ms,
            leg: self.legs,
            kind,
        });
    }

    pub fn close_as(&mut self, reason: ExitReason) {
        self.open = false;
        self.exit = Some(reason);
    }
}

pub fn client_prefix(bot_id: &str, seq: u32) -> String {
    let short: String = bot_id
        .strip_prefix("sb_")
        .unwrap_or(bot_id)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    format!("ae{short}-{seq}-")
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "engine", rename_all = "camelCase")]
pub enum Engine {
    Dca(DcaState),
    Grid(GridState),
}

/// One open (or just closed) cycle. Serialized whole into
/// `strategy_cycles.state_json` while open.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CycleState {
    pub core: CycleCore,
    pub engine: Engine,
}

/// What a new cycle needs from its bot.
#[derive(Clone, Debug)]
pub struct CycleStart<'a> {
    pub bot_id: &'a str,
    pub seq: u32,
    pub side: Side,
    pub budget: f64,
    pub leverage: f64,
    pub size_factor: f64,
    pub costs: CostModel,
    pub max_duration_min: Option<u32>,
}

impl CycleState {
    fn new_core(st: &CycleStart, kind: StrategyKind, bar: &Bar) -> CycleCore {
        CycleCore {
            bot_id: st.bot_id.to_string(),
            seq: st.seq,
            kind,
            side: st.side,
            opened_at: bar.open_ms,
            anchor_price: bar.o,
            size_factor: st.size_factor,
            budget: st.budget * st.size_factor,
            leverage: st.leverage,
            costs: st.costs,
            cash: 0.0,
            fees: 0.0,
            funding: 0.0,
            fills: 0,
            max_margin: 0.0,
            max_notional: 0.0,
            max_adverse_quote: 0.0,
            open: true,
            exit: None,
            legs: 0,
            now_ms: bar.open_ms,
            bar_open_ms: bar.open_ms,
            deadline_ms: st
                .max_duration_min
                .map(|m| bar.open_ms + u64::from(m) * 60_000),
            prev_close: None,
            last_bar_close_ms: 0,
            funding_unknown: false,
            dd_floor: None,
            events: Vec::new(),
        }
    }

    /// Opens a DCA deal at `bar.o` (base order at market).
    pub fn open_dca(
        st: &CycleStart,
        params: &super::model::DcaParams,
        bar: &Bar,
    ) -> Result<CycleState, &'static str> {
        let mut core = Self::new_core(st, StrategyKind::Dca, bar);
        let d = dca::open(&mut core, params, bar.o)?;
        Ok(CycleState {
            core,
            engine: Engine::Dca(d),
        })
    }

    /// Opens a grid at `bar.o`. Err("gridStartOutsideBand") when the open is
    /// already beyond the stop band.
    pub fn open_grid(
        st: &CycleStart,
        params: &super::model::GridParams,
        bar: &Bar,
    ) -> Result<CycleState, &'static str> {
        let mut core = Self::new_core(st, StrategyKind::Grid, bar);
        let g = grid::open(&mut core, params, bar.o)?;
        Ok(CycleState {
            core,
            engine: Engine::Grid(g),
        })
    }

    pub fn is_open(&self) -> bool {
        self.core.open
    }

    pub fn unreal(&self, px: f64) -> f64 {
        match &self.engine {
            Engine::Dca(d) => d.unreal(px),
            Engine::Grid(g) => g.unreal(px),
        }
    }

    /// Mark-to-market net P&L of the cycle at `px`.
    pub fn mtm(&self, px: f64) -> f64 {
        self.core.cash + if self.core.open { self.unreal(px) } else { 0.0 }
    }

    /// Margin in the position now.
    pub fn margin(&self) -> f64 {
        match &self.engine {
            Engine::Dca(d) => d.m,
            Engine::Grid(g) => g.margin(self.core.leverage),
        }
    }

    /// Capital reserved by this cycle (full DCA ladder margin, whole grid budget).
    pub fn committed(&self) -> f64 {
        match &self.engine {
            Engine::Dca(d) => d.committed,
            Engine::Grid(_) => self.core.budget,
        }
    }

    pub fn signed_qty(&self) -> f64 {
        match &self.engine {
            Engine::Dca(d) => d.s * d.q,
            Engine::Grid(g) => g.pos,
        }
    }

    /// Position notional at average entry (quote).
    pub fn notional(&self) -> f64 {
        match &self.engine {
            Engine::Dca(d) => d.n,
            Engine::Grid(g) => g.pos.abs() * g.a,
        }
    }

    pub fn avg_entry(&self) -> Option<f64> {
        match &self.engine {
            Engine::Dca(d) => (d.q > 0.0).then_some(d.a),
            Engine::Grid(g) => (g.pos != 0.0).then_some(g.a),
        }
    }

    pub fn liq_price(&self) -> Option<f64> {
        match &self.engine {
            Engine::Dca(d) => d.liq,
            Engine::Grid(g) => g.liq,
        }
    }

    pub fn so_filled(&self) -> Option<u8> {
        match &self.engine {
            Engine::Dca(d) => Some(d.nso),
            Engine::Grid(_) => None,
        }
    }

    pub fn grid_closing_fills(&self) -> Option<u32> {
        match &self.engine {
            Engine::Dca(_) => None,
            Engine::Grid(g) => Some(g.closes),
        }
    }

    /// Market close at `ref_px` (taker + slippage) with `reason`.
    pub fn close_market(&mut self, ref_px: f64, reason: ExitReason) {
        if !self.core.open {
            return;
        }
        match &mut self.engine {
            Engine::Dca(d) => dca::close_market(&mut self.core, d, ref_px, reason),
            Engine::Grid(g) => grid::close_market(&mut self.core, g, ref_px, reason),
        }
    }

    /// The cycle's resting orders, derived from its state.
    pub fn open_orders(&self) -> Vec<SimOrder> {
        if !self.core.open {
            return Vec::new();
        }
        match &self.engine {
            Engine::Dca(d) => d.open_orders(&self.core),
            Engine::Grid(g) => g.open_orders(&self.core),
        }
    }

    /// Takes the events not yet persisted.
    pub fn drain_events(&mut self) -> Vec<Fill> {
        std::mem::take(&mut self.core.events)
    }

    fn track_extremes(&mut self) {
        let m = self.margin();
        let n = self.notional();
        if m > self.core.max_margin {
            self.core.max_margin = m;
        }
        if n > self.core.max_notional {
            self.core.max_notional = n;
        }
    }
}

impl Walker for CycleState {
    fn exposure(&self) -> i8 {
        match &self.engine {
            Engine::Dca(d) => d.s as i8,
            Engine::Grid(g) => g.exposure(),
        }
    }

    fn can_protect(&self) -> bool {
        match &self.engine {
            Engine::Dca(d) => d.sl.is_some() || d.liq.is_some(),
            Engine::Grid(_) => true,
        }
    }

    fn is_open(&self) -> bool {
        self.core.open
    }

    fn closed_protectively(&self) -> bool {
        self.core.exit.is_some_and(|e| e.is_protective())
    }

    fn seg(&mut self, from: f64, to: f64, gap: bool) {
        self.core.legs += 1;
        match &mut self.engine {
            Engine::Dca(d) => dca::seg(&mut self.core, d, from, to, gap),
            Engine::Grid(g) => grid::seg(&mut self.core, g, from, to, gap),
        }
    }

    fn funding(&mut self, bar: &Bar, open: f64) {
        if !self.core.costs.funding {
            return;
        }
        if bar.funding_unknown {
            self.core.funding_unknown = true;
        }
        let Some(rate) = bar.funding_rate else {
            return;
        };
        if rate == 0.0 {
            return;
        }
        let f = super::accounting::funding(self.signed_qty(), open, rate);
        // A flat position (a neutral grid between round trips) pays nothing:
        // no zero-amount funding row.
        if f == 0.0 {
            return;
        }
        self.core.cash -= f;
        self.core.funding += f;
        let qty = self.signed_qty();
        self.core
            .push_fill("fund", open, qty, Liquidity::Taker, f, 0.0, FillKind::Funding);
        // Like the simulator, the grid liquidation price is refreshed on the
        // next trade, not on a funding charge.
    }
}

/// Walks one closed bar through an open cycle: the intrabar walk, then the
/// bar-close checks (grid take profit, max duration). `first` = the cycle's
/// start bar (no gap leg). Returns the events of this bar.
pub fn step_bar(c: &mut CycleState, bar: &Bar, first: bool) -> Vec<Fill> {
    if !c.core.open {
        return Vec::new();
    }
    c.core.now_ms = bar.open_ms;
    c.core.bar_open_ms = bar.open_ms;
    let prev = if first { None } else { c.core.prev_close };
    walk_bar(c, bar, prev);
    c.track_extremes();
    c.core.now_ms = bar.close_ms;
    if c.core.open {
        if let Engine::Grid(g) = &c.engine {
            if let Some(tp) = g.tp_total {
                if c.core.cash + g.unreal(bar.c) >= c.core.budget * tp / 100.0 {
                    c.close_market(bar.c, ExitReason::GridTp);
                }
            }
        }
    }
    if c.core.open {
        if let Some(deadline) = c.core.deadline_ms {
            if bar.end_ms() >= deadline {
                c.close_market(bar.c, ExitReason::Timeout);
            }
        }
    }
    if c.core.open {
        let mtm = c.mtm(bar.c);
        if mtm < c.core.max_adverse_quote {
            c.core.max_adverse_quote = mtm;
        }
    } else if c.core.cash < c.core.max_adverse_quote {
        c.core.max_adverse_quote = c.core.cash;
    }
    c.core.prev_close = Some(bar.c);
    c.core.last_bar_close_ms = bar.close_ms;
    c.drain_events()
}
