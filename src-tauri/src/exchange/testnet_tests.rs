//! Test-network dry run of every order call the live bots use, one venue
//! per run. Ignored by default; it runs only with test-network keys in the
//! environment and never against a real account:
//!
//!   ALEPH_TESTNET_VENUE=binance|bybit|okx ALEPH_TESTNET_KEY=... \
//!     ALEPH_TESTNET_SECRET=... [ALEPH_TESTNET_PASSPHRASE=... (OKX)] \
//!     cargo test --features live testnet_dry_run -- --ignored --nocapture
//!
//! Binance: the futures base is forced to the testnet, and the run refuses
//! to send anything if it resolves to production. Bybit / OKX: the run sets
//! `ALEPH_EDGE_VENUE_SANDBOX=1` before the first client exists, so ccxt talks
//! to Bybit's testnet / OKX's demo trading (`x-simulated-trading: 1`). Full
//! steps and key setup: docs/venue-dry-run.md.
//!
//! Steps, in the order a real-money bot uses them: account read (flat start,
//! one-way mode), symbol rules, isolated + leverage, entry by client id, the
//! entry found by that id, a lost entry recognised as ours from the venue's
//! own order history (reconcile), the stop, the breakeven move as the engine
//! makes it on this venue, add + reduce, cancelling the stop by id, the
//! confirmed close, the cancel sweep, our ids in the closed-order history,
//! and Close all over two symbols. It ends with a PASS / FAIL table and
//! fails if any step failed. Whatever happens, it flattens and sweeps both
//! symbols before it returns.
//!
//! A venue joins `exchange::DRY_RUN_PASSED` only after every row reads PASS
//! on its test network (a release decision, never a setting).

use super::model::{BinanceKeyCheckError, FuturesAccount};
use super::providers::binance_requests::{entry_client_id, Protective};
use super::providers::binance_rules::{quantize_price, quantize_qty, SymbolRules};
use super::{breakeven_move, venue_order_client_id, BreakevenMove, ExchangeManager};
use crate::vault::model::ExchangeCredential;

const SYMBOL: &str = "BTCUSDT";
const SECOND: &str = "ETHUSDT";

fn held(acc: &FuturesAccount, symbol: &str) -> f64 {
    acc.positions.iter().find(|p| p.symbol == symbol).map_or(0.0, |p| p.position_amt)
}

fn entry_px(acc: &FuturesAccount, symbol: &str) -> Option<f64> {
    acc.positions.iter().find(|p| p.symbol == symbol).map(|p| p.entry_price).filter(|p| *p > 0.0)
}

/// About 130 USDT (or the venue minimum), never below one lot.
fn test_qty(rules: &SymbolRules, price: f64) -> f64 {
    let q = quantize_qty((rules.min_notional.max(100.0) * 1.3) / price, rules.lot);
    if q < rules.lot.min_qty {
        rules.lot.min_qty
    } else {
        q
    }
}

#[derive(Default)]
struct Report {
    rows: Vec<(String, bool, String)>,
    /// Past the flat-start check: anything on the two symbols is the run's.
    started: bool,
}

impl Report {
    fn step(&mut self, name: &str, ok: bool, detail: impl Into<String>) -> bool {
        let detail = detail.into();
        println!("{} {name}: {detail}", if ok { "PASS" } else { "FAIL" });
        self.rows.push((name.to_string(), ok, detail));
        ok
    }

    fn failed(&self) -> usize {
        self.rows.iter().filter(|r| !r.1).count()
    }

    fn print(&self, venue: &str) {
        println!("\n==== {venue} test-network dry run ====");
        println!("{:<3} {:<44} {:<6} detail", "#", "step", "result");
        for (i, (name, ok, detail)) in self.rows.iter().enumerate() {
            let short: String = detail.chars().take(110).collect();
            println!("{:<3} {:<44} {:<6} {short}", i + 1, name, if *ok { "PASS" } else { "FAIL" });
        }
        println!("==== {} of {} steps passed ====\n", self.rows.len() - self.failed(), self.rows.len());
    }
}

