//! The engine's order surface, one method per question, routed by the
//! credential's exchange: Binance to its own layer (`live_orders.rs`),
//! Bybit / OKX to ccxt (`venue/ccxt.rs`). Callers hand over the
//! vaulted credential, never a bare key/secret pair, so a key can only ever
//! reach the exchange it belongs to.

use super::model::{BinanceKeyCheckError as E, FuturesAccount};
use super::providers::binance_fills::UserTrade;
use super::providers::binance_parse::{OrderFill, OrderState, RecentOrder};
use super::providers::binance_requests::Protective;
use super::providers::binance_rules::SymbolRules;
use super::venue::ccxt::{is_ccxt_venue, sandbox_from_env, venue_client_id, CCXT_VENUES};
use super::ExchangeManager;
use crate::vault::model::ExchangeCredential;

/// Exchanges with a real-money order path.
pub fn has_order_path(exchange_id: &str) -> bool {
    exchange_id == "binance" || is_ccxt_venue(exchange_id)
}

/// Venues whose order path ran end to end on the exchange's own test network
/// (`testnet_tests.rs`: entry, stop, move, add, reduce, close, clean symbol).
/// Real money can be switched ON only here. Binance passed on 2026-10-05.
/// Bybit and OKX have not run, and the 2026-10-08 audit found their position
/// mode read, cancel sweep, contract sizing and breakeven move broken or
/// missing. Adding a venue is a release decision taken after its dry run
/// passes, never a setting. Closing a position is never gated by this list.
pub const DRY_RUN_PASSED: [&str; 1] = ["binance"];

/// The refusal for switching real money on where no dry run passed
/// (`errors.liveVenueNotDryRun`, `strategy.errors.liveVenueNotDryRun`).
pub const VENUE_NOT_DRY_RUN: &str = "liveVenueNotDryRun";

/// Whether real money may be switched on for `venue` in this process: it has
/// an order path and passed its dry run, or the app runs against the venues'
/// test networks (`ALEPH_EDGE_VENUE_SANDBOX=1`) to make that dry run.
pub fn live_venue_allowed(venue: &str) -> bool {
    live_venue_allowed_in(venue, sandbox_from_env())
}

/// `live_venue_allowed` with the sandbox switch passed in (pure, tested).
pub fn live_venue_allowed_in(venue: &str, sandbox: bool) -> bool {
    has_order_path(venue) && (DRY_RUN_PASSED.contains(&venue) || sandbox)
}

/// `Err("liveVenueNotDryRun|<venue>")` where real money may not be switched
/// on (`sandbox` = `ALEPH_EDGE_VENUE_SANDBOX=1`, passed in by the caller).
pub fn live_venue_check_in(venue: &str, sandbox: bool) -> Result<(), String> {
    if live_venue_allowed_in(venue, sandbox) {
        Ok(())
    } else {
        Err(format!("{VENUE_NOT_DRY_RUN}|{venue}"))
    }
}

/// The venues real money can be switched on for now, for the UI.
pub fn live_venues() -> Vec<String> {
    let sandbox = sandbox_from_env();
    std::iter::once("binance")
        .chain(CCXT_VENUES)
        .filter(|v| live_venue_allowed_in(v, sandbox))
        .map(str::to_string)
        .collect()
}

/// How a breakeven move reaches the exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakevenMove {
    /// Binance: a reduce-only QUANTITY stop placed beside the original
    /// closePosition stop, then the original cancelled (never a window
    /// without a stop).
    StopBeside,
    /// Bybit / OKX: the whole-position stop is changed in place
    /// (`move_stop`), nothing cancelled. Bybit has one stop slot per
    /// position, so the Binance plan's cancel would delete the moved stop.
    MoveInPlace,
}

/// The breakeven move for `exchange_id` in this process, or None when the
/// breakeven exists only in the app. Behind the same gate as real money:
/// Bybit / OKX move on the exchange only where real money may be switched
/// on (`live_venue_allowed`: the dry-run list, or the sandbox).
pub fn breakeven_move(exchange_id: &str) -> Option<BreakevenMove> {
    breakeven_move_in(exchange_id, sandbox_from_env())
}

