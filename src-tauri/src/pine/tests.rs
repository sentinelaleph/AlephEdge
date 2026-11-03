//! Runner behaviour: compile listing, errors, limits, inputs. Engine output
//! parity with node is in `parity_tests`.

use std::time::Instant;

use super::*;

const EMA_CROSS: &str = r#"//@version=6
indicator("EMA cross", overlay = true)
fastLen = input.int(9, "Fast")
slowLen = input.int(21, "Slow")
fast = ta.ema(close, fastLen)
slow = ta.ema(close, slowLen)
plot(fast, "Fast", color.aqua)
plot(slow, "Slow", color.orange)
bull = ta.crossover(fast, slow)
bear = ta.crossunder(fast, slow)
plotshape(bull, "Buy", shape.triangleup, location.belowbar, color.green, size = size.small)
plotshape(bear, "Sell", shape.triangledown, location.abovebar, color.red, size = size.small)
alertcondition(bull or bear, "Cross", "EMA cross")
"#;

/// Deterministic hourly bars: a slow wave plus LCG noise, always positive.
pub(super) fn wave_bars(n: usize) -> Vec<PineBar> {
    let mut seed: u64 = 42;
    let mut noise = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((seed >> 33) as f64 / (1u64 << 31) as f64) - 0.5
    };
    let mut prev = 100.0;
    (0..n)
        .map(|i| {
            let c = 100.0 + 10.0 * (i as f64 / 25.0).sin() + 3.0 * noise();
            let (o, spread) = (prev, 0.5 + noise().abs());
            prev = c;
            PineBar {
                t: 1_767_225_600_000 + i as u64 * 3_600_000,
                o,
                h: o.max(c) + spread,
                l: o.min(c) - spread,
                c,
                v: 100.0 + 50.0 * noise(),
            }
        })
        .collect()
}

fn run(source: &str, bars: &[PineBar]) -> Result<RunVerdict, PineErrorInfo> {
    run_last_bar(source, bars, &InputOverrides::new(), "BTCUSDT", 3_600_000)
}

#[test]
fn compile_lists_title_inputs_and_triggers_in_output_order() {
    let c = compile(EMA_CROSS);
    assert!(c.ok, "{:?}", c.error);
    assert_eq!(
        (c.title.as_str(), c.overlay, c.inputs.len()),
        ("EMA cross", true, 2)
    );
    assert_eq!(c.inputs[0]["title"], "Fast");
    assert_eq!(c.hash, script_hash(EMA_CROSS));
    let t: Vec<(&str, TriggerKind, &str, u32)> = c
        .triggers
        .iter()
        .map(|t| (t.id.as_str(), t.kind, t.title.as_str(), t.line))
        .collect();
    assert_eq!(
        t,
        vec![
            ("shape#0", TriggerKind::Shape, "Buy", 11),
            ("shape#1", TriggerKind::Shape, "Sell", 12),
            ("alert#0", TriggerKind::Alert, "Cross", 13)
        ]
    );
}

#[test]
fn compile_errors_are_named() {
    let c = compile("//@version=6\nindicator(\"x\")\nplot(close +)\n");
    assert!(!c.ok);
    let e = c.error.unwrap();
    assert_eq!((e.kind.as_str(), e.line), ("syntax", 3));
    assert!(c.triggers.is_empty());
}

#[test]
fn strategy_scripts_get_the_engines_unsupported_error() {
    let src = "//@version=6\nstrategy(\"s\")\nstrategy.entry(\"L\", strategy.long)\n";
    let c = compile(src);
    assert_eq!(
        c.error.as_ref().map(|e| e.kind.as_str()),
        Some("unsupported")
    );
    let e = run(src, &wave_bars(50)).unwrap_err();
    assert_eq!(e.kind, "unsupported");
    assert!(e.message.contains("strategy"), "{}", e.message);
}

#[test]
fn empty_bars_run_nothing_and_fire_nothing() {
    let v = run(EMA_CROSS, &[]).unwrap();
    assert_eq!((v.bars, v.fired.len()), (0, 0));
}