#[test]
#[ignore = "needs ALEPH_TESTNET_KEY / ALEPH_TESTNET_SECRET (test-network keys; see docs/venue-dry-run.md)"]
fn testnet_dry_run() {
    let (Ok(key), Ok(secret)) = (std::env::var("ALEPH_TESTNET_KEY"), std::env::var("ALEPH_TESTNET_SECRET")) else {
        eprintln!("skipped: no testnet keys");
        return;
    };
    let venue = std::env::var("ALEPH_TESTNET_VENUE").unwrap_or_else(|_| "binance".into());
    match venue.as_str() {
        "binance" => {
            std::env::set_var("ALEPH_EDGE_BINANCE_FUTURES_BASE", crate::app::endpoints::BINANCE_FUTURES_TESTNET_BASE);
            assert!(!crate::app::endpoints::binance_is_production(), "refusing: the futures base is production");
        }
        "bybit" | "okx" => {
            std::env::set_var("ALEPH_EDGE_VENUE_SANDBOX", "1");
            assert!(super::venue::ccxt::sandbox_from_env(), "refusing: the venue sandbox is off");
        }
        other => panic!("ALEPH_TESTNET_VENUE={other}: binance, bybit or okx"),
    }
    let cred = ExchangeCredential {
        exchange_id: venue.clone(),
        label: "testnet".into(),
        api_key: key,
        api_secret: secret,
        passphrase: std::env::var("ALEPH_TESTNET_PASSPHRASE").ok(),
        permission: crate::vault::model::CredentialPermission::TradeOnly,
        added_at: 0,
        updated_at: None,
    };
    let ex = ExchangeManager::new();
    let mut report = Report::default();
    tauri::async_runtime::block_on(async {
        run(&ex, &cred, &venue, &mut report).await;
        cleanup(&ex, &cred, &mut report).await;
    });
    report.print(&venue);
    assert_eq!(report.failed(), 0, "{venue}: {} step(s) failed, see the table above", report.failed());
}

