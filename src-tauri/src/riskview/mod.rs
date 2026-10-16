//! Portfolio risk view: what ALL open exposures risk together.
//!
//! - Stress test: every open exposure (signal-bot and manual positions, open
//!   DCA / Grid cycles) under BTC -10 / -20 / -30 / +10%, each coin moved by
//!   its beta to BTC (30 days of hourly DataHub candles, `beta`), walked by
//!   the code that manages it (`exposure`): stops, safety orders and grid
//!   buys on the way, liquidation at the margin.
//! - Crash replay: today's book through the real hourly paths of past crash
//!   windows (`replay`).
//! - What-if: the stress at BTC -20% before and after a candidate bot or
//!   trade (`whatif`).
//! - Concentration and BTC dependency, with threshold checks that alert in
//!   the app and once per crossing on Telegram (`stress`, `alerts`).
//!
//! Market data comes only from DataHub through the Sentinel API
//! (`market::history`), never from an exchange. Paper and real money are
//! separate books. A figure that cannot be computed is reported unavailable.

pub mod alerts;
pub mod beta;
mod commands;
pub mod exposure;
pub mod replay;
pub mod stress;
pub mod whatif;

pub use commands::{riskview_get, riskview_replay, riskview_whatif, spawn};

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::bot::engine::user_trade::is_user_trade_id;
use crate::bot::strategy::backtest::{candles_to_bars, plan_pages, rfc3339};
use crate::bot::strategy::model::{Bar, MarketKind, Side};
use crate::bot::strategy::StrategyManager;
use crate::bot::strategy_live::StrategyLiveManager;
use crate::bot::BotManager;
use crate::market::history::{self, HistoryError};
use crate::membership::MembershipManager;
use crate::risk::model::ViewThresholds;
use crate::signal::model::Direction;

use beta::BetaInfo;
use exposure::{Body, DdStop, Dir, Exposure, Source};
use stress::BookStress;

const HOUR_MS: u64 = 3_600_000;
/// A computed beta is reused this long.
const BETA_TTL_MS: u64 = 6 * HOUR_MS;
/// A failed beta read is retried after this long.
const BETA_RETRY_MS: u64 = 15 * 60_000;
/// A failed window read is retried after this long (served windows are kept
/// for the life of the process: the past does not change).
const WINDOW_RETRY_MS: u64 = 10 * 60_000;
pub const BTC: &str = "BTCUSDT";

