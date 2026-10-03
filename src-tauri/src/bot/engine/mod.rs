//! The bot tick: close phase (management plan, horizon, user max-loss) then
//! open phase (new signals through gates, fill model and §5.4 prechecks).
//! Fills are simulated at the live market price while `LIVE_TRADING_ENABLED`
//! is false — the decision pipeline, risk limits, and persistence are real.
//!
//! Layout: `entry` (open phase), `book` (close phase + persistence),
//! `management` / `geometry` / `pnl` (pure rules), `btc_break` (entry-only
//! regime guard), `precheck` + `filters` (skip reasons), and the real-order
//! path, compile-time disabled: `live` (gate + entry), `live_manage`
//! (breakeven/partial on the exchange), `live_close` (real-fill settlement),
//! `reconcile` (exits the exchange made while the app was away).
//!
//! Tauri-free core (2026-09-22): `entry_rules`, `precheck_rules` and `record`
//! hold the open phase's decisions, the precheck verdicts and the trade
//! records; together with `geometry`, `management`, `pnl`, `sizing`,
//! `take_profit`, `filters` and `btc_break` they import no Tauri and do no
//! I/O. The headless paper runner (`crates/aleph-paper-runner`) compiles
//! exactly these files — keep them free of Tauri, managers and I/O.

pub mod book;
pub mod btc_break;
pub mod entry;
pub mod entry_rules;
pub mod filters;
pub mod geometry;
pub mod live;
pub mod live_close;
pub mod live_manage;
pub mod management;
pub mod pnl;
pub mod precheck;
pub mod precheck_rules;
pub mod reconcile;
pub mod record;
pub mod sizing;
pub mod take_profit;

pub(crate) use book::force_close_kinds;

use tauri::{AppHandle, Manager};

use crate::exchange::ExchangeManager;
use crate::market::MarketManager;
use crate::membership::MembershipManager;
use crate::risk::RiskManager;
use crate::store::StoreManager;

use super::model::{BotKind, LIVE_TRADING_ENABLED};
use super::BotManager;

/// One engine pass. Called from the manager's loop every few seconds.
pub async fn tick(app: &AppHandle) {
    let regime = fetch_btc_regime(app).await;
    app.state::<BotManager>().set_btc_regime(regime);
    reconcile::reconcile_phase(app).await;
    book::close_phase(app).await;
    kill_switch_phase(app).await;
    // DCA / Grid paper cycles (desktop only), before new signal entries.
    super::strategy::engine::strategy_phase(app).await;
    entry::open_phase(app, regime).await;
}

/// Keeps the BTC regime read from launch on, every `REGIME_POLL`.
///
/// The engine tick reads it too, but that loop only starts once a bot or an
/// open position exists: with nothing running the regime stayed at its
/// initial Unknown, and the risk page, status bar and sidebar said
/// "Unknown" while Sentinel's feed was answering fine (owner, 3 Oct).
pub fn spawn_regime_poller(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let regime = fetch_btc_regime(&app).await;
            app.state::<BotManager>().set_btc_regime(regime);
            tokio::time::sleep(REGIME_POLL).await;
        }
    });
}

/// MarketManager caches the macro read for about as long, so a faster poll
/// would only re-read the cache.
const REGIME_POLL: std::time::Duration = std::time::Duration::from_secs(20);

/// Reads Sentinel's BTC regime declaration (cached ~20s by MarketManager).
/// A failed fetch is Unknown, never assumed Normal.
async fn fetch_btc_regime(app: &AppHandle) -> btc_break::BtcRegime {
    let base = app.state::<MembershipManager>().base_url();
    match app.state::<MarketManager>().btc(&base).await {
        Ok(state) => btc_break::classify(Some(&state)),
        Err(_) => btc_break::classify(None),
    }
}

/// Live price on the bot's market: futures for futures-market bots, spot
/// otherwise. No cross-market fallback: a futures position managed on the
/// spot series can close on a price its market never printed (and on a
/// testnet build the spot feed is production's).
pub(crate) async fn live_price(
    exchange: &ExchangeManager,
    exchange_id: &str,
    symbol: &str,
    kind: BotKind,
) -> Option<f64> {
    let status = exchange.status(exchange_id, symbol).await;
    if kind.uses_futures_market() {
        status.futures_price
    } else {
        status.spot_price
    }
}

/// Reference price for CLOSING a position. A live close sizes and fills from
/// the exchange itself; the price is only the book's fallback exit when the
/// exchange is already flat with no fill to read. A missing public quote
/// therefore never keeps a real position open (an unprotected one least of
/// all): it falls back to the entry. Paper closes still need a real quote.
pub(crate) fn close_reference(quote: Option<f64>, live: bool, entry: f64) -> Option<f64> {
    match quote {
        Some(p) => Some(p),
        None if live && entry.is_finite() && entry > 0.0 => Some(entry),
        None => None,
    }
}

