//! Live-path decisions, pure: stop replacement ordering, partial quantization,
//! reconciliation (flat on exchange + which order fired → reason), and trade
//! records built from real fills. No network: `LIVE_TRADING_ENABLED` is false
//! and these tests never need it to be otherwise.

use super::live_close::{
    fallback_settlement, live_trade_record, settlement_from_summary, Settlement,
};
use super::live_manage::{
    drop_partial, plan_breakeven_move, plan_partial, PartialPlan, StopAction,
};
use crate::exchange::BreakevenMove::{MoveInPlace, StopBeside};
use super::reconcile::{
    claim_flat, exchange_state, infer_exit_reason, is_due, plan_reconcile, plan_reconcile_on, reconcile_venues,
    reference_exit, ExchangeSide, ReconcilePlan, RECONCILE_INTERVAL_MS,
};
use super::test_fixtures::{plan, position, signal};
use crate::bot::model::OpenPosition;
use crate::exchange::providers::binance_fills::{parse_user_trades, summarize_fills};
use crate::exchange::providers::binance_rules::LotStep;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn live_short() -> OpenPosition {
    let mut p = position(
        &signal("short", 150.0, 146.0, 160.0),
        Some(plan(0.5, 1.0, 0.5)),
    );
    (p.live, p.qty, p.entry, p.entry_order_id) = (true, 10.0, 150.2, Some(1000));
    (p.stop_algo_id, p.tp_algo_id) = (Some(71), Some(72));
    p
}

#[test]
fn breakeven_places_new_stop_before_cancelling_old() {
    let mut p = live_short();
    assert!(plan_breakeven_move(&p, StopBeside).is_empty(), "not armed yet");
    p.breakeven_armed = true;
    assert_eq!(
        plan_breakeven_move(&p, StopBeside),
        vec![
            StopAction::Place { trigger: 150.2 },
            StopAction::Cancel { algo_id: 71 }
        ],
        "never a window without a stop"
    );
    p.stop_at_breakeven = true;
    assert!(
        plan_breakeven_move(&p, StopBeside).is_empty(),
        "already moved: nothing to resend"
    );

    let mut no_old = live_short();
    (no_old.breakeven_armed, no_old.stop_algo_id) = (true, None);
    assert_eq!(
        plan_breakeven_move(&no_old, StopBeside),
        vec![StopAction::Place { trigger: 150.2 }]
    );

    let mut paper = live_short();
    (paper.live, paper.breakeven_armed) = (false, true);
    for how in [StopBeside, MoveInPlace] {
        assert!(
            plan_breakeven_move(&paper, how).is_empty(),
            "paper never touches the exchange"
        );
    }
}

// Bybit / OKX: the position-level stop moves in place, one step, and is
// never followed by a cancel (on Bybit the cancel writes "0" into the slot
// that was just moved, leaving the position with no stop at all).
#[test]
fn bybit_okx_breakeven_moves_the_stop_in_place_without_a_cancel() {
    let mut p = live_short();
    p.exchange_id = "okx".into();
    assert!(plan_breakeven_move(&p, MoveInPlace).is_empty(), "not armed yet");
    p.breakeven_armed = true;
    let plan = plan_breakeven_move(&p, MoveInPlace);
    assert_eq!(plan, vec![StopAction::Move { algo_id: 71, trigger: 150.2 }]);
    assert!(
        !plan.iter().any(|a| matches!(a, StopAction::Cancel { .. } | StopAction::Place { .. })),
        "no cancel, no quantity stop"
    );
    p.stop_at_breakeven = true;
    assert!(plan_breakeven_move(&p, MoveInPlace).is_empty(), "already moved");

    // No stop on record: a whole-position stop at entry, nothing to cancel.
    let mut no_old = live_short();
    (no_old.breakeven_armed, no_old.stop_algo_id) = (true, None);
    assert_eq!(
        plan_breakeven_move(&no_old, MoveInPlace),
        vec![StopAction::PlaceWhole { trigger: 150.2 }]
    );
}

