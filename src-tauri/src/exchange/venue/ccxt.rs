//! USDT-margined perpetual orders on Bybit and OKX through the official
//! ccxt crate. Binance keeps its own, testnet-verified layer
//! (`exchange/live_orders.rs`); this module answers the same questions with
//! the same types so the engine does not care which venue it talks to.
//!
//! Conventions shared with the Binance layer:
//! - symbols are exchange ids like `BTCUSDT`; the ccxt symbol is
//!   `BTC/USDT:USDT`;
//! - quantities are in the BASE coin. OKX trades contracts (0.01 BTC each on
//!   BTC-USDT-SWAP), so every amount is converted with the market's
//!   `contractSize` on the way in and out;
//! - order ids are i64. ccxt venues use string ids, so ids handed back are
//!   ours: a stable hash for market orders (informational) and, for the
//!   protective stop, `placed_ms * 10 + kind`, which carries what `cancel`
//!   and `status` need;
//! - a protective stop closes the WHOLE position (Binance `closePosition`):
//!   Bybit position-level stop (trading-stop), OKX algo with
//!   `closeFraction=1`.
//!   Every placement is read back; a stop the venue does not show as live is
//!   an error, so the engine never believes a position is protected when it
//!   is not;
//! - the breakeven move changes that stop IN PLACE (`move_stop`): Bybit has
//!   one stop slot per position, OKX amends the algo. Nothing is cancelled
//!   afterwards (on Bybit a cancel writes "0" into the same slot).
//!
//! Bitget had an order path here until 2026-10-09. Its keys were refused (no
//! permission verifier) and it never ran a dry run, so the path was removed
//! rather than kept as a module nothing could reach.
//!
//! Errors map onto `BinanceKeyCheckError` so the engine's retry / refuse
//! logic stays one code path: OrderNotFound → Rejected{-2013},
//! AuthenticationError → InvalidCredentials, network → NetworkUnavailable,
//! rate limit → RateLimited, anything sent with an unknown outcome → Unknown.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ccxt::exchange::ExchangeRuntime;
use ccxt::exchange_generated::ExchangeBase;
use ccxt::exchanges::{bybit::BybitCore, okx::OkxCore};
use ccxt::runtime::{call_typed, catch_typed};
use ccxt::types::{Market, Order, Trade};
use ccxt::{Config, ExchangeError, Params, Value};

use crate::exchange::model::{BinanceKeyCheckError as E, FuturesAccount, FuturesPosition};
use crate::exchange::providers::binance_fills::UserTrade;
use crate::exchange::providers::binance_requests::Side;
use crate::exchange::providers::binance_parse::{OrderFill, OrderState, RecentOrder};
use crate::exchange::providers::binance_requests::Protective;
use crate::exchange::providers::binance_rules::{close_parts, LotStep, SymbolRules};
use crate::vault::model::ExchangeCredential;

/// Venues served here. Order matters for nothing; the list is the allowlist.
pub const CCXT_VENUES: [&str; 2] = ["bybit", "okx"];

pub fn is_ccxt_venue(exchange_id: &str) -> bool {
    CCXT_VENUES.contains(&exchange_id)
}

/// Testnet / demo endpoints for every ccxt venue (the TESTNET build sets it).
pub(crate) fn sandbox_from_env() -> bool {
    std::env::var("ALEPH_EDGE_VENUE_SANDBOX").is_ok_and(|v| v.trim() == "1")
}

enum Core {
    Bybit(BybitCore),
    Okx(OkxCore),
}

/// Runs the same expression on whichever core this is.
macro_rules! on {
    ($core:expr, $c:ident => $body:expr) => {
        match $core {
            Core::Bybit($c) => $body,
            Core::Okx($c) => $body,
        }
    };
}

struct Venue {
    id: &'static str,
    core: Core,
    markets_loaded: bool,
}

/// One authenticated client per (venue, key), reused across calls so
/// markets load once and ccxt's rate limiter sees every request.
#[derive(Default)]
pub struct CcxtVenues {
    pool: Mutex<HashMap<String, Arc<tokio::sync::Mutex<Venue>>>>,
}

fn venue_id(id: &str) -> Option<&'static str> {
    CCXT_VENUES.iter().copied().find(|v| *v == id)
}

fn new_core(id: &str, cred: &ExchangeCredential) -> Option<Core> {
    let mut cfg = Config::new().api_key(&cred.api_key).secret(&cred.api_secret);
    if let Some(p) = cred.passphrase.as_deref() {
        cfg = cfg.password(p);
    }
    let cfg = cfg.into_option();
    let mut core = match id {
        "bybit" => Core::Bybit(BybitCore::new(cfg)),
        "okx" => Core::Okx(OkxCore::new(cfg)),
        _ => return None,
    };
    if sandbox_from_env() {
        let ok = on!(&mut core, c => catch_typed(|| ExchangeBase::set_sandbox_mode(c, Value::Bool(true))));
        if ok.is_err() {
            return None;
        }
    }
    Some(core)
}

/// ccxt error → the engine's error vocabulary.
fn map_err(e: ExchangeError) -> E {
    let kind = e.kind.as_str();
    let msg = format!("{}: {}", e.kind, e.message);
    if e.is("OrderNotFound") {
        E::Rejected { code: -2013, msg }
    } else if e.is("AuthenticationError") || e.is("PermissionDenied") || e.is("AccountSuspended") {
        E::InvalidCredentials
    } else if e.is("RateLimitExceeded") || e.is("DDoSProtection") {
        E::RateLimited
    } else if e.is("RequestTimeout") || kind == "ExchangeNotAvailable" || kind == "OnMaintenance" {
        // A timeout may have reached the venue: the outcome is unknown.
        E::Unknown
    } else if e.is("NetworkError") {
        E::NetworkUnavailable
    } else {
        E::Rejected { code: 0, msg }
    }
}

fn s(v: &str) -> Value {
    Value::from(v)
}

fn get<'a>(v: &'a Value, key: &str) -> &'a Value {
    match v {
        Value::Dict(d) => d.get(key).unwrap_or(&Value::Null),
        _ => &Value::Null,
    }
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        Value::Str(s) => s.parse().ok(),
        _ => None,
    }
}

fn text(v: &Value) -> Option<String> {
    match v {
        Value::Str(s) => Some(s.to_string()),
        Value::Int(i) => Some(i.to_string()),
        _ => None,
    }
}

/// `BTCUSDT` → `BTC/USDT:USDT`. Only USDT-margined linear perps are traded.
pub fn unified_symbol(id: &str) -> Option<String> {
    let base = id.strip_suffix("USDT")?;
    (!base.is_empty()).then(|| format!("{base}/USDT:USDT"))
}

