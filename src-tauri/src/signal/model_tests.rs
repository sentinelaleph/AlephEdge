//! Wire-contract tests against the Sentinel payload as of 2026-09-16, plus
//! the timestamp parser, regeneration buffer and backfill body.

use super::backfill::{parse_pending_body, Row};
use super::buffer::SignalBuffer;
use super::model::{Direction, RegimeVetoEvent, Signal};
use super::time::parse_rfc3339_ms;

const REALISTIC: &str = r#"{
    "id": "c0ffee-1", "symbol": "AVAXUSDT", "timeframe": "4h",
    "direction": "short", "mode": "hybrid",
    "entry": 24.10, "tp": [23.55, 23.02, 22.40], "sl": 25.02,
    "confidence": 0.64, "regime": "Bear",
    "confluence": [{"source": "ob", "score": 0.81, "weighted": 0.29},
                   {"source": "fvg", "score": 0.6, "weighted": 0.18}],
    "rr": 0.6, "combo": "stophunt_snap", "investment_score": 41.5,
    "regenerated": true, "regenerated_at": "2026-09-16T08:00:01.123456789Z",
    "management_plan": {"breakeven_at_r": 0.5, "partial_at_r": 1.0, "partial_fraction": 0.5},
    "instrumentation": {"atr": 0.368, "gate": "short_v2", "flags": [1, 2]},
    "expires_at": "2026-09-17T08:00:00Z",
    "created_at": "2026-09-16T08:00:00.123456789Z"
}"#;

fn variant(edit: impl FnOnce(&mut serde_json::Value)) -> Result<Signal, serde_json::Error> {
    let mut v: serde_json::Value = serde_json::from_str(REALISTIC).unwrap();
    edit(&mut v);
    serde_json::from_value(v)
}

#[test]
fn parses_realistic_short_with_plan() {
    let s: Signal = serde_json::from_str(REALISTIC).expect("parse");
    assert_eq!(s.direction, Direction::Short);
    assert_eq!(s.tp.len(), 3);
    assert_eq!(s.tp1(), Some(23.55));
    assert!(s.regenerated);
    assert_eq!(s.confluence[1].source, "fvg");
    let plan = s.management_plan.expect("plan");
    assert_eq!(
        (plan.breakeven_at_r, plan.partial_at_r),
        (Some(0.5), Some(1.0))
    );
    assert_eq!(plan.partial_fraction, Some(0.5));
    assert!(s.instrumentation.unwrap().contains_key("atr"));
}

#[test]
fn tolerates_nulls_unknown_mode_and_offsets() {
    let s = variant(|v| v["confluence"] = serde_json::Value::Null).unwrap();
    assert!(s.confluence.is_empty());
    let s = variant(|v| v["tp"] = serde_json::Value::Null).unwrap();
    assert!(s.tp.is_empty() && s.tp1().is_none());
    let s = variant(|v| v["mode"] = "quantum_v9".into()).unwrap();
    assert_eq!(s.mode, "quantum_v9");
    let s = variant(|v| {
        v["created_at"] = "2026-09-16T11:00:00+03:00".into();
        for k in ["management_plan", "instrumentation", "regenerated", "combo"] {
            v.as_object_mut().unwrap().remove(k);
        }
    })
    .unwrap();
    assert!(!s.regenerated && s.management_plan.is_none());
    assert_eq!(
        parse_rfc3339_ms(&s.created_at),
        parse_rfc3339_ms("2026-09-16T08:00:00Z")
    );
}

#[test]
fn rfc3339_offsets_and_fraction_lengths() {
    let base = parse_rfc3339_ms("2026-09-16T08:00:00Z").unwrap();
    assert_eq!(
        parse_rfc3339_ms("2026-09-16T08:00:00.123456789Z"),
        Some(base + 123)
    );
    assert_eq!(parse_rfc3339_ms("2026-09-16T08:00:00.5Z"), Some(base + 500));
    assert_eq!(parse_rfc3339_ms("2026-09-16T11:00:00+03:00"), Some(base));
    assert_eq!(
        parse_rfc3339_ms("2026-09-16T03:30:00.25-04:30"),
        Some(base + 250)
    );
    assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:01Z"), Some(1000));
    for bad in [
        "",
        "2026-09-16",
        "2026-09-16T08:00:00",
        "2026-09-16T08:00:00+0300",
        "2026-13-01T00:00:00Z",
    ] {
        assert_eq!(parse_rfc3339_ms(bad), None, "{bad}");
    }
}