#[test]
fn partial_quantizes_down_and_skips_below_minimum() {
    let lot = LotStep {
        step: 1.0,
        min_qty: 1.0,
    };
    assert_eq!(
        plan_partial(7.0, 0.5, lot),
        PartialPlan::Reduce(3.0),
        "3.5 floors to 3"
    );
    assert_eq!(
        plan_partial(1.0, 0.5, lot),
        PartialPlan::BelowMinimum,
        "0.5 < min 1"
    );
    let fine = LotStep {
        step: 0.001,
        min_qty: 0.001,
    };
    assert_eq!(plan_partial(0.015, 0.5, fine), PartialPlan::Reduce(0.007));
    // A fraction that would reduce everything is not a partial.
    assert_eq!(plan_partial(2.0, 1.0, lot), PartialPlan::BelowMinimum);

    let mut p = live_short();
    (p.partial_fraction, p.partial_price, p.partial_qty) = (0.5, Some(148.0), 5.0);
    drop_partial(&mut p);
    assert_eq!((p.partial_fraction, p.partial_qty), (0.0, 0.0));
    assert!(
        p.partial_price.is_some(),
        "kept so the plan does not refire every tick"
    );
}

#[test]
fn exchange_side_from_signed_amount() {
    assert_eq!(exchange_state(true, Some(3.0)), ExchangeSide::Open);
    assert_eq!(exchange_state(false, Some(-3.0)), ExchangeSide::Open);
    assert_eq!(exchange_state(false, None), ExchangeSide::Flat);
    assert_eq!(exchange_state(true, Some(0.0)), ExchangeSide::Flat);
    assert_eq!(exchange_state(true, Some(-3.0)), ExchangeSide::Mismatch);
    assert_eq!(exchange_state(false, Some(2.0)), ExchangeSide::Mismatch);
}

#[test]
fn which_order_fired_decides_the_reason() {
    assert_eq!(infer_exit_reason(false, true, false), "tp");
    assert_eq!(infer_exit_reason(true, false, false), "sl");
    assert_eq!(infer_exit_reason(true, false, true), "breakeven");
    assert_eq!(infer_exit_reason(false, false, true), "exchangeClosed");
    assert_eq!(infer_exit_reason(false, false, false), "exchangeClosed");
    let p = live_short();
    assert_eq!(reference_exit(&p, "tp"), Some(146.0));
    assert_eq!(reference_exit(&p, "sl"), Some(160.0));
    assert_eq!(reference_exit(&p, "breakeven"), Some(150.2));
    assert_eq!(reference_exit(&p, "exchangeClosed"), None);
}

#[test]
fn reconcile_settles_flat_flags_foreign_and_ignores_paper() {
    let mut open = live_short();
    open.signal_id = "open".into();
    let mut gone = live_short();
    (gone.signal_id, gone.symbol) = ("gone".into(), "ETHUSDT".into());
    let mut flipped = live_short();
    (flipped.signal_id, flipped.symbol) = ("flipped".into(), "BNBUSDT".into());
    let mut paper = live_short();
    (paper.signal_id, paper.symbol, paper.live) = ("paper".into(), "XRPUSDT".into(), false);

    let exchange = vec![
        ("SOLUSDT".to_string(), -10.0),
        ("BNBUSDT".to_string(), 4.0),
        ("XRPUSDT".to_string(), 100.0), // user's own: a paper position does not claim it
    ];
    let plan = plan_reconcile(&[open, gone, flipped, paper], &exchange);
    assert_eq!(
        plan,
        ReconcilePlan {
            closed: vec!["gone".into()],
            mismatched: vec!["BNBUSDT".into()],
            untracked: vec!["XRPUSDT".into()],
        }
    );
}

#[test]
fn reconcile_runs_at_startup_then_on_interval() {
    assert!(is_due(0, 5));
    assert!(!is_due(1_000, 1_000 + RECONCILE_INTERVAL_MS - 1));
    assert!(is_due(1_000, 1_000 + RECONCILE_INTERVAL_MS));
}

// The fixture short: entry VWAP 150.2 over 10, exits 148×5 + 146×5 = 147.0,
// commission 1.1888 USDT. The record must carry exactly those, and the
// ledger-scale figure must use the real fee instead of the 0.10 assumption.
#[test]
fn trade_record_from_real_fills_and_fees() {
    let body = include_str!("../../../tests/fixtures/binance_user_trades_short_2legs.json");
    let trades = parse_user_trades(body).unwrap();
    let summary = summarize_fills(&trades, false, Some(1000)).unwrap();
    let p = live_short();
    let rec = live_trade_record(&p, &settlement_from_summary(&summary), "tp", 9);

    assert!(rec.live);
    assert_eq!((rec.fill_entry, rec.fill_exit), (Some(150.2), Some(147.0)));
    assert!(close(rec.commission_usdt.unwrap(), 1.1888));
    let move_pct = (150.2 - 147.0) / 150.2 * 100.0;
    let fee_pct = 1.1888 / 1502.0 * 100.0;
    assert!(close(
        rec.unlevered_net_pct.unwrap(),
        move_pct - fee_pct + 0.02
    ));
    assert!(close(rec.pnl_quote, 3.2 * 10.0 - 1.1888));
    assert!(close(rec.pnl_pct, rec.pnl_quote / p.capital * 100.0));
}