/// `breakeven_move` with the sandbox switch passed in (pure, tested).
pub fn breakeven_move_in(exchange_id: &str, sandbox: bool) -> Option<BreakevenMove> {
    match exchange_id {
        "binance" => Some(BreakevenMove::StopBeside),
        v if is_ccxt_venue(v) && live_venue_allowed_in(v, sandbox) => Some(BreakevenMove::MoveInPlace),
        _ => None,
    }
}

/// The client id an order sent with `client_id` carries ON the venue: ccxt
/// venues rewrite it (`venue_client_id`, cut to OKX's 32 characters),
/// Binance keeps it. Whatever later recognises our own orders in the venue's
/// history (reconcile) must compare against THIS, not the id we passed in.
pub fn venue_order_client_id(exchange_id: &str, client_id: &str) -> String {
    if is_ccxt_venue(exchange_id) {
        venue_client_id(client_id)
    } else {
        client_id.to_string()
    }
}

fn unsupported() -> E {
    E::Rejected { code: 0, msg: "no order path for this exchange".into() }
}

macro_rules! route {
    ($cred:expr, binance => $bn:expr, ccxt => $cx:expr) => {
        if $cred.exchange_id == "binance" {
            $bn
        } else if is_ccxt_venue(&$cred.exchange_id) {
            $cx
        } else {
            Err(unsupported())
        }
    };
}

impl ExchangeManager {
    /// Trading rules for `symbol` on `exchange_id`. Binance reads them
    /// publicly; ccxt venues need the credential only to pick the client.
    pub async fn symbol_rules(&self, cred: &ExchangeCredential, symbol: &str) -> Result<SymbolRules, E> {
        route!(cred, binance => self.bn_symbol_rules(symbol).await, ccxt => self.venues.symbol_rules(cred, symbol).await)
    }

    pub async fn futures_account(&self, cred: &ExchangeCredential) -> Result<FuturesAccount, E> {
        route!(cred,
            binance => self.bn_futures_account(&cred.api_key, &cred.api_secret).await,
            ccxt => self.venues.futures_account(cred).await)
    }

    /// Hedge (two-sided) position mode. Binance sets it per account; Bybit
    /// per symbol, so the symbol about to be traded or closed is the one
    /// asked about.
    pub async fn hedge_mode(&self, cred: &ExchangeCredential, symbol: &str) -> Result<bool, E> {
        route!(cred,
            binance => self.bn_hedge_mode(&cred.api_key, &cred.api_secret).await,
            ccxt => self.venues.hedge_mode(cred, symbol).await)
    }

    pub async fn set_leverage(&self, cred: &ExchangeCredential, symbol: &str, leverage: u8) -> Result<(), E> {
        route!(cred,
            binance => self.bn_set_leverage(&cred.api_key, &cred.api_secret, symbol, leverage).await,
            ccxt => self.venues.set_leverage(cred, symbol, leverage).await)
    }

    pub async fn set_isolated(&self, cred: &ExchangeCredential, symbol: &str) -> Result<(), E> {
        route!(cred,
            binance => self.bn_set_isolated(&cred.api_key, &cred.api_secret, symbol).await,
            ccxt => self.venues.set_isolated(cred, symbol).await)
    }

    pub async fn open_order_count(&self, cred: &ExchangeCredential, symbol: &str) -> Result<usize, E> {
        route!(cred,
            binance => self.bn_open_order_count(&cred.api_key, &cred.api_secret, symbol).await,
            ccxt => self.venues.open_order_count(cred, symbol).await)
    }

    pub async fn order_by_client_id(&self, cred: &ExchangeCredential, symbol: &str, client_id: &str) -> Result<OrderState, E> {
        route!(cred,
            binance => self.bn_order_by_client_id(&cred.api_key, &cred.api_secret, symbol, client_id).await,
            ccxt => self.venues.order_by_client_id(cred, symbol, client_id).await)
    }