#[test]
fn non_finite_bars_are_rejected_before_the_run() {
    let mut bars = wave_bars(10);
    bars[4].c = f64::NAN;
    let e = run(EMA_CROSS, &bars).unwrap_err();
    assert_eq!(
        (e.kind.as_str(), e.message.as_str()),
        ("runtime", "Bar 4 has a missing or non-finite value")
    );
    bars[4].c = 1.0;
    bars[7].v = f64::INFINITY;
    assert_eq!(
        run(EMA_CROSS, &bars).unwrap_err().message,
        "Bar 7 has a missing or non-finite value"
    );
}

#[test]
fn last_bar_only_and_numeric_inputs_resolved() {
    let bars = wave_bars(400);
    let mut inputs = InputOverrides::new();
    inputs.insert("0".into(), serde_json::json!(5));
    let v = run_last_bar(EMA_CROSS, &bars, &inputs, "BTCUSDT", 3_600_000).unwrap();
    assert_eq!(v.bars, 400);
    assert_eq!(
        v.numeric_inputs,
        vec![("Fast".to_string(), 5.0), ("Slow".to_string(), 21.0)]
    );
    // The alert fires exactly when one of the two shapes does.
    let shapes = v.fired.iter().any(|f| f.starts_with("shape#"));
    assert_eq!(shapes, v.fired.contains(&"alert#0".to_string()));
}

#[test]
fn a_bad_input_override_is_an_input_error() {
    let mut inputs = InputOverrides::new();
    inputs.insert("1".into(), serde_json::json!(2.5));
    let e = run_last_bar(EMA_CROSS, &wave_bars(50), &inputs, "BTCUSDT", 3_600_000).unwrap_err();
    assert_eq!(e.kind, "input");
    assert!(e.message.contains("Slow"), "{}", e.message);
}

#[test]
fn a_run_over_the_wall_time_limit_stops_with_a_limit_error() {
    // ~100k loop steps per bar: legal under the node and loop caps, far too
    // slow for 1,000 bars inside RUN_WALL_MS.
    let src = "//@version=6\nindicator(\"slow\")\ns = 0.0\nfor i = 0 to 9999\n    for j = 0 to 9\n        s := s + j\nplot(s)\n";
    let started = Instant::now();
    let e = run(src, &wave_bars(1_000)).unwrap_err();
    let ms = started.elapsed().as_millis() as u64;
    assert_eq!(e.kind, "limit", "{e:?}");
    assert!(e.message.contains("wall time"), "{}", e.message);
    assert!(ms < RUN_WALL_MS + 2_000, "stopped after {ms} ms");
}

#[test]
fn runs_are_isolated() {
    // A script that changes nothing global cannot leak, but a run after a
    // failed / interrupted run must still see a clean engine.
    let bars = wave_bars(200);
    let before = run(EMA_CROSS, &bars).unwrap();
    let _ = run("//@version=6\nindicator(\"x\")\nplot(close +)\n", &bars);
    assert_eq!(run(EMA_CROSS, &bars).unwrap().fired, before.fired);
}

/// The owner's testnet script (2 Nov 2026): Buy on even bar_index, Close on
/// odd. The bot re-runs on the last N closed bars, and with bar_index counted
/// from the window start the newest bar was N - 1 on every run, so it fired
/// Close on every bar. Anchored to the bar time it alternates bar by bar.
#[test]
fn bar_index_grows_by_one_per_bar_across_sliding_windows() {
    const PARITY: &str = r#"//@version=6
indicator("Parity")
alertcondition(bar_index % 2 == 0, "Buy", "B")
alertcondition(bar_index % 2 == 1, "Close", "C")
"#;
    let bars = wave_bars(1_010);
    let fired: Vec<Vec<String>> = (0..4)
        .map(|k| run(PARITY, &bars[k..k + 1_000]).unwrap().fired)
        .collect();
    assert_ne!(fired[0], fired[1]);
    assert_eq!(fired[0], fired[2]);
    assert_eq!(fired[1], fired[3]);
    // The same closed bar gets the same bar_index whatever the window length.
    assert_eq!(run(PARITY, &bars[500..1_000]).unwrap().fired, fired[0]);
}