#[test]
fn fallback_blends_exchange_partial_and_marks_trigger_exit() {
    let mut p = live_short();
    (p.partial_price, p.partial_qty, p.partial_fraction) = (Some(148.0), 4.0, 0.4);
    let s = fallback_settlement(&p, 146.0, false);
    assert!(close(s.exit, (148.0 * 4.0 + 146.0 * 6.0) / 10.0));
    assert_eq!((s.entry, s.qty, s.commission_usdt), (150.2, 10.0, None));
    let rec = live_trade_record(&p, &s, "tp", 9);
    assert_eq!(rec.fill_exit, None, "a trigger level is not a fill");
    // Unknown fee: ledger 0.10 round trip on the ledger-scale figure.
    let move_pct = (150.2 - s.exit) / 150.2 * 100.0;
    assert!(close(
        rec.unlevered_net_pct.unwrap(),
        move_pct - 0.10 + 0.02
    ));

    let no_partial = Settlement {
        exit_is_fill: true,
        ..fallback_settlement(&live_short(), 146.0, true)
    };
    assert_eq!(
        live_trade_record(&p, &no_partial, "tp", 9).fill_exit,
        Some(146.0)
    );
}

// A position's `live` flag is persisted as raw JSON, so anything that can
// write the app's data directory can set it. Until 2026-09-20 every close,
// manage and reconcile path read that flag on its own, so one edited line
// reached real reduce-only orders while the compile-time master switch was
// off — the "two deliberate flips" contract held for entries only. `is_live()`
// is the single gate all of them read now.
// Default (non-live) builds only: in a `--features live` build a live flag
// is honoured by design.
#[cfg(not(feature = "live"))]
#[test]
fn tampered_live_flag_cannot_reach_the_venue() {
    const _: () = assert!(!crate::bot::model::LIVE_TRADING_ENABLED);
    let mut p = live_short();
    assert!(p.live, "fixture opts the position into live");
    assert!(
        !p.is_live(),
        "master switch is off, so no position may be treated as live"
    );
    p.live = false;
    assert!(!p.is_live());
}

// A live position settled by reconcile used to be closed WITHOUT the close
// claim, so a veto close running at the same moment (SSE handler, off the
// tick) could settle it too: two trade records for one real round trip.
#[test]
fn reconcile_settles_only_positions_it_could_claim() {
    let bots = crate::bot::BotManager::new();
    let p = live_short();
    bots.add_position(p.clone());
    let plan = plan_reconcile(std::slice::from_ref(&p), &[]);
    assert_eq!(plan.closed, vec![p.signal_id.clone()]);

    // A veto close holds the claim: reconcile must leave the position alone.
    assert!(bots.claim_open(&p.signal_id, p.bot_kind));
    assert!(claim_flat(&bots, std::slice::from_ref(&p), &plan).is_empty());

    // Released: reconcile claims it (and so blocks any other close).
    bots.closing.release(&p.signal_id, p.bot_kind);
    let claimed = claim_flat(&bots, std::slice::from_ref(&p), &plan);
    assert_eq!(claimed.len(), 1);
    assert!(!bots.claim_open(&p.signal_id, p.bot_kind), "held by reconcile");
}

