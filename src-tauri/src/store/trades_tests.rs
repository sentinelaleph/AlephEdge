//! Live execution fields must survive the trade table: insert → list on an
//! in-memory SQLite, for a paper row (NULL fills) and a live row (real fills
//! and commission), plus the migration on a table that predates them.

use rusqlite::Connection;

use super::model::TradeRecord;
use super::trades;

fn record(live: bool) -> TradeRecord {
    TradeRecord {
        id: 0,
        signal_id: if live { "live" } else { "paper" }.into(),
        bot_kind: "futures".into(),
        exchange_id: "binance".into(),
        symbol: "SOLUSDT".into(),
        direction: "short".into(),
        entry: 150.2,
        exit: 147.0,
        leverage: 3,
        capital: 500.0,
        pnl_pct: 6.2,
        pnl_quote: 31.0,
        exit_reason: "tp".into(),
        fr_at_open: None,
        ld_at_open: Some(1.0e6),
        opened_at: 1,
        closed_at: if live { 3 } else { 2 },
        unlevered_net_pct: Some(2.03),
        live,
        fill_entry: live.then_some(150.2),
        fill_exit: live.then_some(147.0),
        commission_usdt: live.then_some(1.1888),
        sizing_mode: if live { "risk" } else { "fixed" }.into(),
        risk_pct: live.then_some(1.0),
        notional_usdt: Some(1502.0),
        effective_leverage: Some(3.004),
        risk_capped: live,
        pnl_usdt: 31.0,
        pnl_pct_of_capital: 6.2,
        veto_reason_code: None,
        veto_reason_text: None,
        tp_target: live.then(|| "custom:40".to_string()),
        tp_fallback_from: None,
        manual: false,
    }
}

#[test]
fn live_fields_round_trip_and_paper_rows_stay_null() {
    let conn = Connection::open_in_memory().unwrap();
    trades::migrate(&conn).unwrap();
    trades::migrate(&conn).unwrap(); // idempotent
    trades::insert(&conn, &record(false)).unwrap();
    trades::insert(&conn, &record(true)).unwrap();

    let rows = trades::list(&conn, 10).unwrap();
    let live = rows.iter().find(|t| t.signal_id == "live").unwrap();
    assert!(live.live);
    assert_eq!(
        (live.fill_entry, live.fill_exit),
        (Some(150.2), Some(147.0))
    );
    assert_eq!(live.commission_usdt, Some(1.1888));
    assert_eq!(
        (live.sizing_mode.as_str(), live.risk_pct),
        ("risk", Some(1.0))
    );
    assert_eq!(
        (live.notional_usdt, live.effective_leverage),
        (Some(1502.0), Some(3.004))
    );
    assert!(live.risk_capped);
    // Derived on read from the single stored PnL, never stored twice.
    assert_eq!(
        (live.pnl_usdt, live.pnl_pct_of_capital),
        (live.pnl_quote, live.pnl_pct)
    );
    let paper = rows.iter().find(|t| t.signal_id == "paper").unwrap();
    assert!(!paper.live);
    assert_eq!(
        (paper.fill_entry, paper.fill_exit, paper.commission_usdt),
        (None, None, None)
    );

    let csv = trades::export_csv(&conn).unwrap();
    assert!(csv.lines().next().unwrap().ends_with(
        "live,fill_entry,fill_exit,commission_usdt,sizing_mode,risk_pct,notional_usdt,effective_leverage,risk_capped,veto_reason_code,veto_reason_text,tp_target,tp_fallback_from"
    ));
    assert!(csv.contains(",true,150.2,147,1.1888,risk,1,1502,3.004,true,,"));
}

// Veto columns (2026-09-18): NULL on every ordinary exit, round-trip intact
// on a "veto" exit, and present in the CSV export.
#[test]
fn veto_reason_round_trips_and_defaults_to_null() {
    let conn = Connection::open_in_memory().unwrap();
    trades::migrate(&conn).unwrap();
    let mut vetoed = record(false);
    vetoed.signal_id = "vetoed".into();
    vetoed.exit_reason = "veto".into();
    vetoed.veto_reason_code = Some("btc_regime_turn_bullish".into());
    vetoed.veto_reason_text = Some("BTC turned bullish".into());
    trades::insert(&conn, &vetoed).unwrap();
    trades::insert(&conn, &record(false)).unwrap(); // ordinary "tp" exit

    let rows = trades::list(&conn, 10).unwrap();
    let v = rows.iter().find(|t| t.signal_id == "vetoed").unwrap();
    assert_eq!(
        v.veto_reason_code.as_deref(),
        Some("btc_regime_turn_bullish")
    );
    assert_eq!(v.veto_reason_text.as_deref(), Some("BTC turned bullish"));
    let ordinary = rows.iter().find(|t| t.signal_id == "paper").unwrap();
    assert_eq!(
        (&ordinary.veto_reason_code, &ordinary.veto_reason_text),
        (&None, &None)
    );

    let csv = trades::export_csv(&conn).unwrap();
    assert!(csv.contains("btc_regime_turn_bullish,BTC turned bullish"));
}

