//! Open positions must survive a restart: insert → reload round-trip on an
//! in-memory SQLite, including management state and the delete on close.

use rusqlite::Connection;

use super::positions;
use crate::bot::model::{BotKind, OpenPosition};
use crate::signal::model::ManagementPlan;

fn position(id: &str) -> OpenPosition {
    OpenPosition {
        signal_id: id.into(),
        bot_kind: BotKind::Futures,
        exchange_id: "binance".into(),
        symbol: "ETHUSDT".into(),
        timeframe: Some("4h".into()),
        direction: "short".into(),
        entry: 2500.0,
        tp: 2450.0,
        sl: 2600.0,
        signal_entry: 2500.0,
        risk_r: 100.0,
        plan: Some(ManagementPlan {
            breakeven_at_r: Some(0.5),
            partial_at_r: Some(1.0),
            partial_fraction: Some(0.5),
        }),
        breakeven_armed: false,
        partial_fraction: 0.0,
        partial_price: None,
        horizon_ms: 9_999,
        leverage: 3,
        capital: 100.0,
        fr_at_open: Some(0.01),
        ld_at_open: None,
        opened_at: 1,
        live: false,
        qty: 0.0,
        entry_order_id: None,
        stop_algo_id: None,
        tp_algo_id: None,
        stop_at_breakeven: false,
        unprotected: false,
        partial_qty: 0.0,
        sizing_mode: crate::bot::model::SizingMode::Fixed,
        risk_pct: None,
        notional_usdt: 300.0,
        effective_leverage: 3.0,
        risk_capped: false,
        tp_target: "tp1".into(),
        tp_fallback_from: None,
    }
}

fn save(conn: &Connection, p: &OpenPosition, at: u64) {
    let body = serde_json::to_string(p).unwrap();
    positions::upsert(conn, &p.signal_id, p.bot_kind.as_str(), &body, at).unwrap();
}

fn load(conn: &Connection) -> Vec<OpenPosition> {
    positions::load_all(conn)
        .unwrap()
        .iter()
        .map(|b| serde_json::from_str(b).unwrap())
        .collect()
}

#[test]
fn position_round_trip_update_and_delete() {
    let conn = Connection::open_in_memory().unwrap();
    positions::migrate(&conn).unwrap();
    positions::migrate(&conn).unwrap(); // idempotent

    let mut a = position("a");
    save(&conn, &a, 1);
    save(&conn, &position("b"), 2);
    a.breakeven_armed = true;
    a.partial_fraction = 0.5;
    a.partial_price = Some(2400.0);
    save(&conn, &a, 3);

    let loaded = load(&conn);
    assert_eq!(loaded.len(), 2, "update must not duplicate");
    let got = loaded.iter().find(|p| p.signal_id == "a").unwrap();
    assert!(got.breakeven_armed);
    assert_eq!(got.partial_price, Some(2400.0));
    assert_eq!(got.timeframe.as_deref(), Some("4h"));
    assert_eq!(got.plan, a.plan);

    positions::delete(&conn, "a", "futures").unwrap();
    let left = load(&conn);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].signal_id, "b");
}

// A live position's exchange order ids must survive a restart: without them
// the breakeven move cannot cancel the old stop and reconciliation cannot
// tell a fired stop from a fired take-profit.
#[test]
fn live_order_ids_round_trip() {
    let conn = Connection::open_in_memory().unwrap();
    positions::migrate(&conn).unwrap();
    let mut p = position("live");
    (p.live, p.qty, p.entry_order_id) = (true, 0.4, Some(8_389_765_012));
    (p.stop_algo_id, p.tp_algo_id) = (Some(2_146_760), None);
    (p.stop_at_breakeven, p.partial_qty) = (true, 0.2);
    p.sizing_mode = crate::bot::model::SizingMode::Risk;
    (
        p.risk_pct,
        p.notional_usdt,
        p.effective_leverage,
        p.risk_capped,
    ) = (Some(1.0), 1_020.4, 1.0204, true);
    save(&conn, &p, 1);

    let got = load(&conn).pop().unwrap();
    assert!(got.live && got.stop_at_breakeven);
    assert_eq!(got.entry_order_id, Some(8_389_765_012));
    assert_eq!((got.stop_algo_id, got.tp_algo_id), (Some(2_146_760), None));
    assert_eq!((got.qty, got.partial_qty), (0.4, 0.2));
    assert_eq!(got.sizing_mode, crate::bot::model::SizingMode::Risk);
    assert_eq!(
        (got.risk_pct, got.notional_usdt, got.risk_capped),
        (Some(1.0), 1_020.4, true)
    );
    let body = serde_json::to_string(&p).unwrap();
    for key in [
        "entryOrderId",
        "stopAlgoId",
        "tpAlgoId",
        "stopAtBreakeven",
        "partialQty",
        "sizingMode",
        "riskPct",
        "notionalUsdt",
        "effectiveLeverage",
        "riskCapped",
    ] {
        assert!(body.contains(key), "{key}");
    }
}

// A row written before the management fields existed still loads.
#[test]
fn legacy_position_body_loads_with_defaults() {
    let legacy = r#"{"signalId":"old","botKind":"spot","exchangeId":"binance","symbol":"BTCUSDT",
        "direction":"long","entry":100.0,"tp":110.0,"sl":95.0,"leverage":1,"capital":50.0,
        "frAtOpen":null,"ldAtOpen":null,"openedAt":5}"#;
    let p: OpenPosition = serde_json::from_str(legacy).unwrap();
    assert_eq!((p.risk_r, p.horizon_ms, p.plan), (0.0, 0, None));
    assert_eq!(
        (p.stop_algo_id, p.tp_algo_id, p.stop_at_breakeven),
        (None, None, false)
    );
    assert_eq!(
        (p.sizing_mode, p.effective_leverage),
        (crate::bot::model::SizingMode::Fixed, 0.0)
    );
    // Opened before take-profit targets existed: it always exited at TP1.
    assert_eq!(
        (p.tp, p.tp_target.as_str(), p.tp_fallback_from),
        (110.0, "tp1", None)
    );
}