    pub async fn recent_orders(&self, cred: &ExchangeCredential, symbol: &str) -> Result<Vec<RecentOrder>, E> {
        route!(cred,
            binance => self.bn_recent_orders(&cred.api_key, &cred.api_secret, symbol).await,
            ccxt => self.venues.recent_orders(cred, symbol).await)
    }

    pub async fn open_market(&self, cred: &ExchangeCredential, symbol: &str, long: bool, qty: f64, client_id: &str) -> Result<OrderFill, E> {
        route!(cred,
            binance => self.bn_open_market(&cred.api_key, &cred.api_secret, symbol, long, qty, client_id).await,
            ccxt => self.venues.open_market(cred, symbol, long, qty, client_id).await)
    }

    pub async fn reduce_market(&self, cred: &ExchangeCredential, symbol: &str, long: bool, qty: f64) -> Result<OrderFill, E> {
        route!(cred,
            binance => self.bn_reduce_market(&cred.api_key, &cred.api_secret, symbol, long, qty).await,
            ccxt => self.venues.reduce_market(cred, symbol, long, qty).await)
    }

    pub async fn reduce_market_all(&self, cred: &ExchangeCredential, symbol: &str, long: bool, qty: f64) -> Result<OrderFill, E> {
        route!(cred,
            binance => self.bn_reduce_market_all(&cred.api_key, &cred.api_secret, symbol, long, qty).await,
            ccxt => self.venues.reduce_market_all(cred, symbol, long, qty).await)
    }

    pub async fn place_protective(&self, cred: &ExchangeCredential, symbol: &str, long: bool, kind: Protective, trigger: f64) -> Result<i64, E> {
        route!(cred,
            binance => self.bn_place_protective(&cred.api_key, &cred.api_secret, symbol, long, kind, trigger).await,
            ccxt => self.venues.place_protective(cred, symbol, long, kind, trigger).await)
    }

    /// The Binance breakeven stop (`BreakevenMove::StopBeside`). The ccxt
    /// venues' stops are position-level: they move in place (`move_stop`).
    pub async fn place_reduce_stop(&self, cred: &ExchangeCredential, symbol: &str, long: bool, qty: f64, trigger: f64) -> Result<i64, E> {
        route!(cred,
            binance => self.bn_place_reduce_stop(&cred.api_key, &cred.api_secret, symbol, long, qty, trigger).await,
            ccxt => Err(E::Rejected { code: 0, msg: "quantity stops are Binance only; this venue moves its stop".into() }))
    }

    /// Moves the whole-position stop `algo_id` to `trigger` in place
    /// (`BreakevenMove::MoveInPlace`), read back; the id the stop now
    /// carries. Binance moves by placing beside and cancelling instead.
    pub async fn move_stop(&self, cred: &ExchangeCredential, symbol: &str, algo_id: i64, trigger: f64) -> Result<i64, E> {
        route!(cred,
            binance => Err(E::Rejected { code: 0, msg: "Binance moves a stop by place-then-cancel".into() }),
            ccxt => self.venues.move_stop(cred, symbol, algo_id, trigger).await)
    }

    /// Cancels a protective order. ccxt venues need the symbol; Binance
    /// cancels by algo id alone.
    pub async fn cancel_algo(&self, cred: &ExchangeCredential, symbol: &str, algo_id: i64) -> Result<(), E> {
        route!(cred,
            binance => self.bn_cancel_algo(&cred.api_key, &cred.api_secret, algo_id).await,
            ccxt => self.venues.cancel_protective(cred, symbol, algo_id).await)
    }

    pub async fn algo_status(&self, cred: &ExchangeCredential, symbol: &str, algo_id: i64) -> Result<String, E> {
        route!(cred,
            binance => self.bn_algo_status(&cred.api_key, &cred.api_secret, algo_id).await,
            ccxt => self.venues.algo_status(cred, symbol, algo_id).await)
    }