#[test]
fn table_from_before_live_columns_migrates() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE trades (id INTEGER PRIMARY KEY AUTOINCREMENT, signal_id TEXT NOT NULL,
            bot_kind TEXT NOT NULL, symbol TEXT NOT NULL, direction TEXT NOT NULL,
            entry REAL NOT NULL, exit REAL NOT NULL, leverage INTEGER NOT NULL,
            capital REAL NOT NULL, pnl_pct REAL NOT NULL, pnl_quote REAL NOT NULL,
            exit_reason TEXT NOT NULL, fr_at_open REAL, ld_at_open REAL,
            opened_at INTEGER NOT NULL, closed_at INTEGER NOT NULL);
         INSERT INTO trades (signal_id, bot_kind, symbol, direction, entry, exit, leverage,
            capital, pnl_pct, pnl_quote, exit_reason, opened_at, closed_at)
         VALUES ('old', 'spot', 'BTCUSDT', 'long', 1, 2, 1, 10, 1, 1, 'tp', 1, 1);",
    )
    .unwrap();
    trades::migrate(&conn).unwrap();
    let rows = trades::list(&conn, 10).unwrap();
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].live && rows[0].commission_usdt.is_none());
    assert_eq!(
        rows[0].sizing_mode, "fixed",
        "pre-sizing rows read as fixed"
    );
    assert_eq!((rows[0].notional_usdt, rows[0].risk_capped), (None, false));
    assert_eq!(rows[0].pnl_usdt, 1.0);
    assert_eq!(
        (&rows[0].tp_target, &rows[0].tp_fallback_from),
        (&None, &None),
        "rows from before take-profit targets read as unknown (all were TP1)"
    );
}

// Take-profit target columns (2026-09-22): round-trip and CSV.
#[test]
fn tp_target_round_trips_and_exports() {
    let conn = Connection::open_in_memory().unwrap();
    trades::migrate(&conn).unwrap();
    let mut fell_back = record(false);
    fell_back.signal_id = "fallback".into();
    fell_back.tp_target = Some("tp1".into());
    fell_back.tp_fallback_from = Some("tp3".into());
    trades::insert(&conn, &fell_back).unwrap();
    trades::insert(&conn, &record(true)).unwrap();

    let rows = trades::list(&conn, 10).unwrap();
    let f = rows.iter().find(|t| t.signal_id == "fallback").unwrap();
    assert_eq!(
        (f.tp_target.as_deref(), f.tp_fallback_from.as_deref()),
        (Some("tp1"), Some("tp3"))
    );
    let live = rows.iter().find(|t| t.live).unwrap();
    assert_eq!(live.tp_target.as_deref(), Some("custom:40"));
    let csv = trades::export_csv(&conn).unwrap();
    assert!(csv.contains(",tp1,tp3"));
    assert!(csv.contains(",custom:40,"));
}

#[test]
fn real_and_simulated_money_never_share_a_total() {
    let conn = Connection::open_in_memory().unwrap();
    trades::migrate(&conn).unwrap();
    let mut paper_win = record(false);
    paper_win.pnl_quote = 50.0;
    let mut live_loss = record(true);
    live_loss.pnl_quote = -20.0;
    trades::insert(&conn, &paper_win).unwrap();
    trades::insert(&conn, &live_loss).unwrap();

    let all = trades::stats_scoped(&conn, 0, None).unwrap();
    let live = trades::stats_scoped(&conn, 0, Some(true)).unwrap();
    let paper = trades::stats_scoped(&conn, 0, Some(false)).unwrap();
    assert_eq!(all.today_pnl_quote, 30.0, "the old mixed total hid the real loss");
    assert_eq!(live.today_pnl_quote, -20.0);
    assert_eq!(paper.today_pnl_quote, 50.0);
}

#[test]
fn recent_keys_lists_traded_signals_since_a_time() {
    let conn = Connection::open_in_memory().unwrap();
    trades::migrate(&conn).unwrap();
    trades::insert(&conn, &record(false)).unwrap();
    assert_eq!(
        trades::recent_keys(&conn, 0).unwrap(),
        vec![("paper".to_string(), "futures".to_string())]
    );
    assert!(trades::recent_keys(&conn, 10).unwrap().is_empty(), "opened_at 1 < 10");
}

/// Sentinel's veto text reaches the CSV verbatim. A comma in it used to shift
/// every later column, so the row no longer matched its header.
#[test]
fn free_text_with_commas_and_quotes_keeps_the_csv_aligned() {
    let conn = Connection::open_in_memory().unwrap();
    trades::migrate(&conn).unwrap();
    let mut vetoed = record(false);
    vetoed.exit_reason = "veto".into();
    vetoed.veto_reason_code = Some("btc_regime_turn_bullish".into());
    vetoed.veto_reason_text = Some("BTC turned bullish, +1.4% in 1h \"strong\"".into());
    vetoed.tp_target = Some("=HYPERLINK(1)".into());
    trades::insert(&conn, &vetoed).unwrap();

    let csv = trades::export_csv(&conn).unwrap();
    let mut lines = csv.lines();
    let header_cols = lines.next().unwrap().split(',').count();
    let row = lines.next().unwrap();
    // RFC 4180 split: commas inside quotes do not count.
    let (mut cols, mut quoted) = (1, false);
    for ch in row.chars() {
        match ch {
            '"' => quoted = !quoted,
            ',' if !quoted => cols += 1,
            _ => {}
        }
    }
    assert_eq!(cols, header_cols, "row: {row}");
    assert!(row.contains(r#""BTC turned bullish, +1.4% in 1h ""strong""""#), "{row}");
    assert!(row.contains(",'=HYPERLINK(1),"), "formula not neutralised: {row}");
}