/// `BTC/USDT:USDT` → `BTCUSDT`.
pub fn exchange_symbol(unified: &str) -> String {
    match unified.split_once('/') {
        Some((base, rest)) => format!("{base}{}", rest.split(':').next().unwrap_or("")),
        None => unified.to_string(),
    }
}

/// Client ids the venues accept: OKX allows letters and digits only (max
/// 32), Bybit up to 36. Deterministic, so a lookup by the
/// engine's id finds the order it sent; the `ae`/`as` prefix survives.
pub fn venue_client_id(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { 'x' }).take(32).collect()
}

/// OKX algo order types the app or a user can leave resting on a symbol.
/// "conditional,oco" is one request (OKX accepts that pair comma-joined).
const OKX_ALGO_TYPES: [&str; 2] = ["conditional,oco", "trigger"];

/// Bybit v5 position list → hedge mode: any row on positionIdx 1 or 2.
/// None when the answer has no list to judge from.
pub fn bybit_hedged(r: &Value) -> Option<bool> {
    match get(get(r, "result"), "list") {
        Value::Arr(rows) if !rows.is_empty() => Some(rows.iter().any(|row| num(get(row, "positionIdx")).is_some_and(|i| i != 0.0))),
        _ => None,
    }
}

/// OKX algo-pending answer → the algo ids it lists.
pub fn okx_algo_ids(r: &Value) -> Vec<String> {
    match get(r, "data") {
        Value::Arr(rows) => rows.iter().filter_map(|row| text(get(row, "algoId")).filter(|id| !id.is_empty())).collect(),
        _ => Vec::new(),
    }
}

/// A price as the venue reads it: printed at the tick's decimals, so a
/// quantized float (0.30000000000000004) is sent as the exact decimal.
pub fn price_text(price: f64, tick: f64) -> String {
    if tick > 0.0 {
        format!("{price:.*}", step_decimals(tick))
    } else {
        price.to_string()
    }
}

/// Bybit v5 trading-stop body that moves the one-way position's stop slot to
/// `px` (mark price trigger, whole position).
pub fn bybit_move_stop_params(venue_symbol: &str, px: &str) -> Params {
    Params::new()
        .with_str("category", "linear")
        .with_str("symbol", venue_symbol)
        .with_str("tpslMode", "Full")
        .with_int("positionIdx", 0)
        .with_str("stopLoss", px)
        .with_str("slTriggerBy", "MarkPrice")
}

/// Bybit's "nothing changed" answer to a trading-stop update.
fn bybit_not_modified(message: &str) -> bool {
    message.contains("34040") || message.to_ascii_lowercase().contains("not modified")
}

/// OKX amend-algos body that moves the stop-loss of our algo (by its client
/// id) to `px`, still a market close on the mark price.
pub fn okx_amend_stop_params(inst_id: &str, algo_cl_ord_id: &str, px: &str) -> Params {
    Params::new()
        .with_str("instId", inst_id)
        .with_str("algoClOrdId", algo_cl_ord_id)
        .with_str("newSlTriggerPx", px)
        .with_str("newSlOrdPx", "-1")
        .with_str("newSlTriggerPxType", "mark")
}

/// Bybit v5 position list -> the stop level of the open one-way row (None:
/// no open row, or no stop on it).
pub fn bybit_open_stop(r: &Value) -> Option<f64> {
    let Value::Arr(rows) = get(get(r, "result"), "list") else { return None };
    rows.iter()
        .find(|row| num(get(row, "size")).is_some_and(|s| s > 0.0))
        .and_then(|row| num(get(row, "stopLoss")))
        .filter(|l| *l > 0.0)
}

/// OKX order-algo answer -> the stop-loss trigger of the algo while it still
/// rests (None once it fired, was cancelled, or is unknown).
pub fn okx_live_stop(r: &Value) -> Option<f64> {
    let Value::Arr(rows) = get(r, "data") else { return None };
    let row = rows.first()?;
    let live = matches!(text(get(row, "state")).as_deref(), Some("live" | "pause" | "partially_effective"));
    live.then(|| num(get(row, "slTriggerPx"))).flatten().filter(|l| *l > 0.0)
}

/// Whether a read-back stop level is `trigger` (within half a tick).
pub fn stop_reads_at(read: Option<f64>, trigger: f64, tick: f64) -> bool {
    let tol = if tick > 0.0 { tick / 2.0 } else { trigger.abs() * 1e-9 };
    read.is_some_and(|l| (l - trigger).abs() <= tol)
}

/// Stable i64 for a venue's string order id (informational only).
fn id_hash(id: &str) -> i64 {
    if let Ok(n) = id.parse::<i64>() {
        return n;
    }
    // FNV-1a, positive.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in id.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    (h >> 1) as i64
}

/// Protective id: placement time and kind, so cancel/status need no lookup.
pub fn protective_id(placed_ms: u64, kind: Protective) -> i64 {
    (placed_ms as i64) * 10 + matches!(kind, Protective::TakeProfit) as i64
}