    pub async fn cancel_symbol_orders(&self, cred: &ExchangeCredential, symbol: &str) -> Result<(), E> {
        route!(cred,
            binance => self.bn_cancel_symbol_orders(&cred.api_key, &cred.api_secret, symbol).await,
            ccxt => self.venues.cancel_symbol_orders(cred, symbol).await)
    }

    /// (can trade futures, can withdraw) for a ccxt venue's key.
    pub async fn venue_key_permissions(&self, cred: &ExchangeCredential) -> Result<(bool, bool), E> {
        if !is_ccxt_venue(&cred.exchange_id) {
            return Err(unsupported());
        }
        self.venues.key_permissions(cred).await
    }

    pub async fn user_trades(&self, cred: &ExchangeCredential, symbol: &str, start_ms: u64) -> Result<Vec<UserTrade>, E> {
        route!(cred,
            binance => self.bn_user_trades(&cred.api_key, &cred.api_secret, symbol, start_ms).await,
            ccxt => self.venues.user_trades(cred, symbol, start_ms).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::providers::binance_requests::entry_client_id;

    #[test]
    fn real_money_switches_on_only_where_a_dry_run_passed() {
        assert!(live_venue_allowed_in("binance", false));
        for venue in ["bybit", "okx"] {
            assert!(has_order_path(venue), "{venue} keeps its order path: closing stays possible");
            assert!(!live_venue_allowed_in(venue, false), "{venue}: no dry run passed");
            assert_eq!(live_venue_check_in(venue, false), Err(format!("liveVenueNotDryRun|{venue}")));
        }
        assert_eq!(live_venue_check_in("binance", false), Ok(()));
    }

    #[test]
    fn the_sandbox_dry_run_may_switch_any_order_venue_on() {
        for venue in ["binance", "bybit", "okx"] {
            assert!(live_venue_allowed_in(venue, true), "{venue}");
        }
        // No order path at all stays refused, sandbox or not. Bitget's was
        // removed on 2026-10-09 (no key verifier, no dry run).
        for venue in ["bitget", "mexc", "coindcx", ""] {
            assert!(!live_venue_allowed_in(venue, true), "{venue}");
            assert!(!live_venue_allowed_in(venue, false), "{venue}");
        }
    }

    #[test]
    fn the_dry_run_list_is_binance_alone_today() {
        // Lifting the gate for a venue is a release decision after its dry
        // run: this test fails on purpose when the list changes.
        assert_eq!(DRY_RUN_PASSED, ["binance"]);
    }

    #[test]
    fn breakeven_moves_follow_the_real_money_gate() {
        for sandbox in [false, true] {
            assert_eq!(breakeven_move_in("binance", sandbox), Some(BreakevenMove::StopBeside));
            for venue in ["bitget", "mexc", ""] {
                assert_eq!(breakeven_move_in(venue, sandbox), None, "{venue}");
            }
        }
        for venue in ["bybit", "okx"] {
            // No dry run passed: the breakeven stays in the app.
            assert_eq!(breakeven_move_in(venue, false), None, "{venue}");
            // The sandbox dry run moves the stop on the test network.
            assert_eq!(breakeven_move_in(venue, true), Some(BreakevenMove::MoveInPlace), "{venue}");
        }
    }

    #[test]
    fn venue_client_ids_are_what_the_venue_stores() {
        let entry = entry_client_id("5ded7193-2335-436c-ba17-9bd2b5f8e87e");
        assert_eq!(entry.len(), 34);
        assert_eq!(venue_order_client_id("binance", &entry), entry, "Binance keeps the full id");
        for venue in ["bybit", "okx"] {
            let stored = venue_order_client_id(venue, &entry);
            assert_eq!(stored, "ae5ded71932335436cba179bd2b5f8e8", "{venue}: cut to 32");
            // ccxt maps the id again on a lookup: the mapping must be stable.
            assert_eq!(venue_order_client_id(venue, &stored), stored, "{venue}");
        }
    }
}