/// One exposure as the Risk page lists it.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExposureRow {
    pub id: String,
    pub source: Source,
    pub label: String,
    pub symbol: String,
    pub market: MarketKind,
    pub live: bool,
    pub direction: Dir,
    pub capital: f64,
    /// None: no price read yet, left out of every figure.
    pub mark: Option<f64>,
    pub stop: Option<f64>,
    pub liq: Option<f64>,
    pub beta: Option<BetaInfo>,
    /// Why there is no beta (history error code, `betaShort`).
    pub beta_error: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RiskViewSnapshot {
    pub computed_at: u64,
    /// Browser preview sample data (never true from Rust).
    pub sample: bool,
    pub beta_days: u64,
    pub exposures: Vec<ExposureRow>,
    pub paper: BookStress,
    pub real: BookStress,
    pub thresholds: ViewThresholds,
    /// Exposures with no price yet (not in any figure).
    pub unpriced: u32,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReplayReport {
    pub computed_at: u64,
    pub sample: bool,
    pub windows: Vec<replay::ReplayWindow>,
}

type Key = (MarketKind, String);
type WindowKey = (&'static str, MarketKind, String);

#[derive(Default)]
struct Caches {
    betas: HashMap<Key, (Result<BetaInfo, String>, u64)>,
    btc_hourly: HashMap<MarketKind, (Vec<Bar>, u64)>,
    windows: HashMap<WindowKey, (Result<Vec<Bar>, String>, u64)>,
}

/// Tauri-managed state of the risk view.
pub struct RiskViewManager {
    caches: Mutex<Caches>,
    latest: Mutex<Option<RiskViewSnapshot>>,
    edge: Mutex<alerts::Edge>,
    /// One computation at a time (the loop, a refresh, a replay, a what-if).
    compute: tokio::sync::Mutex<()>,
    client: reqwest::Client,
}

impl RiskViewManager {
    pub fn new() -> Self {
        Self {
            caches: Mutex::new(Caches::default()),
            latest: Mutex::new(None),
            edge: Mutex::new(alerts::Edge::default()),
            compute: tokio::sync::Mutex::new(()),
            client: history::build_client(),
        }
    }

    fn latest(&self) -> Option<RiskViewSnapshot> {
        self.latest.lock().expect("riskview mutex").clone()
    }
}

impl Default for RiskViewManager {
    fn default() -> Self {
        Self::new()
    }
}

fn now_ms() -> u64 {
    crate::bot::engine::now_ms()
}

/// Start of the current hour: synthetic bars open here.
fn t0(now: u64) -> u64 {
    now - now % HOUR_MS
}

/* ---------------- Exposures from the desk ---------------- */

/// Every open exposure, and the ones without a price yet (rows only).
fn gather(app: &AppHandle) -> (Vec<Exposure>, Vec<ExposureRow>) {
    let mut priced = Vec::new();
    let mut unpriced = Vec::new();
    let bots = app.state::<BotManager>();
    let status = bots.status();
    for pos in &status.open_positions {
        let mark = status
            .marks
            .iter()
            .find(|m| m.signal_id == pos.signal_id && m.bot_kind == pos.bot_kind)
            .map(|m| m.price)
            .filter(|p| p.is_finite() && *p > 0.0);
        let max_loss_pct = bots.config_for(pos.bot_kind).and_then(|c| c.max_loss_pct);
        let e = Exposure {
            id: format!("pos:{}:{}", pos.bot_kind.as_str(), pos.signal_id),
            source: if pos.manual || is_user_trade_id(&pos.signal_id) { Source::Manual } else { Source::Signal },
            label: pos.bot_kind.as_str().to_string(),
            symbol: pos.symbol.clone(),
            market: if pos.bot_kind.uses_futures_market() { MarketKind::Futures } else { MarketKind::Spot },
            live: pos.is_live(),
            scale: 1.0,
            direction: if pos.direction == Direction::Long { Dir::Long } else { Dir::Short },
            capital: pos.capital,
            mark: mark.unwrap_or(0.0),
            body: Body::Position { pos: Box::new(pos.clone()), max_loss_pct },
        };
        match mark {
            Some(_) => priced.push(e),
            None => unpriced.push(row(&e, None, None, false)),
        }
    }
    let live = app.state::<StrategyLiveManager>();
    for (bot, cycle, mark) in app.state::<StrategyManager>().open_cycle_snapshots() {
        let st = live.get(&bot.id);
        let is_live = crate::bot::model::LIVE_TRADING_ENABLED
            && st.as_ref().is_some_and(|s| s.enabled || s.synced_qty != 0.0);
        let scale = match (&st, is_live) {
            (Some(s), true) if s.cycle_factor > 0.0 => s.cycle_factor.min(1.0),
            _ => 1.0,
        };
        let dd = bot.cfg.max_drawdown_pct.filter(|p| *p > 0.0).map(|pct| DdStop {
            pct,
            peak_equity: bot.peak_equity,
            budget: bot.cfg.budget,
            realized: bot.realized_quote,
        });
        let e = Exposure {
            id: format!("sb:{}", bot.id),
            source: match bot.cfg.kind() {
                crate::bot::strategy::model::StrategyKind::Dca => Source::Dca,
                crate::bot::strategy::model::StrategyKind::Grid => Source::Grid,
            },
            label: bot.cfg.name.clone(),
            symbol: bot.cfg.symbol.clone(),
            market: bot.cfg.market,
            live: is_live,
            scale,
            direction: match bot.cfg.side {
                Side::Long => Dir::Long,
                Side::Short => Dir::Short,
                Side::Neutral => Dir::Neutral,
            },
            capital: cycle.core.budget * scale,
            mark: mark.unwrap_or(0.0),
            body: Body::Cycle { cycle: Box::new(cycle), dd },
        };
        match mark {
            Some(_) => priced.push(e),
            None => unpriced.push(row(&e, None, None, false)),
        }
    }
    (priced, unpriced)
}

fn row(e: &Exposure, beta: Option<BetaInfo>, beta_error: Option<String>, priced: bool) -> ExposureRow {
    ExposureRow {
        id: e.id.clone(),
        source: e.source,
        label: e.label.clone(),
        symbol: e.symbol.clone(),
        market: e.market,
        live: e.live,
        direction: e.direction,
        capital: e.capital,
        mark: priced.then_some(e.mark),
        stop: e.stop(),
        liq: e.liq(),
        beta,
        beta_error,
    }
}

/* ---------------- DataHub reads ---------------- */

/// Hourly bars of `[start, end)` from DataHub (paged; one 401 refreshes the
/// session and retries). Err = history error code.
async fn fetch_bars(app: &AppHandle, client: &reqwest::Client, symbol: &str, market: MarketKind, start: u64, end: u64) -> Result<Vec<Bar>, String> {
    let membership = app.state::<MembershipManager>();
    let base = membership.base_url();
    let mut token = membership.fresh_access_token().await.ok_or("historySignIn")?;
    let mut refreshed = false;
    let mut candles = Vec::new();
    for (from, to) in plan_pages(start, end, HOUR_MS) {
        let (from_s, to_s) = (rfc3339(from), rfc3339(to));
        let page = loop {
            match history::fetch_page(client, &base, &token, symbol, market.as_str(), "1h", &from_s, &to_s).await {
                Err(HistoryError::Unauthorized) if !refreshed => {
                    refreshed = true;
                    if !membership.refresh_after_unauthorized(&token).await {
                        return Err("historyUnauthorized".into());
                    }
                    token = membership.access_token().ok_or("historySignIn")?;
                }
                other => break other.map_err(|e| e.to_code())?,
            }
        };
        candles.extend(page.candles);
    }
    let (bars, _) = candles_to_bars(candles, HOUR_MS, now_ms());
    Ok(bars.into_iter().filter(|b| b.open_ms >= start && b.open_ms < end).collect())
}

impl RiskViewManager {
    /// BTCUSDT hourly bars of the last BETA_DAYS on `market` (cached 1 h).
    async fn btc_hourly(&self, app: &AppHandle, market: MarketKind, now: u64) -> Result<Vec<Bar>, String> {
        if let Some((bars, at)) = self.caches.lock().expect("riskview mutex").btc_hourly.get(&market) {
            if now.saturating_sub(*at) < HOUR_MS {
                return Ok(bars.clone());
            }
        }
        let end = t0(now);
        let bars = fetch_bars(app, &self.client, BTC, market, end - beta::BETA_DAYS * 24 * HOUR_MS, end).await?;
        self.caches.lock().expect("riskview mutex").btc_hourly.insert(market, (bars.clone(), now));
        Ok(bars)
    }

    /// The coin's beta to BTC on its market (cached; BTC itself is 1).
    async fn beta(&self, app: &AppHandle, market: MarketKind, symbol: &str, now: u64) -> Result<BetaInfo, String> {
        if symbol == BTC {
            return Ok(BetaInfo::unit());
        }
        let key = (market, symbol.to_string());
        if let Some((res, at)) = self.caches.lock().expect("riskview mutex").betas.get(&key) {
            let ttl = if res.is_ok() { BETA_TTL_MS } else { BETA_RETRY_MS };
            if now.saturating_sub(*at) < ttl {
                return res.clone();
            }
        }
        let res = async {
            let btc = self.btc_hourly(app, market, now).await?;
            let end = t0(now);
            let coin = fetch_bars(app, &self.client, symbol, market, end - beta::BETA_DAYS * 24 * HOUR_MS, end).await?;
            beta::estimate(&coin, &btc).ok_or_else(|| "betaShort".to_string())
        }
        .await;
        // Signed out is not cached: signing in should work at once.
        if res.as_ref().err().map(String::as_str) != Some("historySignIn") {
            self.caches.lock().expect("riskview mutex").betas.insert(key, (res.clone(), now));
        }
        res
    }

    /// Betas of every distinct (market, symbol) in `exps`.
    async fn betas_for(&self, app: &AppHandle, exps: &[&Exposure], now: u64) -> HashMap<Key, Result<BetaInfo, String>> {
        let mut out = HashMap::new();
        for e in exps {
            let key = (e.market, e.symbol.clone());
            if out.contains_key(&key) {
                continue;
            }
            let b = self.beta(app, e.market, &e.symbol, now).await;
            out.insert(key, b);
        }
        out
    }

    /// The window's hourly bars of `symbol` on `market` (cached).
    async fn window_bars(&self, app: &AppHandle, w: &replay::Window, market: MarketKind, symbol: &str, now: u64) -> Result<Vec<Bar>, String> {
        let key = (w.id, market, symbol.to_string());
        if let Some((res, at)) = self.caches.lock().expect("riskview mutex").windows.get(&key) {
            if res.is_ok() || now.saturating_sub(*at) < WINDOW_RETRY_MS {
                return res.clone();
            }
        }
        let (start, end) = w.range_ms();
        let res = fetch_bars(app, &self.client, symbol, market, start, end).await;
        if res.as_ref().err().map(String::as_str) != Some("historySignIn") {
            self.caches.lock().expect("riskview mutex").windows.insert(key, (res.clone(), now));
        }
        res
    }
}

/* ---------------- Computations ---------------- */

fn split<'a>(exps: &'a [Exposure], betas: &HashMap<Key, Result<BetaInfo, String>>, live: bool) -> Vec<(&'a Exposure, Option<BetaInfo>)> {
    exps.iter()
        .filter(|e| e.live == live)
        .map(|e| (e, betas.get(&(e.market, e.symbol.clone())).and_then(|b| b.as_ref().ok().copied())))
        .collect()
}

/// Side word and coin for a Telegram line.
fn telegram_fill(book: &BookStress) -> (Option<String>, Option<&'static str>, Option<f64>) {
    let c = &book.concentration;
    let side = c.dominant.map(|d| match d {
        Dir::Long => "long",
        Dir::Short => "short",
        Dir::Neutral => "neutral",
    });
    let loss = book
        .scenarios
        .iter()
        .find(|s| s.btc_move_pct == stress::CHECK_SHOCK)
        .map(|s| (-s.book.pnl).max(0.0));
    (c.top_coin.clone(), side, loss)
}

impl RiskViewManager {
    /// Recomputes the snapshot, stores it, and sends one Telegram line per
    /// fresh threshold crossing.
    async fn refresh(&self, app: &AppHandle) -> RiskViewSnapshot {
        let _guard = self.compute.lock().await;
        let now = now_ms();
        let (exps, unpriced) = gather(app);
        let refs: Vec<&Exposure> = exps.iter().collect();
        let betas = self.betas_for(app, &refs, now).await;
        let thresholds = app.state::<crate::risk::RiskManager>().view_thresholds();
        let start = t0(now);
        let mut paper = stress::book_stress(&split(&exps, &betas, false), false, &thresholds, start);
        let mut real = stress::book_stress(&split(&exps, &betas, true), true, &thresholds, start);
        let (fresh_paper, fresh_real) = {
            let mut edge = self.edge.lock().expect("riskview mutex");
            (edge.apply(false, &mut paper.checks, now), edge.apply(true, &mut real.checks, now))
        };
        let mut rows: Vec<ExposureRow> = exps
            .iter()
            .map(|e| {
                let b = betas.get(&(e.market, e.symbol.clone()));
                row(e, b.and_then(|r| r.as_ref().ok().copied()), b.and_then(|r| r.as_ref().err().cloned()), true)
            })
            .collect();
        rows.extend(unpriced.iter().cloned());
        let snap = RiskViewSnapshot {
            computed_at: now,
            sample: false,
            beta_days: beta::BETA_DAYS,
            exposures: rows,
            paper,
            real,
            thresholds,
            unpriced: unpriced.len() as u32,
        };
        *self.latest.lock().expect("riskview mutex") = Some(snap.clone());
        for (book, fresh) in [(&snap.paper, fresh_paper), (&snap.real, fresh_real)] {
            let (coin, side, loss) = telegram_fill(book);
            for c in fresh {
                let text = alerts::line(&c, coin.as_deref(), side, loss);
                crate::telegram::send_line_for(app, crate::telegram::notify::Category::KillSwitch, &text, book.live).await;
            }
        }
        snap
    }

    async fn replay(&self, app: &AppHandle) -> ReplayReport {
        let _guard = self.compute.lock().await;
        let now = now_ms();
        let (exps, _) = gather(app);
        let refs: Vec<&Exposure> = exps.iter().collect();
        let betas = self.betas_for(app, &refs, now).await;
        let paper = split(&exps, &betas, false);
        let real = split(&exps, &betas, true);
        // BTC's path for beta-estimated coins: futures when the book trades
        // futures (the two markets move together; one reference keeps it simple).
        let btc_market = if exps.iter().any(|e| e.market == MarketKind::Futures) || exps.is_empty() {
            MarketKind::Futures
        } else {
            MarketKind::Spot
        };
        let start = t0(now);
        let mut windows = Vec::new();
        'win: for w in replay::WINDOWS {
            let (ws, we) = w.range_ms();
            let btc = match self.window_bars(app, &w, btc_market, BTC, now).await {
                Ok(b) => b,
                Err(e) => {
                    windows.push(replay::ReplayWindow::unavailable(&w, e));
                    continue;
                }
            };
            let (btc, ok) = replay::served(&btc, ws, we);
            if !ok {
                windows.push(replay::ReplayWindow::unavailable(&w, "replayBtcShort".into()));
                continue;
            }
            // Every coin's own window, read before walking.
            let mut coins: BTreeMap<Key, Option<Vec<Bar>>> = BTreeMap::new();
            for e in &exps {
                let key = (e.market, e.symbol.clone());
                if coins.contains_key(&key) {
                    continue;
                }
                let got = if e.symbol == BTC && e.market == btc_market {
                    Ok(btc.clone())
                } else {
                    self.window_bars(app, &w, e.market, &e.symbol, now).await
                };
                match got {
                    Ok(bars) => {
                        let (inside, ok) = replay::served(&bars, ws, we);
                        coins.insert(key, ok.then_some(inside));
                    }
                    // The symbol did not exist then (or is unknown): no
                    // history, the coin follows beta x BTC.
                    Err(code) if code.starts_with("historyBadRequest") || code == "historyNotAvailable" => {
                        coins.insert(key, None);
                    }
                    // Anything else (network, rate limit, upstream) would
                    // pass a transient failure off as an estimate.
                    Err(code) => {
                        windows.push(replay::ReplayWindow::unavailable(&w, code));
                        continue 'win;
                    }
                }
            }
            let coin_of = |e: &Exposure| coins.get(&(e.market, e.symbol.clone())).cloned().flatten();
            let (mv, low) = replay::btc_summary(&btc).unzip();
            windows.push(replay::ReplayWindow {
                id: w.id,
                start_ms: ws,
                end_ms: we,
                status: "ok",
                error: None,
                btc_move_pct: mv,
                btc_low_pct: low,
                paper: (!paper.is_empty()).then(|| replay::replay_book(&paper, &btc, coin_of, start)),
                real: (!real.is_empty()).then(|| replay::replay_book(&real, &btc, coin_of, start)),
            });
        }
        ReplayReport { computed_at: now, sample: false, windows }
    }

    async fn whatif(&self, app: &AppHandle, cand: whatif::Candidate) -> whatif::WhatIf {
        let _guard = self.compute.lock().await;
        let now = now_ms();
        let start = t0(now);
        let move_pct = stress::CHECK_SHOCK;
        let (exps, _) = gather(app);
        let refs: Vec<&Exposure> = exps.iter().filter(|e| !e.live).collect();
        let betas = self.betas_for(app, &refs, now).await;
        let book = split(&exps, &betas, false);
        let brief = |b: &[(&Exposure, Option<BetaInfo>)]| {
            let r = stress::scenario(b, move_pct, start);
            whatif::Brief {
                pnl: r.pnl,
                pct_of_capital: r.pct_of_capital,
                capital: b.iter().map(|(e, _)| e.capital).sum(),
                included: r.included,
                excluded: r.excluded,
            }
        };
        let before = brief(&book);
        let max_lev = app.state::<crate::risk::RiskManager>().limits().max_leverage;
        let mut out = whatif::WhatIf {
            btc_move_pct: move_pct,
            live: false,
            before: Some(before.clone()),
            after: None,
            candidate_pnl: None,
            candidate_beta: None,
            error: None,
            sample: false,
        };
        let e = match cand.exposure(max_lev, start) {
            Ok(e) => e,
            Err(code) => {
                out.error = Some(code);
                return out;
            }
        };
        let b = match self.beta(app, cand.market(), &cand.symbol(), now).await {
            Ok(b) => b,
            Err(code) => {
                out.error = Some(code);
                return out;
            }
        };
        let shock = e.shock(b.beta, move_pct, start);
        let entry = e.mtm_now();
        let cand_pnl = shock.pnl + entry;
        out.candidate_beta = Some(b);
        out.candidate_pnl = Some(cand_pnl);
        let capital = before.capital + e.capital;
        let pnl = before.pnl + cand_pnl;
        out.after = Some(whatif::Brief {
            pnl,
            pct_of_capital: (capital > 0.0).then(|| pnl / capital * 100.0),
            capital,
            included: before.included + 1,
            excluded: before.excluded,
        });
        out
    }
}