#[test]
fn a_position_is_never_settled_from_another_venues_account() {
    // A Binance live position; the bot now trades Bybit (an exchange change
    // used to keep LIVE on). The Bybit account does not list the symbol.
    let mut on_binance = live_short();
    (on_binance.signal_id, on_binance.exchange_id) = ("bn".into(), "binance".into());
    let bybit_account = vec![("ETHUSDT".to_string(), 1.0)];
    let plan = plan_reconcile_on("bybit", std::slice::from_ref(&on_binance), &bybit_account);
    assert!(plan.closed.is_empty(), "not flat: it is on another account");
    assert_eq!(plan.untracked, vec!["ETHUSDT".to_string()]);
    // Its own venue still settles it once flat there.
    let plan = plan_reconcile_on("binance", std::slice::from_ref(&on_binance), &[]);
    assert_eq!(plan.closed, vec!["bn".to_string()]);
    // Each venue holding a live position is read, plus the live bot's venue.
    let mut on_okx = live_short();
    on_okx.exchange_id = "okx".into();
    let mut paper = live_short();
    (paper.exchange_id, paper.live) = ("bitget".into(), false);
    let book = [on_binance.clone(), on_okx, on_binance, paper];
    assert_eq!(reconcile_venues(Some("bybit"), &book), ["binance", "okx", "bybit"]);
    assert_eq!(reconcile_venues(Some("binance"), &book), ["binance", "okx"]);
    assert_eq!(reconcile_venues(None, &[]), Vec::<String>::new());
}

// The default build must not be able to send a signed ORDER from the engine.
// The gates (`live_trading_allowed`, `is_live`) are tested above; this pins
// WHERE the order-sending calls may live, so a new call site elsewhere (a
// strategy, a command, a helper) fails here instead of slipping past them.
// The manual flatten in `exchange/mod.rs` (`close_position` / `close_all`,
// user-invoked, reduce-only, now in `exchange/close.rs`) is the one
// documented exception.
#[test]
fn order_sending_calls_live_only_behind_the_live_gates() {
    const SENDERS: &[&str] = &[
        ".open_market(",
        ".place_protective(",
        ".place_reduce_stop(",
        ".move_stop(",
        ".reduce_market(",
        ".reduce_market_all(",
        ".cancel_algo(",
        ".cancel_symbol_orders(",
        ".set_leverage(",
        ".set_isolated(",
    ];
    const ALLOWED: &[&str] = &[
        "bot/engine/live.rs",
        "bot/engine/live_close.rs",
        "bot/engine/live_manage.rs",
        // DCA / Grid real-money mirror: gated by LIVE_TRADING_ENABLED and a
        // per-bot typed LIVE (strategy_set_live), like the signal path.
        "bot/strategy_live.rs",
        "exchange/live_orders.rs",
        "exchange/mod.rs",
        // The manual Close / Close all and the confirmed flatten (user-
        // invoked or behind the live gates, reduce-only).
        "exchange/close.rs",
        // The order router and the Bybit / OKX venues behind it.
        "exchange/orders.rs",
        "exchange/venue/ccxt.rs",
    ];
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).expect("src readable").flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    walk(&root, &mut files);
    let mut offenders = Vec::new();
    for f in files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        if ALLOWED.contains(&rel.as_str()) || rel.ends_with("_tests.rs") {
            continue;
        }
        let src = std::fs::read_to_string(&f).unwrap_or_default();
        for s in SENDERS {
            if src.contains(s) {
                offenders.push(format!("{rel}: {s}"));
            }
        }
    }
    assert!(offenders.is_empty(), "order-sending call outside the gated files: {offenders:?}");
}

// ---- Untracked positions: attribution and the adopt / flatten / report
// decision (an uncertain entry finalized as failed that filled afterwards).

mod untracked {
    use super::super::live::Protection;
    use super::super::reconcile::{
        adopted_position, attribute_position, is_own_client_id, own_entry_id, signal_for, untracked_action,
        OwnEntry, UntrackedAction,
    };
    use super::super::take_profit::TpResolution;
    use super::super::test_fixtures::{cfg, signal};
    use crate::exchange::providers::binance_parse::RecentOrder;
    use crate::exchange::providers::binance_requests::entry_client_id;

    const UUID_CID: &str = "aec3393031884949a7b006a3d50e2d3566";

    fn order(id: i64, cid: &str, buy: bool, qty: f64, at: u64) -> RecentOrder {
        RecentOrder {
            order_id: id,
            client_order_id: cid.into(),
            buy,
            status: if qty > 0.0 { "FILLED" } else { "CANCELED" }.into(),
            avg_price: if qty > 0.0 { 150.2 } else { 0.0 },
            executed_qty: qty,
            reducing: false,
            update_time: at,
        }
    }

    fn own(cid: &str) -> OwnEntry {
        OwnEntry { client_id: cid.into(), order_id: 9, avg_price: 100.0, qty: 2.0, filled_at: 1_000 }
    }