fn buffered(id: &str, created: &str, regenerated: bool) -> Signal {
    let mut s: Signal = serde_json::from_str(REALISTIC).unwrap();
    s.id = id.into();
    s.created_at = created.into();
    s.expires_at = "2099-01-01T00:00:00Z".into();
    s.regenerated = regenerated;
    s
}

#[test]
fn regeneration_supersedes_older_same_market_signals() {
    let mut buf = SignalBuffer::default();
    assert!(
        buf.push(buffered("old", "2026-09-16T04:00:00Z", false), 0)
            .added
    );
    let mut other_tf = buffered("other", "2026-09-16T04:00:00Z", false);
    other_tf.timeframe = Some("1h".into());
    buf.push(other_tf, 0);
    let out = buf.push(buffered("new", "2026-09-16T08:00:00Z", true), 0);
    assert!(out.added);
    assert_eq!(
        out.superseded
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        ["old"]
    );
    // A late backfill copy of the old signal cannot come back.
    assert!(
        !buf.push(buffered("old", "2026-09-16T04:00:00Z", false), 0)
            .added
    );
    let ids: Vec<_> = buf.recent().into_iter().map(|s| s.id).collect();
    assert_eq!(ids, ["new", "other"]);
}

#[test]
fn backfill_body_keeps_good_rows_and_reports_bad_ones() {
    let body = format!(
        r#"{{"signals":[{REALISTIC},{{"id":"x9","symbol":"DOGEUSDT"}}],"data":[],"total":2}}"#
    );
    let rows = parse_pending_body(&body).unwrap();
    assert_eq!(rows.len(), 2);
    assert!(matches!(&rows[0], Row::Ok(s) if s.id == "c0ffee-1"));
    assert!(matches!(&rows[1], Row::Bad { id, symbol, .. } if id == "x9" && symbol == "DOGEUSDT"));
    let data_only = format!(r#"{{"signals":[],"data":[{REALISTIC}],"total":1}}"#);
    assert_eq!(parse_pending_body(&data_only).unwrap().len(), 1);
}

#[test]
fn pending_page_carries_the_total_and_the_buffer_drops_by_id() {
    let body = format!(r#"{{"signals":[{REALISTIC}],"total":7}}"#);
    let page = super::backfill::parse_pending_page(&body).unwrap();
    assert_eq!((page.rows.len(), page.total), (1, Some(7)));
    let no_total = format!(r#"{{"signals":[{REALISTIC}]}}"#);
    assert_eq!(super::backfill::parse_pending_page(&no_total).unwrap().total, None);

    let mut buf = SignalBuffer::default();
    let Row::Ok(sig) = page.rows.into_iter().next().unwrap() else { panic!("realistic row") };
    buf.push(*sig, 0);
    assert_eq!(buf.id_created().len(), 1);
    let removed = buf.remove_ids(&["c0ffee-1".to_string()]);
    assert_eq!(removed.len(), 1);
    assert!(buf.recent().is_empty());
}

// The NEW "veto" event (2026-09-18): wire-contract parsing. Bookkeeping
// (dedup/bounding) is tested separately in regime_veto.rs; the actual close
// is exercised via `bot::engine::book`'s own pure tests.
const VETO: &str = r#"{
    "signal_id": "sig-1", "symbol": "ETHUSDT", "direction": "short",
    "reason_code": "btc_regime_turn_bullish",
    "reason_text": "BTC turned bullish for 60 min (strength 0.21) — contra-trend short vetoed",
    "trigger": "regime_turn", "btc_price": 77541.9, "btc_trend": "bullish",
    "btc_strength": 0.21, "btc_change_1h_pct": 1.4,
    "vetoed_at": "2026-09-18T11:02:03.123456789Z"
}"#;

#[test]
fn veto_event_deserializes_the_full_wire_contract() {
    let event: RegimeVetoEvent = serde_json::from_str(VETO).expect("parses");
    assert_eq!(event.signal_id, "sig-1");
    assert_eq!(event.symbol, "ETHUSDT");
    assert_eq!(event.direction, Some(Direction::Short));
    assert_eq!(event.reason_code, "btc_regime_turn_bullish");
    assert_eq!(event.trigger, "regime_turn");
    assert!((event.btc_price - 77541.9).abs() < 1e-9);
    assert!((event.btc_strength - 0.21).abs() < 1e-9);
    assert!((event.btc_change_1h_pct - 1.4).abs() < 1e-9);
    assert_eq!(event.vetoed_at, "2026-09-18T11:02:03.123456789Z");
}

#[test]
fn veto_event_rejects_malformed_or_incomplete_payloads() {
    assert!(serde_json::from_str::<RegimeVetoEvent>("not json").is_err());
    // Missing the required `signal_id` and `reason_code`.
    assert!(serde_json::from_str::<RegimeVetoEvent>(r#"{"symbol":"ETHUSDT"}"#).is_err());
}

// `reason_code` stays a plain String (like `Signal.mode`) on purpose: an
// unmapped code upstream must still parse, never drop the whole event.
#[test]
fn veto_event_tolerates_an_unknown_reason_code() {
    let payload = VETO.replace("btc_regime_turn_bullish", "some_future_reason_v2");
    let event: RegimeVetoEvent = serde_json::from_str(&payload).expect("parses");
    assert_eq!(event.reason_code, "some_future_reason_v2");
}

/// Sentinel serves the pending list NEWEST first. Pushed in that order, the
/// newest signal was read back as the OLDEST, so bots walked a backfill from
/// the stalest setup up instead of freshest first (owner rule 2026-09-23).
#[test]
fn recent_is_newest_first_whatever_the_arrival_order() {
    let mut buf = SignalBuffer::default();
    // Page order: newest first, mixed offsets and fraction lengths, so text
    // order would be wrong too ("10:30+02:00" is 08:30Z, older than 09:00Z).
    for (id, created) in [
        ("newest", "2026-09-16T09:15:00.5Z"),
        ("middle", "2026-09-16T09:00:00Z"),
        ("oldest", "2026-09-16T10:30:00.123456+02:00"),
    ] {
        buf.push(buffered(id, created, false), 0);
    }
    let ids: Vec<_> = buf.recent().into_iter().map(|s| s.id).collect();
    assert_eq!(ids, ["newest", "middle", "oldest"]);
}

/// The batch itself is put in publication order before it is buffered, so
/// arrival order (and cap eviction) match time.
#[test]
fn a_pending_page_is_ingested_oldest_first_by_parsed_time() {
    let mk = |id: &str, created: &str| {
        Row::Ok(Box::new(buffered(id, created, false)))
    };
    let mut rows = vec![
        mk("b", "2026-09-16T09:15:00Z"),
        mk("c", "2026-09-16T10:30:00+02:00"),
        Row::Bad { id: "x".into(), symbol: "—".into(), error: "bad".into() },
        mk("a", "2026-09-16T09:15:00.999Z"),
    ];
    super::backfill::chronological(&mut rows);
    let order: Vec<String> = rows
        .iter()
        .map(|r| match r {
            Row::Ok(s) => s.id.clone(),
            Row::Bad { id, .. } => id.clone(),
        })
        .collect();
    assert_eq!(order, ["x", "c", "b", "a"]);
}

/// At the cap, the oldest by publication time goes — not whichever arrived
/// first, which for a newest-first page was the newest signal.
#[test]
fn the_cap_evicts_the_oldest_signal_not_the_first_arrival() {
    let mut buf = SignalBuffer::default();
    buf.push(buffered("newest", "2026-09-16T23:59:00Z", false), 0);
    for i in 0..super::buffer::BUFFER_CAP {
        buf.push(buffered(&format!("s{i}"), "2026-09-16T01:00:00Z", false), 0);
    }
    assert!(buf.recent().iter().any(|s| s.id == "newest"), "newest evicted");
}