/// The steps. Returns early (after recording why) when going on would only
/// trade blind; `cleanup` runs either way.
async fn run(ex: &ExchangeManager, cred: &ExchangeCredential, venue: &str, r: &mut Report) {
    // 1. account read, flat start, one-way mode
    let acc = match ex.futures_account(cred).await {
        Ok(a) => a,
        Err(e) => {
            r.step("account read", false, format!("{e:?}"));
            return;
        }
    };
    r.step("account read", true, format!("wallet {:.2} USDT, available {:.2}", acc.total_wallet_balance, acc.available_balance));
    for sym in [SYMBOL, SECOND] {
        if !r.step(&format!("flat start {sym}"), held(&acc, sym) == 0.0, format!("{}", held(&acc, sym))) {
            return; // never touch a position the run did not open
        }
    }
    r.started = true;
    let hedge = ex.hedge_mode(cred, SYMBOL).await;
    if !r.step("one-way mode", matches!(hedge, Ok(false)), format!("hedge={hedge:?}")) {
        return;
    }

    // 2. symbol rules
    let rules = match ex.symbol_rules(cred, SYMBOL).await {
        Ok(rules) => rules,
        Err(e) => {
            r.step("symbol rules", false, format!("{e:?}"));
            return;
        }
    };
    let Some(price) = ex.status(venue, SYMBOL).await.futures_price else {
        r.step("reference price", false, "no public price");
        return;
    };
    let rules_ok = rules.trading && rules.lot.step > 0.0 && rules.tick > 0.0;
    r.step(
        "symbol rules",
        rules_ok,
        format!("trading {} step {} min {} tick {} minNotional {}", rules.trading, rules.lot.step, rules.lot.min_qty, rules.tick, rules.min_notional),
    );
    if !rules_ok {
        return;
    }
    let iso = ex.set_isolated(cred, SYMBOL).await;
    r.step("isolated margin", iso.is_ok(), format!("{iso:?}"));
    let lev = ex.set_leverage(cred, SYMBOL, 2).await;
    r.step("leverage 2x", lev.is_ok(), format!("{lev:?}"));

    // 3. entry with the signal bot's own client id shape ("ae" + a UUID's hex)
    let qty = test_qty(&rules, price);
    let now = super::providers::binance_http::now_ms();
    let signal_id = format!("{:08x}-0000-4000-8000-{:012x}", now % 0xffff_ffff, now % 0xffff_ffff_ffff);
    let cid = entry_client_id(&signal_id);
    let fill = ex.open_market(cred, SYMBOL, true, qty, &cid).await;
    if !r.step("entry with client id", fill.is_ok(), format!("{fill:?} qty {qty}")) {
        return;
    }
    let acc = ex.futures_account(cred).await.ok();
    let amt = acc.as_ref().map_or(0.0, |a| held(a, SYMBOL));
    r.step("position read back", (amt - qty).abs() < rules.lot.step / 2.0, format!("{amt} vs {qty}"));
    // The venue's (test-network) entry price: stops are set from it, not
    // from the public production price.
    let reference = acc.as_ref().and_then(|a| entry_px(a, SYMBOL)).unwrap_or(price);

    // 4. the entry found by client id, and recognised as ours if lost
    let by_id = ex.order_by_client_id(cred, SYMBOL, &cid).await;
    r.step("entry found by client id", by_id.as_ref().is_ok_and(|s| s.fill.is_some() && s.status == "FILLED"), format!("{by_id:?}"));
    let orders = ex.recent_orders(cred, SYMBOL).await;
    let own = orders.as_ref().ok().and_then(|o| crate::bot::engine::reconcile::attribute_position(amt, o, &[]));
    let expected = crate::bot::engine::reconcile::own_entry_id(venue, &signal_id);
    r.step(
        "lost entry recognised by client id",
        own.as_ref().is_some_and(|o| o.client_id == expected),
        format!("{own:?} (expected {expected})"),
    );

    // 5. the stop
    let s1 = quantize_price(reference * 0.95, rules.tick);
    let stop = ex.place_protective(cred, SYMBOL, true, Protective::Stop, s1).await;
    r.step("stop placed (read back live)", stop.is_ok(), format!("{stop:?} at {s1}"));
    let Ok(mut stop_id) = stop else { return };
    let st = ex.algo_status(cred, SYMBOL, stop_id).await;
    r.step("stop status NEW", st.as_deref().is_ok_and(|s| s == "NEW"), format!("{st:?}"));

    // 6. the breakeven move, as the engine makes it on this venue. The
    // target sits between the stop and the price (a level at the entry
    // itself can be refused while the mark is under the fill).
    let s2 = quantize_price(reference * 0.97, rules.tick);
    match breakeven_move(venue) {
        Some(BreakevenMove::MoveInPlace) => {
            r.step("breakeven gate (move in place)", true, "sandbox: venue allowed");
            let moved = ex.move_stop(cred, SYMBOL, stop_id, s2).await;
            r.step("breakeven move (in place, read back)", moved.is_ok(), format!("{moved:?} to {s2}"));
            if let Ok(id) = moved {
                stop_id = id;
                let st = ex.algo_status(cred, SYMBOL, stop_id).await;
                r.step("moved stop status NEW", st.as_deref().is_ok_and(|s| s == "NEW"), format!("{st:?}"));
            }
            let open = ex.open_order_count(cred, SYMBOL).await;
            println!("INFO open orders after the move (one stop expected; Bybit may list the position stop or not): {open:?}");
        }
        Some(BreakevenMove::StopBeside) => {
            r.step("breakeven gate (stop beside)", true, "binance");
            let beside = ex.place_reduce_stop(cred, SYMBOL, true, qty, s2).await;
            r.step("breakeven stop placed beside", beside.is_ok(), format!("{beside:?} at {s2}"));
            if let Ok(id) = beside {
                let cancel = ex.cancel_algo(cred, SYMBOL, stop_id).await;
                r.step("old stop cancelled", cancel.is_ok(), format!("{cancel:?}"));
                stop_id = id;
            }
        }
        None => {
            r.step("breakeven gate", false, "no exchange-side breakeven for this venue in the sandbox");
        }
    }

    // 7. add + reduce (partials and DCA adds)
    let add_cid = format!("aa{}", super::providers::binance_http::now_ms());
    let add = ex.open_market(cred, SYMBOL, true, qty, &add_cid).await;
    r.step("market add", add.is_ok(), format!("{add:?}"));
    let red = ex.reduce_market_all(cred, SYMBOL, true, qty).await;
    r.step("reduce", red.is_ok(), format!("{red:?}"));

    // 8. cancelling the stop by id (OKX: cancel-algos by algoClOrdId)
    let cancel = ex.cancel_algo(cred, SYMBOL, stop_id).await;
    r.step("stop cancelled by id", cancel.is_ok(), format!("{cancel:?}"));
    let st = ex.algo_status(cred, SYMBOL, stop_id).await;
    r.step("cancelled stop reads CANCELED", st.as_deref().is_ok_and(|s| s == "CANCELED"), format!("{st:?}"));

    // 9. the confirmed close, with a resting stop for the sweep to cancel
    let rest = ex.place_protective(cred, SYMBOL, true, Protective::Stop, quantize_price(reference * 0.9, rules.tick)).await;
    r.step("stop before close", rest.is_ok(), format!("{rest:?}"));
    let left = ex.futures_account(cred).await.map(|a| held(&a, SYMBOL)).unwrap_or(qty);
    let close = ex.flatten(cred, SYMBOL, true, left.abs()).await;
    r.step("confirmed close (account flat)", close.is_ok(), format!("{close:?}"));

    // 10. the cancel sweep
    let sweep = ex.cancel_symbol_orders(cred, SYMBOL).await;
    r.step("cancel sweep", sweep.is_ok(), format!("{sweep:?}"));
    let flat = ex.futures_account(cred).await.map(|a| held(&a, SYMBOL));
    let open = ex.open_order_count(cred, SYMBOL).await;
    r.step(
        "flat and no orders left",
        matches!(flat, Ok(a) if a == 0.0) && matches!(open, Ok(0)),
        format!("held {flat:?}, orders {open:?}"),
    );

    // 11. our ids in the closed-order history (reconcile reads it)
    let history = ex.recent_orders(cred, SYMBOL).await;
    let venue_cid = venue_order_client_id(venue, &cid);
    let entry_row = history.as_ref().ok().and_then(|h| h.iter().find(|o| o.client_order_id == venue_cid));
    r.step(
        "closed-order history has the entry id",
        entry_row.is_some_and(|o| o.status == "FILLED" && o.executed_qty > 0.0),
        format!("{entry_row:?} (looked for {venue_cid})"),
    );
    let reducing = history.as_ref().map(|h| h.iter().filter(|o| o.reducing && o.executed_qty > 0.0).count());
    r.step("closed-order history marks the closes reduce-only", matches!(reducing, Ok(n) if n > 0), format!("{reducing:?} reducing fills"));

    // 12. Close all over two symbols, each with a stop
    let mut opened = Vec::new();
    for sym in [SYMBOL, SECOND] {
        let (Ok(rules), Some(px)) = (ex.symbol_rules(cred, sym).await, ex.status(venue, sym).await.futures_price) else {
            r.step(&format!("close all: open {sym}"), false, "no rules or price");
            continue;
        };
        let _ = ex.set_isolated(cred, sym).await;
        let _ = ex.set_leverage(cred, sym, 2).await;
        let cid = format!("astest{}{}", sym.len(), super::providers::binance_http::now_ms() % 1_000_000_000);
        let q = test_qty(&rules, px);
        let entry = ex.open_market(cred, sym, true, q, &cid).await;
        let at = match ex.futures_account(cred).await.ok().and_then(|a| entry_px(&a, sym)) {
            Some(p) => p,
            None => px,
        };
        let stop = match &entry {
            Ok(_) => ex.place_protective(cred, sym, true, Protective::Stop, quantize_price(at * 0.9, rules.tick)).await.map(|_| ()),
            Err(_) => Err(BinanceKeyCheckError::Unknown),
        };
        if r.step(&format!("close all: open {sym} with a stop"), entry.is_ok() && stop.is_ok(), format!("{entry:?} / stop {stop:?}")) {
            opened.push(sym);
        } else if entry.is_ok() {
            opened.push(sym); // still close it
        }
    }
    let report = ex.close_all(cred).await;
    let all_closed = report.as_ref().is_ok_and(|rep| {
        rep.failed.is_empty() && rep.uncleaned.is_empty() && opened.iter().all(|s| rep.closed.iter().any(|c| c == s))
    });
    r.step("close all", all_closed, format!("{report:?}"));
    let acc = ex.futures_account(cred).await.ok();
    for sym in opened {
        let amt = acc.as_ref().map(|a| held(a, sym));
        let orders = ex.open_order_count(cred, sym).await;
        r.step(
            &format!("close all: {sym} flat and clean"),
            amt == Some(0.0) && matches!(orders, Ok(0)),
            format!("held {amt:?}, orders {orders:?}"),
        );
    }
}

/// Leaves both symbols flat with no orders, whatever `run` did.
async fn cleanup(ex: &ExchangeManager, cred: &ExchangeCredential, r: &mut Report) {
    if !r.started {
        return; // the account was not flat: nothing here is the run's
    }
    let Ok(acc) = ex.futures_account(cred).await else {
        r.step("cleanup", false, "account unreadable: check the test account by hand");
        return;
    };
    let mut ok = true;
    for sym in [SYMBOL, SECOND] {
        let amt = held(&acc, sym);
        if amt != 0.0 {
            ok &= ex.flatten(cred, sym, amt > 0.0, amt.abs()).await.is_ok();
        }
        ok &= ex.cancel_symbol_orders(cred, sym).await.is_ok();
    }
    if !ok {
        r.step("cleanup", false, "a position or order may be left: check the test account by hand");
    }
}