pub fn protective_parts(id: i64) -> (u64, Protective) {
    let kind = if id % 10 == 1 { Protective::TakeProfit } else { Protective::Stop };
    ((id / 10).max(0) as u64, kind)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// What one market needs for sizing: contract size and rules in BASE units.
struct MarketInfo {
    unified: String,
    venue_id: String,
    contract_size: f64,
    /// The amount step in the venue's own unit (contracts on OKX, base coin
    /// on Bybit); 0 when the venue did not say.
    lot_contracts: f64,
    rules: SymbolRules,
}

/// Decimal places of a step like 0.01 (2) or 1 (0).
fn step_decimals(step: f64) -> usize {
    let s = format!("{step:.12}");
    s.trim_end_matches('0').split_once('.').map_or(0, |(_, frac)| frac.len())
}

/// The order amount in the venue's unit for `qty` in BASE coin: a whole
/// number of `lot` steps. A float division lands just under a whole step
/// (0.3 / 0.1 = 2.9999999999999996) and ccxt TRUNCATES the amount, which
/// sent OKX orders and closes one contract short. Within 1e-6 of a whole
/// step rounds to it; anything else floors (never more than asked).
pub fn contracts_for(qty: f64, contract_size: f64, lot: f64) -> f64 {
    let cs = if contract_size > 0.0 { contract_size } else { 1.0 };
    let raw = qty / cs;
    if lot.is_nan() || lot <= 0.0 || !raw.is_finite() {
        return raw;
    }
    let steps = raw / lot;
    let whole = if (steps - steps.round()).abs() < 1e-6 { steps.round() } else { steps.floor() };
    let amount = whole.max(0.0) * lot;
    // Printed at the step's decimals, so the float ccxt formats reads as the
    // exact decimal (2.99, not 2.9899999999999998, which truncates to 2.98).
    format!("{amount:.*}", step_decimals(lot)).parse().unwrap_or(amount)
}

fn market_info(m: &Market) -> MarketInfo {
    let cs = num(get(&m.raw, "contractSize")).filter(|c| *c > 0.0).unwrap_or(1.0);
    let step = m.precision.amount.unwrap_or(0.0) * cs;
    let min_qty = m.limits.amount.min.unwrap_or(0.0) * cs;
    MarketInfo {
        unified: m.symbol.clone(),
        venue_id: m.id.clone(),
        contract_size: cs,
        lot_contracts: m.precision.amount.unwrap_or(0.0),
        rules: SymbolRules {
            lot: LotStep { step: if step > 0.0 { step } else { min_qty }, min_qty },
            tick: m.precision.price.unwrap_or(0.0),
            market_max_qty: m.limits.amount.max.map(|x| x * cs),
            min_notional: m.limits.cost.min.unwrap_or(0.0),
            trading: m.active && m.swap && m.linear != Some(false),
        },
    }
}

fn fill_of(o: &Order, cs: f64) -> Option<OrderFill> {
    let avg = o.average.or(o.price).filter(|p| *p > 0.0)?;
    let filled = o.filled.filter(|f| *f > 0.0)? * cs;
    Some(OrderFill { order_id: id_hash(o.id.as_deref().unwrap_or("")), avg_price: avg, executed_qty: filled })
}

/// ccxt order status → the Binance words the engine reads.
fn binance_status(o: &Order) -> String {
    let filled = o.filled.unwrap_or(0.0) > 0.0;
    match o.status.as_deref() {
        Some("closed") => "FILLED",
        Some("open") if filled => "PARTIALLY_FILLED",
        Some("open") => "NEW",
        Some("canceled") if filled => "PARTIALLY_FILLED",
        Some("canceled") => "CANCELED",
        Some("expired") => "EXPIRED",
        Some("rejected") => "REJECTED",
        _ => "NEW",
    }
    .to_string()
}

impl CcxtVenues {
    fn venue(&self, cred: &ExchangeCredential) -> Result<Arc<tokio::sync::Mutex<Venue>>, E> {
        let id = venue_id(&cred.exchange_id).ok_or(E::Rejected { code: 0, msg: "unsupported exchange".into() })?;
        // Key the pool by venue + api key (never the secret).
        let pool_key = format!("{id}:{}", cred.api_key);
        let mut pool = self.pool.lock().expect("venue pool");
        if let Some(v) = pool.get(&pool_key) {
            return Ok(v.clone());
        }
        let core = new_core(id, cred).ok_or(E::Rejected { code: 0, msg: "venue setup failed".into() })?;
        let v = Arc::new(tokio::sync::Mutex::new(Venue { id, core, markets_loaded: false }));
        pool.insert(pool_key, v.clone());
        Ok(v)
    }

    async fn loaded(v: &mut Venue) -> Result<(), E> {
        if !v.markets_loaded {
            on!(&mut v.core, c => call_typed(c.load_markets(&[Value::Bool(false), Value::Null])).await).map_err(map_err)?;
            v.markets_loaded = true;
        }
        Ok(())
    }

    async fn info(v: &mut Venue, symbol: &str) -> Result<MarketInfo, E> {
        Self::loaded(v).await?;
        let unified = unified_symbol(symbol).ok_or(E::Rejected { code: -1121, msg: format!("{symbol}: not a USDT perp") })?;
        let raw = on!(&v.core, c => catch_typed(|| ExchangeBase::market(c, s(&unified))))
            .map_err(|_| E::Rejected { code: -1121, msg: format!("{symbol}: unknown on {}", v.id) })?;
        Ok(market_info(&Market::from_value(raw)))
    }

    /// Params every order on this venue carries.
    fn order_params(id: &str) -> Params {
        match id {
            // Isolated margin is chosen per order on OKX.
            "okx" => Params::new().with_str("marginMode", "isolated"),
            _ => Params::new(),
        }
    }

    // ---- read side ------------------------------------------------------

    pub async fn symbol_rules(&self, cred: &ExchangeCredential, symbol: &str) -> Result<SymbolRules, E> {
        let v = self.venue(cred)?;
        let mut v = v.lock().await;
        Ok(Self::info(&mut v, symbol).await?.rules)
    }

    /// Hedge mode for `symbol`. Bybit sets the mode per symbol and ccxt
    /// 4.5.85 has no fetchPositionMode for it (every read was NotSupported),
    /// so its position list is read directly: hedge mode lists the symbol
    /// under positionIdx 1 and 2, one-way under 0.
    pub async fn hedge_mode(&self, cred: &ExchangeCredential, symbol: &str) -> Result<bool, E> {
        let v = self.venue(cred)?;
        let mut v = v.lock().await;
        let m = Self::info(&mut v, symbol).await?;
        if let Core::Bybit(c) = &mut v.core {
            let p = Params::new().with_str("category", "linear").with_str("symbol", &m.venue_id);
            let r = call_typed(c.private_get_v5_position_list(&[p.into_value()])).await.map_err(map_err)?;
            return bybit_hedged(&r).ok_or(E::Unknown);
        }
        let r = on!(&mut v.core, c => call_typed(c.fetch_position_mode(&[s(&m.unified), Value::Null])).await).map_err(map_err)?;
        Ok(matches!(get(&r, "hedged"), Value::Bool(true)))
    }

    pub async fn futures_account(&self, cred: &ExchangeCredential) -> Result<FuturesAccount, E> {
        let v = self.venue(cred)?;
        let mut v = v.lock().await;
        Self::loaded(&mut v).await?;
        let params = match v.id {
            "bybit" => Params::new().with_str("type", "swap"),
            _ => Params::new().with_str("type", "swap"),
        };
        let bal = on!(&mut v.core, c => call_typed(c.fetch_balance(&[params.into_value()])).await).map_err(map_err)?;
        let usdt = get(&bal, "USDT");
        let total = num(get(usdt, "total")).unwrap_or(0.0);
        let free = num(get(usdt, "free")).unwrap_or(0.0);
        let raw = on!(&mut v.core, c => call_typed(c.fetch_positions(&[Value::Null, Value::Null])).await).map_err(map_err)?;
        let mut positions = Vec::new();
        let mut upnl = 0.0;
        if let Value::Arr(list) = raw {
            for p in list.iter() {
                let contracts = num(get(p, "contracts")).unwrap_or(0.0);
                if contracts == 0.0 {
                    continue;
                }
                let cs = num(get(p, "contractSize")).filter(|c| *c > 0.0).unwrap_or(1.0);
                let short = text(get(p, "side")).as_deref() == Some("short");
                let amt = contracts * cs * if short { -1.0 } else { 1.0 };
                let unreal = num(get(p, "unrealizedPnl")).unwrap_or(0.0);
                upnl += unreal;
                positions.push(FuturesPosition {
                    symbol: exchange_symbol(&text(get(p, "symbol")).unwrap_or_default()),
                    position_amt: amt,
                    entry_price: num(get(p, "entryPrice")).unwrap_or(0.0),
                    unrealized_pnl: unreal,
                });
            }
        }
        Ok(FuturesAccount { total_wallet_balance: total, available_balance: free, total_unrealized_pnl: upnl, positions })
    }

    pub async fn open_order_count(&self, cred: &ExchangeCredential, symbol: &str) -> Result<usize, E> {
        let v = self.venue(cred)?;
        let mut v = v.lock().await;
        let m = Self::info(&mut v, symbol).await?;
        if let Core::Okx(c) = &mut v.core {
            // ccxt's trigger=true lists ordType=trigger only: the app's own
            // stop / take-profit algos (ordType=conditional) went uncounted.
            let regular = call_typed(c.fetch_open_orders(&[s(&m.unified), Value::Null, Value::Null, Params::none().into_value()]))
                .await
                .map_err(map_err)?;
            let mut total = match regular {
                Value::Arr(a) => a.len(),
                _ => return Err(E::Unknown),
            };
            for ord_type in OKX_ALGO_TYPES {
                total += okx_algo_ids(&Self::okx_algos_pending(c, &m.venue_id, ord_type).await?).len();
            }
            return Ok(total);
        }
        let mut total = 0;
        for trigger in [false, true] {
            let p = if trigger { Params::new().with_bool("trigger", true) } else { Params::none() };
            let r = on!(&mut v.core, c => call_typed(c.fetch_open_orders(&[s(&m.unified), Value::Null, Value::Null, p.into_value()])).await)
                .map_err(map_err)?;
            if let Value::Arr(a) = r {
                total += a.len();
            }
        }
        Ok(total)
    }

    pub async fn order_by_client_id(&self, cred: &ExchangeCredential, symbol: &str, client_id: &str) -> Result<OrderState, E> {
        let v = self.venue(cred)?;
        let mut v = v.lock().await;
        let m = Self::info(&mut v, symbol).await?;
        let cid = venue_client_id(client_id);
        let r = on!(&mut v.core, c => call_typed(c.fetch_order_with_client_order_id(s(&cid), &[s(&m.unified), Value::Null])).await)
            .map_err(map_err)?;
        let o = Order::from_value(r);
        Ok(OrderState { status: binance_status(&o), fill: fill_of(&o, m.contract_size) })
    }

    pub async fn recent_orders(&self, cred: &ExchangeCredential, symbol: &str) -> Result<Vec<RecentOrder>, E> {
        let v = self.venue(cred)?;
        let mut v = v.lock().await;
        let m = Self::info(&mut v, symbol).await?;
        let r = on!(&mut v.core, c => call_typed(c.fetch_closed_orders(&[s(&m.unified), Value::Null, Value::Int(50), Value::Null])).await)
            .map_err(map_err)?;
        let Value::Arr(list) = r else { return Err(E::Unknown) };
        Ok(list
            .iter()
            .map(|raw| {
                let o = Order::from_value(raw.clone());
                RecentOrder {
                    order_id: id_hash(o.id.as_deref().unwrap_or("")),
                    client_order_id: o.client_order_id.clone().unwrap_or_default(),
                    buy: o.side.as_deref() == Some("buy"),
                    status: binance_status(&o),
                    avg_price: o.average.or(o.price).unwrap_or(0.0),
                    executed_qty: o.filled.unwrap_or(0.0) * m.contract_size,
                    reducing: o.reduce_only.unwrap_or(false),
                    update_time: o.last_update_timestamp.or(o.timestamp).unwrap_or(0).max(0) as u64,
                }
            })
            .collect())
    }

    pub async fn user_trades(&self, cred: &ExchangeCredential, symbol: &str, start_ms: u64) -> Result<Vec<UserTrade>, E> {
        let v = self.venue(cred)?;
        let mut v = v.lock().await;
        let m = Self::info(&mut v, symbol).await?;
        let mut out = Vec::new();
        let mut since = start_ms as i64;
        for _ in 0..20 {
            let r = on!(&mut v.core, c => call_typed(c.fetch_my_trades(&[s(&m.unified), Value::Int(since), Value::Int(100), Value::Null])).await)
                .map_err(map_err)?;
            let Value::Arr(list) = r else { return Err(E::Unknown) };
            let n = list.len();
            let mut last = since;
            for raw in list.iter() {
                let t = Trade::from_value(raw.clone());
                let ts = t.timestamp.unwrap_or(0);
                last = last.max(ts + 1);
                let (fee, asset) = t
                    .fee
                    .as_ref()
                    .map(|f| (f.cost.unwrap_or(0.0), f.currency.clone().unwrap_or_default()))
                    .unwrap_or((0.0, String::new()));
                out.push(UserTrade {
                    id: id_hash(t.id.as_deref().unwrap_or("")),
                    order_id: id_hash(t.order.as_deref().unwrap_or("")),
                    side: if t.side.as_deref() == Some("buy") { Side::Buy } else { Side::Sell },
                    price: t.price.unwrap_or(0.0),
                    qty: t.amount.unwrap_or(0.0) * m.contract_size,
                    commission: fee,
                    commission_asset: asset,
                    time: ts.max(0) as u64,
                });
            }
            if n < 100 || last <= since {
                break;
            }
            since = last;
        }
        Ok(out)
    }

    // ---- write side -----------------------------------------------------

    pub async fn set_leverage(&self, cred: &ExchangeCredential, symbol: &str, leverage: u8) -> Result<(), E> {
        let v = self.venue(cred)?;
        let mut v = v.lock().await;
        let m = Self::info(&mut v, symbol).await?;
        let p = Self::order_params(v.id);
        let r = on!(&mut v.core, c => call_typed(c.set_leverage(Value::Int(leverage as i64), &[s(&m.unified), p.into_value()])).await);
        match r {
            Ok(_) => Ok(()),
            // Bybit 110043 "leverage not modified": already at that value.
            Err(e) if e.message.contains("110043") || e.message.to_ascii_lowercase().contains("not modified") => Ok(()),
            Err(e) => Err(map_err(e)),
        }
    }

    pub async fn set_isolated(&self, cred: &ExchangeCredential, symbol: &str) -> Result<(), E> {
        let v = self.venue(cred)?;
        let mut v = v.lock().await;
        let m = Self::info(&mut v, symbol).await?;
        if v.id == "okx" {
            // OKX takes the margin mode on every order (tdMode); nothing to set.
            return Ok(());
        }
        let r = on!(&mut v.core, c => call_typed(c.set_margin_mode(s("isolated"), &[s(&m.unified), Value::Null])).await);
        match r {
            Ok(_) => Ok(()),
            Err(e) => {
                let low = e.message.to_ascii_lowercase();
                // Already isolated / nothing to change.
                if low.contains("not modified") || low.contains("same") || e.message.contains("110026") || e.message.contains("100028") {
                    Ok(())
                } else {
                    Err(map_err(e))
                }
            }
        }
    }

    async fn market_order(&self, cred: &ExchangeCredential, symbol: &str, buy: bool, qty: f64, reduce: bool, client_id: Option<&str>) -> Result<OrderFill, E> {
        let v = self.venue(cred)?;
        let mut v = v.lock().await;
        let m = Self::info(&mut v, symbol).await?;
        let amount = contracts_for(qty, m.contract_size, m.lot_contracts);
        if amount.is_nan() || amount <= 0.0 {
            return Err(E::Rejected { code: 0, msg: format!("{symbol}: {qty} is below one contract") });
        }
        let mut p = Self::order_params(v.id).with_bool("reduceOnly", reduce);
        if let Some(cid) = client_id {
            p = p.with_str("clientOrderId", &venue_client_id(cid));
        }
        let side = if buy { "buy" } else { "sell" };
        let placed = on!(&mut v.core, c => call_typed(c.create_order(s(&m.unified), s("market"), s(side), Value::Float(amount), &[Value::Null, p.into_value()])).await)
            .map_err(map_err)?;
        let o = Order::from_value(placed);
        if let Some(f) = fill_of(&o, m.contract_size) {
            return Ok(f);
        }
        // Most venues answer a market order with its id only: read it back.
        let id = o.id.clone().ok_or(E::Unknown)?;
        for attempt in 0..4u64 {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(300 * attempt)).await;
            }
            let Ok(r) = on!(&mut v.core, c => call_typed(c.fetch_order(s(&id), &[s(&m.unified), Value::Null])).await) else {
                continue;
            };
            if let Some(f) = fill_of(&Order::from_value(r), m.contract_size) {
                return Ok(f);
            }
        }
        Err(E::Unknown)
    }

    pub async fn open_market(&self, cred: &ExchangeCredential, symbol: &str, long: bool, qty: f64, client_id: &str) -> Result<OrderFill, E> {
        self.market_order(cred, symbol, long, qty, false, Some(client_id)).await
    }

    pub async fn reduce_market(&self, cred: &ExchangeCredential, symbol: &str, long: bool, qty: f64) -> Result<OrderFill, E> {
        self.market_order(cred, symbol, !long, qty, true, None).await
    }

    pub async fn reduce_market_all(&self, cred: &ExchangeCredential, symbol: &str, long: bool, qty: f64) -> Result<OrderFill, E> {
        let parts = match self.symbol_rules(cred, symbol).await {
            Ok(rules) => close_parts(qty, rules.market_max_qty, rules.lot),
            Err(_) => vec![qty],
        };
        let (mut filled, mut value, mut order_id) = (0.0, 0.0, 0);
        for part in parts {
            let fill = self.reduce_market(cred, symbol, long, part).await?;
            filled += fill.executed_qty;
            value += fill.executed_qty * fill.avg_price;
            order_id = fill.order_id;
        }
        if filled <= 0.0 {
            return Err(E::Unknown);
        }
        Ok(OrderFill { order_id, avg_price: value / filled, executed_qty: filled })
    }

    /// Whole-position stop or take-profit; read back before it counts.
    pub async fn place_protective(&self, cred: &ExchangeCredential, symbol: &str, long: bool, kind: Protective, trigger: f64) -> Result<i64, E> {
        let placed = now_ms();
        let id = protective_id(placed, kind);
        {
            let v = self.venue(cred)?;
            let mut v = v.lock().await;
            let m = Self::info(&mut v, symbol).await?;
            let px = trigger.to_string();
            let stop = matches!(kind, Protective::Stop);
            match &mut v.core {
                Core::Bybit(c) => {
                    let mut p = Params::new()
                        .with_str("category", "linear")
                        .with_str("symbol", &m.venue_id)
                        .with_str("tpslMode", "Full")
                        .with_int("positionIdx", 0);
                    p = if stop {
                        p.with_str("stopLoss", &px).with_str("slTriggerBy", "MarkPrice")
                    } else {
                        p.with_str("takeProfit", &px).with_str("tpTriggerBy", "MarkPrice")
                    };
                    call_typed(c.private_post_v5_position_trading_stop(&[p.into_value()])).await.map_err(map_err)?;
                }
                Core::Okx(c) => {
                    let mut p = Params::new()
                        .with_str("instId", &m.venue_id)
                        .with_str("tdMode", "isolated")
                        .with_str("side", if long { "sell" } else { "buy" })
                        .with_str("ordType", "conditional")
                        .with_str("closeFraction", "1")
                        .with_bool("reduceOnly", true)
                        .with_str("algoClOrdId", &format!("ap{id}"));
                    p = if stop {
                        p.with_str("slTriggerPx", &px).with_str("slOrdPx", "-1").with_str("slTriggerPxType", "mark")
                    } else {
                        p.with_str("tpTriggerPx", &px).with_str("tpOrdPx", "-1").with_str("tpTriggerPxType", "mark")
                    };
                    call_typed(c.private_post_trade_order_algo(&[p.into_value()])).await.map_err(map_err)?;
                }
            }
        }
        // Read back: a stop the venue does not show as live is not a stop.
        match self.algo_status(cred, symbol, id).await {
            Ok(st) if st == "NEW" => Ok(id),
            Ok(st) => Err(E::Rejected { code: 0, msg: format!("protective not live after placement ({st})") }),
            Err(e) => Err(e),
        }
    }

    /// Moves the whole-position stop `id` to `trigger` IN PLACE (the
    /// breakeven move) and reads it back. Bybit: one trading-stop update of
    /// the position's stop slot, a new id (placed now, so a later fire is
    /// found in the order history). OKX: amend-algos on the same algo, which
    /// keeps its client id, so the id is unchanged. Nothing is cancelled: on
    /// Bybit a cancel writes "0" into the very slot just moved. An error
    /// leaves the old stop where it was (the move either happened or not).
    pub async fn move_stop(&self, cred: &ExchangeCredential, symbol: &str, id: i64, trigger: f64) -> Result<i64, E> {
        if !matches!(protective_parts(id).1, Protective::Stop) {
            return Err(E::Rejected { code: 0, msg: "not a stop id".into() });
        }
        let v = self.venue(cred)?;
        let mut v = v.lock().await;
        let m = Self::info(&mut v, symbol).await?;
        let px = price_text(trigger, m.rules.tick);
        let (new_id, read) = match &mut v.core {
            Core::Bybit(c) => {
                let sent = call_typed(c.private_post_v5_position_trading_stop(&[bybit_move_stop_params(&m.venue_id, &px).into_value()])).await;
                match sent {
                    Ok(_) => {}
                    // 34040 "not modified": the slot already holds this level
                    // (a retry after a lost answer). The read-back decides.
                    Err(e) if bybit_not_modified(&e.message) => {}
                    Err(e) => return Err(map_err(e)),
                }
                let p = Params::new().with_str("category", "linear").with_str("symbol", &m.venue_id);
                let r = call_typed(c.private_get_v5_position_list(&[p.into_value()])).await.map_err(map_err)?;
                (protective_id(now_ms(), Protective::Stop), bybit_open_stop(&r))
            }
            Core::Okx(c) => {
                let cl = format!("ap{id}");
                let sent = call_typed(c.private_post_trade_amend_algos(&[okx_amend_stop_params(&m.venue_id, &cl, &px).into_value()])).await;
                let p = Params::new().with_str("algoClOrdId", &cl);
                let r = call_typed(c.private_get_trade_order_algo(&[p.into_value()])).await.map_err(map_err)?;
                let read = okx_live_stop(&r);
                // A refused amend counts only when the algo is not already
                // there (a retry after an answer that was lost).
                if let Err(e) = sent {
                    if !stop_reads_at(read, trigger, m.rules.tick) {
                        return Err(map_err(e));
                    }
                }
                (id, read)
            }
        };
        if stop_reads_at(read, trigger, m.rules.tick) {
            Ok(new_id)
        } else {
            Err(E::Rejected { code: 0, msg: format!("stop not at {px} after the move (reads {read:?})") })
        }
    }

    pub async fn cancel_protective(&self, cred: &ExchangeCredential, symbol: &str, id: i64) -> Result<(), E> {
        let (_, kind) = protective_parts(id);
        let stop = matches!(kind, Protective::Stop);
        let v = self.venue(cred)?;
        let mut v = v.lock().await;
        let m = Self::info(&mut v, symbol).await?;
        let r = match &mut v.core {
            Core::Bybit(c) => {
                let p = Params::new()
                    .with_str("category", "linear")
                    .with_str("symbol", &m.venue_id)
                    .with_str("tpslMode", "Full")
                    .with_int("positionIdx", 0)
                    .with_str(if stop { "stopLoss" } else { "takeProfit" }, "0");
                call_typed(c.private_post_v5_position_trading_stop(&[p.into_value()])).await
            }
            Core::Okx(c) => {
                let one = Params::new().with_str("instId", &m.venue_id).with_str("algoClOrdId", &format!("ap{id}"));
                let list = Value::Arr(Arc::new(vec![one.into_value()]));
                call_typed(c.private_post_trade_cancel_algos(&[list])).await
            }
        };
        match r {
            Ok(_) => Ok(()),
            // Gone already (fired, or the position closed): nothing to cancel.
            Err(e) if e.is("OrderNotFound") => Ok(()),
            Err(e) => Err(map_err(e)),
        }
    }

    /// "NEW" while the stop rests, "FINISHED" once it fired, "CANCELED" otherwise.
    pub async fn algo_status(&self, cred: &ExchangeCredential, symbol: &str, id: i64) -> Result<String, E> {
        let (placed, kind) = protective_parts(id);
        let stop = matches!(kind, Protective::Stop);
        let v = self.venue(cred)?;
        let mut v = v.lock().await;
        let m = Self::info(&mut v, symbol).await?;
        match &mut v.core {
            Core::Bybit(c) => {
                // Resting: the position carries the level. Fired: a closed
                // StopLoss / TakeProfit order since placement.
                let p = Params::new().with_str("category", "linear").with_str("symbol", &m.venue_id);
                let r = call_typed(c.private_get_v5_position_list(&[p.into_value()])).await.map_err(map_err)?;
                let list = get(get(&r, "result"), "list");
                if let Value::Arr(items) = list {
                    for it in items.iter() {
                        let size = num(get(it, "size")).unwrap_or(0.0);
                        let level = num(get(it, if stop { "stopLoss" } else { "takeProfit" })).unwrap_or(0.0);
                        if size > 0.0 && level > 0.0 {
                            return Ok("NEW".into());
                        }
                    }
                }
                let h = Params::new()
                    .with_str("category", "linear")
                    .with_str("symbol", &m.venue_id)
                    .with_str("stopOrderType", if stop { "StopLoss" } else { "TakeProfit" })
                    .with_int("startTime", placed as i64)
                    .with_int("limit", 20);
                let r = call_typed(c.private_get_v5_order_history(&[h.into_value()])).await.map_err(map_err)?;
                if let Value::Arr(items) = get(get(&r, "result"), "list") {
                    if items.iter().any(|o| text(get(o, "orderStatus")).as_deref() == Some("Filled")) {
                        return Ok("FINISHED".into());
                    }
                }
                Ok("CANCELED".into())
            }
            Core::Okx(c) => {
                let p = Params::new().with_str("algoClOrdId", &format!("ap{id}"));
                let r = call_typed(c.private_get_trade_order_algo(&[p.into_value()])).await.map_err(map_err)?;
                let state = match get(&r, "data") {
                    Value::Arr(a) => a.first().and_then(|o| text(get(o, "state"))).unwrap_or_default(),
                    _ => String::new(),
                };
                Ok(match state.as_str() {
                    "live" | "pause" | "partially_effective" => "NEW",
                    "effective" => "FINISHED",
                    "" => return Err(E::Rejected { code: -2013, msg: "algo order not found".into() }),
                    _ => "CANCELED",
                }
                .into())
            }
        }
    }

    /// OKX pending algo orders of `ord_type` on one instrument (raw answer).
    async fn okx_algos_pending(c: &mut OkxCore, inst_id: &str, ord_type: &str) -> Result<Value, E> {
        let p = Params::new()
            .with_str("instType", "SWAP")
            .with_str("instId", inst_id)
            .with_str("ordType", ord_type);
        call_typed(c.private_get_trade_orders_algo_pending(&[p.into_value()])).await.map_err(map_err)
    }

    /// OKX has no cancelAllOrders in ccxt 4.5.85 (NotSupported): the sweep
    /// lists the symbol's regular orders and cancels each, then its algo
    /// orders (the stop and take-profit) by id, ten per request.
    async fn okx_cancel_symbol_orders(c: &mut OkxCore, m: &MarketInfo) -> Result<(), E> {
        let mut first: Option<E> = None;
        let regular = call_typed(c.fetch_open_orders(&[s(&m.unified), Value::Null, Value::Null, Params::none().into_value()]))
            .await
            .map_err(map_err)?;
        if let Value::Arr(list) = regular {
            for o in list.iter() {
                let Some(id) = Order::from_value(o.clone()).id else { continue };
                if let Err(e) = call_typed(c.cancel_order(s(&id), &[s(&m.unified), Value::Null])).await {
                    if !e.is("OrderNotFound") && first.is_none() {
                        first = Some(map_err(e));
                    }
                }
            }
        }
        let mut algo_ids = Vec::new();
        for ord_type in OKX_ALGO_TYPES {
            algo_ids.extend(okx_algo_ids(&Self::okx_algos_pending(c, &m.venue_id, ord_type).await?));
        }
        for chunk in algo_ids.chunks(10) {
            let list: Vec<Value> = chunk
                .iter()
                .map(|id| Params::new().with_str("instId", &m.venue_id).with_str("algoId", id).into_value())
                .collect();
            if let Err(e) = call_typed(c.private_post_trade_cancel_algos(&[Value::Arr(Arc::new(list))])).await {
                if !e.is("OrderNotFound") && first.is_none() {
                    first = Some(map_err(e));
                }
            }
        }
        first.map_or(Ok(()), Err)
    }

    pub async fn cancel_symbol_orders(&self, cred: &ExchangeCredential, symbol: &str) -> Result<(), E> {
        let v = self.venue(cred)?;
        let mut v = v.lock().await;
        let m = Self::info(&mut v, symbol).await?;
        if let Core::Okx(c) = &mut v.core {
            return Self::okx_cancel_symbol_orders(c, &m).await;
        }
        let mut first: Option<E> = None;
        for trigger in [false, true] {
            let p = if trigger { Params::new().with_bool("trigger", true) } else { Params::none() };
            let r = on!(&mut v.core, c => call_typed(c.cancel_all_orders(&[s(&m.unified), p.into_value()])).await);
            if let Err(e) = r {
                if !e.is("OrderNotFound") && first.is_none() {
                    first = Some(map_err(e));
                }
            }
        }
        first.map_or(Ok(()), Err)
    }

    // ---- key check ------------------------------------------------------

    /// (can_trade_futures, can_withdraw) as the venue reports them.
    pub async fn key_permissions(&self, cred: &ExchangeCredential) -> Result<(bool, bool), E> {
        let v = self.venue(cred)?;
        let mut v = v.lock().await;
        match &mut v.core {
            Core::Bybit(c) => {
                let r = call_typed(c.private_get_v5_user_query_api(&[Value::Null])).await.map_err(map_err)?;
                let res = get(&r, "result");
                let read_only = num(get(res, "readOnly")).unwrap_or(1.0) != 0.0;
                let perms = get(res, "permissions");
                let has = |group: &str, p: &str| match get(perms, group) {
                    Value::Arr(a) => a.iter().any(|x| text(x).as_deref() == Some(p)),
                    _ => false,
                };
                let trade = !read_only && (has("ContractTrade", "Order") || has("ContractTrade", "Position") || has("Derivatives", "DerivativesTrade"));
                Ok((trade, has("Wallet", "Withdraw")))
            }
            Core::Okx(c) => {
                let r = call_typed(c.private_get_account_config(&[Value::Null])).await.map_err(map_err)?;
                let perm = match get(&r, "data") {
                    Value::Arr(a) => a.first().and_then(|o| text(get(o, "perm"))).unwrap_or_default(),
                    _ => String::new(),
                };
                Ok((perm.contains("trade"), perm.contains("withdraw")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_round_trip() {
        assert_eq!(unified_symbol("BTCUSDT").as_deref(), Some("BTC/USDT:USDT"));
        assert_eq!(unified_symbol("1000PEPEUSDT").as_deref(), Some("1000PEPE/USDT:USDT"));
        assert_eq!(unified_symbol("BTCUSDC"), None);
        assert_eq!(unified_symbol("USDT"), None);
        assert_eq!(exchange_symbol("BTC/USDT:USDT"), "BTCUSDT");
        assert_eq!(exchange_symbol("ETH/USDT"), "ETHUSDT");
    }

    #[test]
    fn client_ids_fit_every_venue_and_stay_deterministic() {
        let id = venue_client_id("as12345678-3-2");
        assert_eq!(id, "as12345678x3x2");
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_eq!(venue_client_id(&"a".repeat(60)).len(), 32);
        assert_eq!(venue_client_id("ae-x"), venue_client_id("ae-x"));
    }

    #[test]
    fn protective_ids_carry_time_and_kind() {
        let id = protective_id(1_759_700_000_123, Protective::TakeProfit);
        assert_eq!(protective_parts(id), (1_759_700_000_123, Protective::TakeProfit));
        let id = protective_id(1_759_700_000_123, Protective::Stop);
        assert!(matches!(protective_parts(id).1, Protective::Stop));
        assert!(id > 0);
    }

    #[test]
    fn okx_contracts_are_never_one_lot_short() {
        // The audit's probes: a float division lands under a whole contract.
        assert_eq!(contracts_for(0.3, 0.1, 1.0), 3.0);
        assert_eq!(contracts_for(4.3, 0.1, 1.0), 43.0, "a size read from the account round-trips");
        assert_eq!(contracts_for(0.0003, 0.0001, 1.0), 3.0);
        assert_eq!(contracts_for(0.299, 0.1, 0.01), 2.99);
        // A real fraction still floors: never more than asked.
        assert_eq!(contracts_for(0.35, 0.1, 1.0), 3.0);
        assert_eq!(contracts_for(0.05, 0.1, 1.0), 0.0, "below one contract");
        // Every whole contract count at ctVal 0.1 survives qty = n * 0.1.
        for n in 1..=20_000u32 {
            let qty = f64::from(n) * 0.1;
            assert_eq!(contracts_for(qty, 0.1, 1.0), f64::from(n), "{n} contracts");
        }
        // Base-coin venues (Bybit: contract size 1) on a 0.001 step.
        assert_eq!(contracts_for(0.003, 1.0, 0.001), 0.003);
        assert_eq!(contracts_for(0.0039, 1.0, 0.001), 0.003);
        // No step known: the plain division, as before.
        assert_eq!(contracts_for(0.25, 1.0, 0.0), 0.25);
        assert_eq!(step_decimals(1.0), 0);
        assert_eq!(step_decimals(0.01), 2);
        assert_eq!(step_decimals(0.0001), 4);
    }

    #[test]
    fn bybit_hedge_mode_comes_from_the_position_index() {
        let rows = |idx: &[i64]| {
            let list: Vec<serde_json::Value> = idx.iter().map(|i| serde_json::json!({"symbol": "BTCUSDT", "positionIdx": i, "size": "0"})).collect();
            Value::from(serde_json::json!({"retCode": 0, "result": {"category": "linear", "list": list}}))
        };
        assert_eq!(bybit_hedged(&rows(&[0])), Some(false), "one-way");
        assert_eq!(bybit_hedged(&rows(&[1, 2])), Some(true), "hedge");
        assert_eq!(bybit_hedged(&rows(&[])), None, "nothing to judge from");
        assert_eq!(bybit_hedged(&Value::from(serde_json::json!({"retCode": 10001}))), None);
    }

    #[test]
    fn okx_algo_ids_are_read_from_the_pending_list() {
        let r = Value::from(serde_json::json!({"code": "0", "data": [
            {"algoId": "681096944655273984", "ordType": "conditional", "instId": "BTC-USDT-SWAP"},
            {"algoId": "681096944655273985", "ordType": "oco"},
            {"algoId": ""}
        ]}));
        assert_eq!(okx_algo_ids(&r), ["681096944655273984", "681096944655273985"]);
        assert!(okx_algo_ids(&Value::from(serde_json::json!({"code": "0", "data": []}))).is_empty());
        assert!(okx_algo_ids(&Value::Null).is_empty());
    }

    fn dict(p: Params) -> Value {
        p.into_value_object()
    }

    #[test]
    fn bybit_breakeven_is_one_stop_slot_update() {
        let p = dict(bybit_move_stop_params("BTCUSDT", "61234.5"));
        for (k, want) in [("category", "linear"), ("symbol", "BTCUSDT"), ("tpslMode", "Full"), ("stopLoss", "61234.5"), ("slTriggerBy", "MarkPrice")] {
            assert_eq!(text(get(&p, k)).as_deref(), Some(want), "{k}");
        }
        assert_eq!(num(get(&p, "positionIdx")), Some(0.0), "one-way slot");
        // Never touches the take-profit slot, and never writes "0" (a cancel).
        assert!(matches!(get(&p, "takeProfit"), Value::Null));
        assert!(bybit_not_modified(r#"bybit {"retCode":34040,"retMsg":"not modified"}"#));
        assert!(!bybit_not_modified("10001 params error"));
    }

    #[test]
    fn bybit_stop_is_read_from_the_open_row() {
        let r = |rows: serde_json::Value| Value::from(serde_json::json!({"retCode": 0, "result": {"list": rows}}));
        let moved = r(serde_json::json!([{"symbol": "BTCUSDT", "positionIdx": 0, "size": "0.01", "stopLoss": "61234.5", "takeProfit": "65000"}]));
        assert_eq!(bybit_open_stop(&moved), Some(61234.5));
        // A flat row (size 0) or an empty slot ("0" / "") is not a stop.
        assert_eq!(bybit_open_stop(&r(serde_json::json!([{"size": "0", "stopLoss": "61234.5"}]))), None);
        assert_eq!(bybit_open_stop(&r(serde_json::json!([{"size": "0.01", "stopLoss": "0"}]))), None);
        assert_eq!(bybit_open_stop(&r(serde_json::json!([{"size": "0.01", "stopLoss": ""}]))), None);
        assert_eq!(bybit_open_stop(&Value::from(serde_json::json!({"retCode": 10001}))), None);
    }

    #[test]
    fn okx_breakeven_amends_the_same_algo() {
        let p = dict(okx_amend_stop_params("BTC-USDT-SWAP", "ap17597000001230", "61234.5"));
        for (k, want) in [
            ("instId", "BTC-USDT-SWAP"),
            ("algoClOrdId", "ap17597000001230"),
            ("newSlTriggerPx", "61234.5"),
            ("newSlOrdPx", "-1"),
            ("newSlTriggerPxType", "mark"),
        ] {
            assert_eq!(text(get(&p, k)).as_deref(), Some(want), "{k}");
        }
        assert!(matches!(get(&p, "newTpTriggerPx"), Value::Null), "the take-profit is not touched");
    }

    #[test]
    fn okx_stop_is_read_only_while_the_algo_rests() {
        let r = |state: &str, px: &str| Value::from(serde_json::json!({"code": "0", "data": [{"algoClOrdId": "ap1", "state": state, "slTriggerPx": px}]}));
        assert_eq!(okx_live_stop(&r("live", "61234.5")), Some(61234.5));
        assert_eq!(okx_live_stop(&r("partially_effective", "61234.5")), Some(61234.5));
        assert_eq!(okx_live_stop(&r("effective", "61234.5")), None, "fired");
        assert_eq!(okx_live_stop(&r("canceled", "61234.5")), None);
        assert_eq!(okx_live_stop(&r("live", "")), None);
        assert_eq!(okx_live_stop(&Value::from(serde_json::json!({"code": "0", "data": []}))), None);
    }

    #[test]
    fn a_move_counts_only_when_the_level_reads_back() {
        assert!(stop_reads_at(Some(61234.5), 61234.5, 0.1));
        assert!(stop_reads_at(Some(61234.54), 61234.5, 0.1), "venue rounding inside half a tick");
        assert!(!stop_reads_at(Some(60000.0), 61234.5, 0.1), "still the original stop");
        assert!(!stop_reads_at(None, 61234.5, 0.1), "no stop at all");
        assert!(stop_reads_at(Some(0.3), 0.1 + 0.2, 0.0));
        assert_eq!(price_text(0.1 + 0.2, 0.1), "0.3");
        assert_eq!(price_text(61234.5, 0.1), "61234.5");
        assert_eq!(price_text(2.0, 1.0), "2");
    }

    #[test]
    fn hashed_ids_are_stable_and_positive() {
        assert_eq!(id_hash("12345"), 12345);
        let a = id_hash("1e2f-uuid-like");
        assert_eq!(a, id_hash("1e2f-uuid-like"));
        assert!(a >= 0);
    }
}
