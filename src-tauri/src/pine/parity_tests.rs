//! QuickJS == node: the fixture (fixtures/parity.json, written by
//! scripts/pine-fixtures.cjs from the same bundle under node) holds, for
//! each engine golden script and a few trigger / error scripts, node's
//! compile reply, every plot / marker / alert over 300 bars, and the
//! last-bar verdict on several bar prefixes. Each is recomputed here in
//! QuickJS. Plot values may differ in the last bits (QuickJS and V8 have
//! their own Math.exp / Math.pow), so numbers compare within 1e-9 relative,
//! as the engine's own vitest helper does; everything else is exact.

use serde::Deserialize;
use serde_json::Value;

use super::runtime::{call, Entry};
use super::*;

const FIXTURE: &str = include_str!("fixtures/parity.json");

#[derive(Deserialize)]
struct Fixture {
    bars: Vec<PineBar>,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    source: String,
    inputs: InputOverrides,
    compile: Value,
    outputs: Value,
    runs: Vec<PrefixRun>,
}

#[derive(Deserialize)]
struct PrefixRun {
    n: usize,
    reply: Value,
}

fn fixture() -> Fixture {
    serde_json::from_str(FIXTURE).expect("parity.json")
}

/// Paths where `a` and `b` differ (numbers within 1e-9 relative).
fn diffs(path: &str, a: &Value, b: &Value, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            if (x - y).abs() > 1e-9 * x.abs().max(y.abs()).max(1.0) {
                out.push(format!("{path}: {x} vs {y}"));
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            x.iter()
                .zip(y)
                .enumerate()
                .for_each(|(i, (x, y))| diffs(&format!("{path}[{i}]"), x, y, out));
        }
        (Value::Object(x), Value::Object(y)) if x.len() == y.len() => {
            for (k, v) in x {
                match y.get(k) {
                    Some(w) => diffs(&format!("{path}.{k}"), v, w, out),
                    None => out.push(format!("{path}.{k}: missing")),
                }
            }
        }
        _ if a == b => {}
        _ => out.push(format!("{path}: {a} vs {b}")),
    }
}

fn assert_same(what: &str, quickjs: &Value, node: &Value) {
    let mut out = Vec::new();
    diffs(what, quickjs, node, &mut out);
    assert!(
        out.is_empty(),
        "{} difference(s), first: {:?}",
        out.len(),
        &out[..out.len().min(5)]
    );
}

/// The public verdict as the host's run reply shape (minus run time).
fn verdict_json(r: Result<RunVerdict, PineErrorInfo>) -> Value {
    match r {
        Ok(v) => {
            serde_json::json!({ "ok": true, "fired": v.fired, "bars": v.bars, "numeric_inputs": v.numeric_inputs })
        }
        Err(e) => serde_json::json!({ "ok": false, "error": e }),
    }
}

#[test]
fn fixture_covers_the_trigger_kinds() {
    let f = fixture();
    assert!(f.cases.len() >= 15 && f.bars.len() == 300);
    let fired: Vec<&Value> = f
        .cases
        .iter()
        .flat_map(|c| &c.runs)
        .flat_map(|r| r.reply["fired"].as_array())
        .flatten()
        .collect();
    assert!(fired
        .iter()
        .any(|v| v.as_str().unwrap().starts_with("alert#")));
    assert!(fired
        .iter()
        .any(|v| v.as_str().unwrap().starts_with("shape#")));
}

#[test]
fn compile_matches_node() {
    for c in fixture().cases {
        let mut got = serde_json::to_value(compile(&c.source)).unwrap();
        got.as_object_mut().unwrap().remove("hash");
        assert_same(&format!("{} compile", c.name), &got, &c.compile);
    }
}

#[test]
fn every_output_matches_node_bar_by_bar() {
    let f = fixture();
    for c in &f.cases {
        let args = host::run_args(&c.source, &f.bars, &c.inputs, "BTCUSDT", 3_600_000);
        let got: Value = serde_json::from_str(&call(Entry::Outputs, &args).unwrap()).unwrap();
        assert_same(&format!("{} outputs", c.name), &got, &c.outputs);
    }
}

#[test]
fn last_bar_verdicts_match_node_on_every_prefix() {
    let f = fixture();
    for c in &f.cases {
        for r in &c.runs {
            let got = verdict_json(run_last_bar(
                &c.source,
                &f.bars[..r.n],
                &c.inputs,
                "BTCUSDT",
                3_600_000,
            ));
            assert_same(&format!("{} run n={}", c.name, r.n), &got, &r.reply);
        }
    }
}