    #[test]
    fn our_client_ids_are_recognised_and_the_users_are_not() {
        assert!(is_own_client_id(UUID_CID, &[]), "Sentinel UUID shape");
        assert_eq!(entry_client_id("c3393031-8849-49a7-b006-a3d50e2d3566"), UUID_CID);
        assert!(is_own_client_id("aesig1", &["aesig1".to_string()]), "a buffered signal's id");
        for foreign in [
            "web_8Hk2s9dLq0aWm3",      // Binance web
            "aesig1",                  // "ae" prefix, not a known id, not a UUID
            "ae8Hk2s9dLq0aWm3xPz7Qa",  // a random 22-char API id that starts with "ae"
            "AEC3393031884949A7B006A3D50E2D3566", // case matters
            "xc3393031884949a7b006a3d50e2d3566",
            "",
        ] {
            assert!(!is_own_client_id(foreign, &[]), "{foreign}");
        }
    }

    #[test]
    fn lost_entries_are_recognised_on_every_order_venue() {
        // The audit's repro: a real signal id from the testnet DB.
        let mut sig = signal("short", 150.0, 146.0, 160.0);
        sig.id = "5ded7193-2335-436c-ba17-9bd2b5f8e87e".into();
        for venue in ["binance", "bybit", "okx"] {
            // What the venue's order history shows for our entry.
            let cid = own_entry_id(venue, &sig.id);
            assert!(is_own_client_id(&cid, &[]), "{venue}: {cid}");
            let got = attribute_position(-10.0, &[order(2, &cid, false, 10.0, 200)], &[]).expect(venue);
            assert_eq!(
                signal_for(&got, &sig.symbol, std::slice::from_ref(&sig), venue).map(|s| s.id.as_str()),
                Some(sig.id.as_str()),
                "{venue}: the signal behind it is found"
            );
        }
        assert_eq!(own_entry_id("binance", &sig.id), "ae5ded71932335436cba179bd2b5f8e87e");
        assert_eq!(own_entry_id("okx", &sig.id), "ae5ded71932335436cba179bd2b5f8e8", "cut to 32");
        // The short form is ours only behind our prefix, in lowercase hex.
        for foreign in [
            "xe5ded71932335436cba179bd2b5f8e8",
            "AE5DED71932335436CBA179BD2B5F8E8",
            "ae5ded71932335436cba179bd2b5f8g8",
            "ae5ded71932335436cba179bd2b5f8e",
        ] {
            assert!(!is_own_client_id(foreign, &[]), "{foreign}");
        }
        // A buffered non-UUID signal id maps the same way on both sides.
        let mut short_id = signal("long", 100.0, 106.0, 93.0);
        short_id.id = "sig-1".into();
        let known = vec![own_entry_id("okx", &short_id.id)];
        assert!(is_own_client_id(&known[0], &known));
    }

    #[test]
    fn a_position_is_ours_only_if_our_entry_is_its_latest_fill_at_its_size() {
        // Short 10 opened by our SELL entry; an earlier cancelled order and
        // a reduce-only stop that never fired do not matter.
        let mut stop = order(3, "stop_x", true, 0.0, 300);
        stop.reducing = true;
        let orders = vec![order(1, "web_old", true, 0.0, 100), order(2, UUID_CID, false, 10.0, 200), stop];
        let got = attribute_position(-10.0, &orders, &[]).expect("ours");
        assert_eq!(
            got,
            OwnEntry { client_id: UUID_CID.into(), order_id: 2, avg_price: 150.2, qty: 10.0, filled_at: 200 }
        );

        // The user's own manual trade: never ours.
        assert_eq!(attribute_position(-10.0, &[order(5, "web_abc", false, 10.0, 200)], &[]), None);
        // Ours, but the user added to it afterwards (latest fill is theirs).
        let added = vec![order(2, UUID_CID, false, 10.0, 200), order(6, "web_abc", false, 5.0, 400)];
        assert_eq!(attribute_position(-15.0, &added, &[]), None);
        // Ours, but the user reduced it afterwards with a reduce-only order.
        let mut cut = order(7, "web_abc", true, 4.0, 400);
        cut.reducing = true;
        let reduced = vec![order(2, UUID_CID, false, 10.0, 200), cut];
        assert_eq!(attribute_position(-6.0, &reduced, &[]), None);
        // Size does not match the entry: not provably ours.
        assert_eq!(attribute_position(-12.0, &orders, &[]), None);
        // Side does not match: our SELL cannot have opened a long.
        assert_eq!(attribute_position(10.0, &orders, &[]), None);
        // No orders, or a zero amount: nothing to attribute.
        assert_eq!(attribute_position(-10.0, &[], &[]), None);
        assert_eq!(attribute_position(0.0, &orders, &[]), None);
    }