pub(crate) async fn close_price(exchange: &ExchangeManager, pos: &crate::bot::model::OpenPosition) -> Option<f64> {
    let quote = live_price(exchange, &pos.exchange_id, &pos.symbol, pos.bot_kind).await;
    close_reference(quote, pos.is_live(), pos.entry)
}

/// Trips the daily loss kill-switch when today's realized net PnL breaches the
/// level's limit (as % of the simulated balance). All bots stop; per PRD §5.3
/// the default is to CLOSE open positions (user preference `close_on_stop`).
/// Restart unlocks next UTC day.
async fn kill_switch_phase(app: &AppHandle) {
    let bots = app.state::<BotManager>();
    // Restored positions are managed before any bot starts; the daily stop
    // guards them too. Strategy (DCA/Grid) bots are outside it (owner
    // decision 2026-10-01): their own budget cap and portfolio breaker apply.
    let idle = !bots.any_running() && bots.open_count() == 0;
    if idle {
        return;
    }
    let risk = app.state::<RiskManager>();
    if bots.kill_switch_tripped() {
        // The close-all runs once at the trip; a position it could not close
        // then (no price, a live close that failed, claimed by a veto that
        // failed) is closed on a later tick, not left trading past the stop.
        if retry_daily_stop_close(true, risk.close_on_stop(), bots.open_count()) {
            force_close_kinds(app, "dailyStop", &SIGNAL_KINDS).await;
        }
        return;
    }
    let Ok(dir) = app.path().app_data_dir() else {
        return;
    };
    // Real money is judged on real trades against the real wallet; the
    // simulation on simulated trades against the balance the user declared.
    // One total over both let paper wins hide real losses (and a declared
    // 1000 against a 200 USDT wallet made "2%" mean 10%).
    let live_mode = live_mode_active(&bots);
    let Ok(stats) = app.state::<StoreManager>().stats_for(&dir, live_mode) else {
        return;
    };
    // Signal bots only. A no-stop DCA ladder routinely sits deeper than a
    // 2% daily line; counting it here closed cycles the preset holds.
    let today_pnl = stats.today_pnl_quote;
    let balance = if live_mode {
        match live_equity(app).await {
            // Start-of-day equity: today's realized result added back.
            Some(wallet) => wallet - stats.today_pnl_quote,
            None => risk.balance(),
        }
    } else {
        risk.balance()
    };
    if balance <= 0.0 {
        return;
    }
    // The EFFECTIVE stop: the user's own tolerance when it is stricter than the
    // level cap, the level cap otherwise.
    let limit_pct = risk.effective_daily_loss_pct();
    if today_pnl <= -(balance * limit_pct / 100.0) {
        bots.trip_kill_switch(app);
        if risk.close_on_stop() {
            force_close_kinds(app, "dailyStop", &SIGNAL_KINDS).await;
        }
    }
}

/// The bots the daily stop covers (strategy bots are outside it).
const SIGNAL_KINDS: [BotKind; 3] = [BotKind::Futures, BotKind::Spot, BotKind::Pump];

/// Whether a tick after the trip must close again: the stop tripped, the
/// user's preference is to close, and signal positions are still open. The
/// book holds signal-bot positions only (strategy bots keep their own).
pub fn retry_daily_stop_close(tripped: bool, close_on_stop: bool, open_positions: usize) -> bool {
    tripped && close_on_stop && open_positions > 0
}

/// Real money is in play: a live position is open, or a live bot runs.
/// Strategy (DCA/Grid) bots never count: they cannot be live in v1.
fn live_mode_active(bots: &BotManager) -> bool {
    LIVE_TRADING_ENABLED
        && (bots.positions_snapshot().iter().any(|p| p.is_live())
            || [BotKind::Futures, BotKind::Spot, BotKind::Pump]
                .into_iter()
                .filter_map(|k| bots.running_config(k))
                .any(|c| c.live))
}

/// The Binance futures wallet balance, cached a minute (the kill switch runs
/// every tick). None when the vault is locked or Binance does not answer.
async fn live_equity(app: &AppHandle) -> Option<f64> {
    const TTL_MS: u64 = 60_000;
    let bots = app.state::<BotManager>();
    let now = now_ms();
    if let Some((value, at)) = bots.cached_equity() {
        if now.saturating_sub(at) < TTL_MS {
            return Some(value);
        }
    }
    let cred = app.state::<crate::vault::VaultManager>().credential("binance")?;
    let account = app
        .state::<ExchangeManager>()
        .futures_account(&cred.api_key, &cred.api_secret)
        .await
        .ok()?;
    bots.cache_equity(account.total_wallet_balance, now);
    Some(account.total_wallet_balance)
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod age_gate_tests;
#[cfg(test)]
mod entry_tests;
#[cfg(test)]
mod filter_tests;
#[cfg(test)]
mod liquidation_tests;
#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod sizing_tests;
#[cfg(test)]
mod take_profit_tests;
#[cfg(test)]
mod test_fixtures;
#[cfg(test)]
mod tests;
