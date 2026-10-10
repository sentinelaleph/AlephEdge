//! Pure serde types for the DCA and Grid strategy bots (paper only in v1).
//!
//! No I/O, no Tauri, and no `crate::` path outside `bot::strategy`, so a
//! backtest or the headless runner can `#[path]`-mount this file unchanged.
//!
//! DCA and Grid are deliberately NOT `BotKind` variants: `BotKind` is mounted
//! by the paper runner and mirrored in the signed phone protocol. Strategy
//! bots are separate, multi-instance objects keyed by their own `BotId`.

use serde::{Deserialize, Serialize};

/// "sb_" + 12 lowercase base32 characters, generated once, never reused.
pub type BotId = String;

/// Config schema version written into every stored config.
pub const CONFIG_SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "lowercase")]
pub enum StrategyKind {
    Dca,
    Grid,
}

impl StrategyKind {
    pub fn as_str(self) -> &'static str {
        match self {
            StrategyKind::Dca => "dca",
            StrategyKind::Grid => "grid",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Long,
    Short,
    /// Grid only (validate refuses a neutral DCA).
    Neutral,
}

impl Side {
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Long => "long",
            Side::Short => "short",
            Side::Neutral => "neutral",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
#[serde(rename_all = "lowercase")]
pub enum MarketKind {
    /// USDT-M perpetual, isolated margin.
    Futures,
    /// Long only, leverage 1, no funding.
    Spot,
}

impl MarketKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MarketKind::Futures => "futures",
            MarketKind::Spot => "spot",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Spacing {
    Arith,
    Geom,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum CrossDir {
    Up,
    Down,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RestartPolicy {
    /// Minutes between a cycle's end (bar close) and the next start decision.
    #[serde(default)]
    pub cooldown_min: u32,
    /// `None` = unlimited.
    #[serde(default)]
    pub max_cycles: Option<u32>,
    #[serde(default = "yes")]
    pub after_stop: bool,
    /// Default false: a liquidated bot is dead.
    #[serde(default)]
    pub after_liquidation: bool,
    /// No new cycle while the start bar's open is outside `[min, max]`.
    #[serde(default)]
    pub price_band: Option<(f64, f64)>,
    /// No new cycle at or after this time.
    #[serde(default)]
    pub end_at_ms: Option<u64>,
}

fn yes() -> bool {
    true
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            cooldown_min: 0,
            max_cycles: None,
            after_stop: true,
            after_liquidation: false,
            price_band: None,
            end_at_ms: None,
        }
    }
}

/// When the first cycle may open. A signal-based start (Sentinel) is
/// deliberately not offered in v1.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StartCondition {
    /// The first bar that opens after Start.
    Immediately,
    /// The bar after the first closed bar whose range crosses `price`.
    PriceCross { price: f64, direction: CrossDir },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DcaParams {
    /// Base order notional (quote). Exclusive with the weight form.
    #[serde(default)]
    pub base_order: Option<f64>,
    #[serde(default)]
    pub safety_order: Option<f64>,
    /// Weight form: the ladder is scaled so its total margin == budget.
    #[serde(default)]
    pub base_weight: Option<f64>,
    #[serde(default)]
    pub safety_weight: Option<f64>,
    pub max_so: u8,
    /// Deviation of SO1 from the anchor (start bar open), %.
    pub so_step_pct: f64,
    pub step_scale: f64,
    pub volume_scale: f64,
    /// Take profit from the average entry, %, maker limit.
    pub tp_pct: f64,
    #[serde(default)]
    pub trailing_pct: Option<f64>,
    /// Stop loss from the average entry, %, market.
    #[serde(default)]
    pub sl_pct: Option<f64>,
    #[serde(default)]
    pub max_duration_min: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum GridRange {
    Absolute { lower: f64, upper: f64 },
    #[serde(rename_all = "camelCase")]
    Relative { lower_pct: f64, upper_pct: f64 },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GridParams {
    pub range: GridRange,
    /// Intervals (levels = n_grids + 1).
    pub n_grids: u16,
    pub spacing: Spacing,
    /// Stop when price trades this % beyond the range.
    #[serde(default)]
    pub stop_out_pct: Option<f64>,
    /// Long only; disables the upper stop.
    #[serde(default)]
    pub trailing_up: bool,
    #[serde(default)]
    pub trail_up_limit: Option<f64>,
    /// % of budget, checked on bar-close MTM.
    #[serde(default)]
    pub take_profit_pct: Option<f64>,
    #[serde(default)]
    pub max_duration_min: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StrategyParams {
    Dca(DcaParams),
    Grid(GridParams),
}

impl StrategyParams {
    pub fn kind(&self) -> StrategyKind {
        match self {
            StrategyParams::Dca(_) => StrategyKind::Dca,
            StrategyParams::Grid(_) => StrategyKind::Grid,
        }
    }

    pub fn max_duration_min(&self) -> Option<u32> {
        match self {
            StrategyParams::Dca(p) => p.max_duration_min,
            StrategyParams::Grid(p) => p.max_duration_min,
        }
    }
}

/// A strategy bot's full settings. There is NO `live` field in v1: a strategy
/// bot cannot even express a real-money intent.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StrategyConfig {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    /// 1..=40 characters.
    pub name: String,
    /// "binance" only in v1 (the kline provider is Binance-only).
    pub exchange_id: String,
    pub market: MarketKind,
    pub symbol: String,
    pub side: Side,
    /// Quote currency, paper.
    pub budget: f64,
    /// Clamped to the risk level's ceiling; spot = 1.
    pub leverage: u8,
    pub start: StartCondition,
    #[serde(default)]
    pub restart: RestartPolicy,
    /// Per-bot MTM drawdown stop, % of budget.
    #[serde(default)]
    pub max_drawdown_pct: Option<f64>,
    /// Blocks NEW cycles while Sentinel declares a BTC break.
    #[serde(default = "yes")]
    pub pause_on_btc_break: bool,
    /// Inside the strategy portfolio breaker: counted toward its P&L and
    /// budget, closed when it trips, held while it is tripped. Off = the
    /// breaker neither counts, closes nor holds this bot. Default true, so
    /// configs saved before the field keep the breaker.
    #[serde(default = "yes")]
    pub portfolio_breaker: bool,
    pub params: StrategyParams,
    /// Provenance when created from a research preset.
    #[serde(default)]
    pub preset_id: Option<String>,
}

fn schema_version() -> u32 {
    CONFIG_SCHEMA_VERSION
}

impl StrategyConfig {
    pub fn kind(&self) -> StrategyKind {
        self.params.kind()
    }
}

/// One closed bar (OHLC) of the execution feed. `funding_rate` is the funding
/// rate (fraction, not %) of a funding timestamp falling inside this bar;
/// `funding_unknown` marks a funding bar whose rate could not be read (it is
/// then charged 0 and flagged, never invented).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Bar {
    pub open_ms: u64,
    /// Exchange convention: open + interval - 1.
    pub close_ms: u64,
    pub o: f64,
    pub h: f64,
    pub l: f64,
    pub c: f64,
    #[serde(default)]
    pub funding_rate: Option<f64>,
    #[serde(default)]
    pub funding_unknown: bool,
}

impl Bar {
    /// First millisecond after this bar (= next bar's open).
    pub fn end_ms(&self) -> u64 {
        self.close_ms + 1
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum BotRunState {
    Stopped,
    /// Waiting for the start condition, a cooldown or the price band.
    Armed,
    InCycle,
    /// Price feed stale (fills and new cycles held).
    Paused,
    Dead,
}

impl BotRunState {
    pub fn as_str(self) -> &'static str {
        match self {
            BotRunState::Stopped => "stopped",
            BotRunState::Armed => "armed",
            BotRunState::InCycle => "inCycle",
            BotRunState::Paused => "paused",
            BotRunState::Dead => "dead",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "stopped" => BotRunState::Stopped,
            "armed" => BotRunState::Armed,
            "inCycle" => BotRunState::InCycle,
            "paused" => BotRunState::Paused,
            "dead" => BotRunState::Dead,
            _ => return None,
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum ExitReason {
    Tp,
    Trail,
    Sl,
    Liq,
    /// Grid stop-out.
    Stop,
    GridTp,
    Timeout,
    Ddstop,
    PortfolioDd,
    DailyStop,
    RemoteKill,
    Manual,
    /// Data end (research replay only).
    End,
    /// Never a cycle exit: why a bot died after losses shrank its orders
    /// below 1% of the budget or below the exchange minimum order.
    SizeDown,
}

impl ExitReason {
    pub fn as_str(self) -> &'static str {
        match self {
            ExitReason::Tp => "tp",
            ExitReason::Trail => "trail",
            ExitReason::Sl => "sl",
            ExitReason::Liq => "liq",
            ExitReason::Stop => "stop",
            ExitReason::GridTp => "gridTp",
            ExitReason::Timeout => "timeout",
            ExitReason::Ddstop => "ddstop",
            ExitReason::PortfolioDd => "portfolioDd",
            ExitReason::DailyStop => "dailyStop",
            ExitReason::RemoteKill => "remoteKill",
            ExitReason::Manual => "manual",
            ExitReason::End => "end",
            ExitReason::SizeDown => "sizeDown",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            ExitReason::Tp,
            ExitReason::Trail,
            ExitReason::Sl,
            ExitReason::Liq,
            ExitReason::Stop,
            ExitReason::GridTp,
            ExitReason::Timeout,
            ExitReason::Ddstop,
            ExitReason::PortfolioDd,
            ExitReason::DailyStop,
            ExitReason::RemoteKill,
            ExitReason::Manual,
            ExitReason::End,
            ExitReason::SizeDown,
        ]
        .into_iter()
        .find(|r| r.as_str() == s)
    }

    /// Protective exits win the intrabar tie (botsim rule 4). The per-bot
    /// drawdown stop is one too: it rests as a stop level inside the bar
    /// (and on the exchange), so a bar touching it and the take profit
    /// books the stop, adverse first, like a stop loss.
    pub fn is_protective(self) -> bool {
        matches!(self, ExitReason::Sl | ExitReason::Stop | ExitReason::Liq | ExitReason::Ddstop)
    }

    /// Stops that `RestartPolicy::after_stop` governs.
    pub fn is_stop(self) -> bool {
        matches!(self, ExitReason::Sl | ExitReason::Stop)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Liquidity {
    Maker,
    Taker,
}

impl Liquidity {
    pub fn as_str(self) -> &'static str {
        match self {
            Liquidity::Maker => "maker",
            Liquidity::Taker => "taker",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum OrderType {
    Limit,
    Market,
    StopMarket,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(tag = "role", content = "index", rename_all = "camelCase")]
pub enum OrderRole {
    Base,
    Safety(u8),
    TakeProfit,
    StopLoss,
    GridBuy(u16),
    GridSell(u16),
    Close,
}

impl OrderSide {
    pub fn as_str(self) -> &'static str {
        match self {
            OrderSide::Buy => "buy",
            OrderSide::Sell => "sell",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "buy" => Some(OrderSide::Buy),
            "sell" => Some(OrderSide::Sell),
            _ => None,
        }
    }
}

impl OrderType {
    pub fn as_str(self) -> &'static str {
        match self {
            OrderType::Limit => "limit",
            OrderType::Market => "market",
            OrderType::StopMarket => "stopMarket",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "limit" => Some(OrderType::Limit),
            "market" => Some(OrderType::Market),
            "stopMarket" => Some(OrderType::StopMarket),
            _ => None,
        }
    }
}

impl OrderRole {
    pub fn parse(s: &str) -> Option<Self> {
        let num = |p: &str| s.strip_prefix(p).and_then(|n| n.parse::<u16>().ok());
        Some(match s {
            "base" => OrderRole::Base,
            "tp" => OrderRole::TakeProfit,
            "sl" => OrderRole::StopLoss,
            "close" => OrderRole::Close,
            _ if s.starts_with("so") => OrderRole::Safety(u8::try_from(num("so")?).ok()?),
            _ if s.starts_with("gb") => OrderRole::GridBuy(num("gb")?),
            _ if s.starts_with("gs") => OrderRole::GridSell(num("gs")?),
            _ => return None,
        })
    }

    pub fn as_str(self) -> String {
        match self {
            OrderRole::Base => "base".into(),
            OrderRole::Safety(i) => format!("so{i}"),
            OrderRole::TakeProfit => "tp".into(),
            OrderRole::StopLoss => "sl".into(),
            OrderRole::GridBuy(i) => format!("gb{i}"),
            OrderRole::GridSell(i) => format!("gs{i}"),
            OrderRole::Close => "close".into(),
        }
    }
}

/// Paper uses Planned -> Open -> Filled | Canceled only; the rest exist for
/// the later live lifecycle.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum OrderState {
    Planned,
    Submitted,
    Open,
    PartiallyFilled,
    Filled,
    Canceled,
    Rejected,
    Expired,
}

impl OrderState {
    pub fn as_str(self) -> &'static str {
        match self {
            OrderState::Planned => "planned",
            OrderState::Submitted => "submitted",
            OrderState::Open => "open",
            OrderState::PartiallyFilled => "partiallyFilled",
            OrderState::Filled => "filled",
            OrderState::Canceled => "canceled",
            OrderState::Rejected => "rejected",
            OrderState::Expired => "expired",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            OrderState::Planned,
            OrderState::Submitted,
            OrderState::Open,
            OrderState::PartiallyFilled,
            OrderState::Filled,
            OrderState::Canceled,
            OrderState::Rejected,
            OrderState::Expired,
        ]
        .into_iter()
        .find(|o| o.as_str() == s)
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            OrderState::Filled | OrderState::Canceled | OrderState::Rejected | OrderState::Expired
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SimOrder {
    /// "ae" + bot short (8) + "-" + seq + "-" + role (<= 36 chars, the
    /// Binance newClientOrderId rule) — deterministic, so a later live retry
    /// is idempotent.
    pub client_id: String,
    pub role: OrderRole,
    pub side: OrderSide,
    pub kind: OrderType,
    pub price: f64,
    pub qty: f64,
    /// Created by a fill on leg k => active from leg k+1 only.
    pub active_from_leg: u32,
    pub state: OrderState,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum FillKind {
    Fill,
    Funding,
    Liquidation,
}

impl FillKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FillKind::Fill => "fill",
            FillKind::Funding => "funding",
            FillKind::Liquidation => "liquidation",
        }
    }
}

/// One accounting event of a cycle. `qty` is signed by the trade direction
/// (buy +, sell -) for fills; funding rows carry the charged amount in
/// `fee_quote` (positive = paid).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Fill {
    pub client_id: String,
    pub ts: u64,
    pub price: f64,
    pub qty: f64,
    pub liquidity: Liquidity,
    pub fee_quote: f64,
    pub realized_quote: f64,
    pub bar_open_ms: u64,
    pub leg: u32,
    pub kind: FillKind,
}

/// One note for the strategy feed (i18n key under `strategy.notes.*`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StrategyNote {
    pub at_ms: u64,
    pub bot_id: String,
    pub symbol: String,
    pub key: String,
    #[serde(default)]
    pub detail: Option<String>,
}

/// A strategy bot: config plus lifetime accounting (simple P&L, % of budget).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StrategyBot {
    pub id: BotId,
    pub cfg: StrategyConfig,
    pub state: BotRunState,
    pub created_at: u64,
    pub cycles_done: u32,
    /// Sum of closed cycles' net P&L (quote).
    pub realized_quote: f64,
    /// Highest bar-close equity seen (budget + P&L).
    pub peak_equity: f64,
    /// Most negative (equity - running peak), quote (<= 0).
    pub max_dd_quote: f64,
    pub dead_reason: Option<ExitReason>,
    /// Earliest bar open a new cycle may start at.
    pub next_decision_ms: u64,
    /// Cycles started since the last Start press (max_cycles counts these).
    pub chain_cycles: u32,
    /// Set by a PriceCross start once the price was crossed.
    #[serde(default)]
    pub start_triggered: bool,
    /// Bar-count integrals for the two utilisation figures.
    #[serde(default)]
    pub util: Utilisation,
    #[serde(default)]
    pub archived_at: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Utilisation {
    /// Sum over bars of margin in positions (quote x bars).
    pub margin_bars: f64,
    /// Sum over bars of capital reserved by the open cycle.
    pub commit_bars: f64,
    /// Bars of life (first cycle start onward, cooldowns included).
    pub bars: f64,
}