    #[test]
    fn untracked_decision_matrix() {
        use UntrackedAction::*;
        let long = signal("long", 100.0, 106.0, 93.0); // 7% stop
        let o = own("aesig1");
        let act = |amt, own: Option<&OwnEntry>, sig, can, price, lev| {
            untracked_action(amt, own, sig, can, price, lev)
        };
        // Not ours: reported, whatever else holds.
        assert_eq!(act(2.0, None, Some(&long), true, Some(100.0), 2), Report);
        // Ours and protectable: adopt.
        assert_eq!(act(2.0, Some(&o), Some(&long), true, Some(100.0), 2), Adopt);
        assert_eq!(act(2.0, Some(&o), Some(&long), true, None, 2), Adopt, "the exchange refuses a crossed stop");
        // Ours but unprotectable as planned: flatten.
        assert_eq!(act(2.0, Some(&o), None, true, Some(100.0), 2), Flatten, "signal left the buffer");
        assert_eq!(act(2.0, Some(&o), Some(&long), false, Some(100.0), 2), Flatten, "no live bot to hold it");
        assert_eq!(act(-2.0, Some(&o), Some(&long), true, Some(100.0), 2), Flatten, "direction mismatch");
        assert_eq!(act(2.0, Some(&o), Some(&long), true, Some(92.0), 2), Flatten, "stop already crossed");
        assert_eq!(act(2.0, Some(&o), Some(&long), true, Some(100.0), 20), Flatten, "stop beyond liquidation");
        let short = signal("short", 100.0, 94.0, 107.0);
        assert_eq!(act(-2.0, Some(&o), Some(&short), true, Some(99.0), 2), Adopt);
        assert_eq!(act(-2.0, Some(&o), Some(&short), true, Some(107.5), 2), Flatten);
        let mut broken = long.clone();
        broken.tp.clear();
        assert_eq!(act(2.0, Some(&o), Some(&broken), true, Some(100.0), 2), Flatten, "incoherent geometry");
    }

    #[test]
    fn adopted_position_is_live_sized_by_the_real_fill_and_finds_its_signal() {
        let sig = signal("long", 100.0, 106.0, 93.0);
        let o = own(&entry_client_id(&sig.id));
        assert_eq!(signal_for(&o, "SOLUSDT", std::slice::from_ref(&sig), "binance").map(|s| s.id.as_str()), Some("sig-1"));
        assert!(signal_for(&o, "BTCUSDT", std::slice::from_ref(&sig), "binance").is_none(), "symbol must match");

        let tp = TpResolution::signal_tp1(&sig);
        let protected = Protection { stop_algo_id: Some(71), tp_algo_id: Some(72), tp: tp.clone() };
        let p = adopted_position(&cfg(), &sig, &o, 3, &protected, 5_000);
        assert!(p.live && !p.unprotected);
        assert_eq!((p.entry, p.qty, p.sl, p.tp), (100.0, 2.0, 93.0, 106.0));
        assert_eq!((p.entry_order_id, p.stop_algo_id, p.tp_algo_id), (Some(9), Some(71), Some(72)));
        assert_eq!((p.notional_usdt, p.effective_leverage), (200.0, 2.0));
        assert_eq!(p.opened_at, 1_000, "the fill time, not the adoption time");

        let bare = Protection { stop_algo_id: None, tp_algo_id: None, tp };
        let p = adopted_position(&cfg(), &sig, &o, 3, &bare, 5_000);
        assert!(p.unprotected, "the book flattens it every tick");
    }
}

#[test]
fn real_money_exposure_is_seen_from_a_position_or_a_live_bot() {
    use crate::bot::model::{BotConfig, BotKind};
    use crate::bot::BotManager;
    let bots = BotManager::new();
    assert!(!bots.has_live_exposure());
    bots.add_position(live_short());
    assert!(bots.has_live_exposure(), "an open real position blocks updates and key changes");

    let bots = BotManager::new();
    let mut cfg = BotConfig::new_default(BotKind::Futures, "binance");
    bots.configure(cfg.clone());
    assert!(!bots.has_live_exposure(), "a paper bot is not exposure");
    cfg.live = true;
    bots.configure(cfg);
    assert!(bots.has_live_exposure(), "a bot switched to live is exposure");
}
