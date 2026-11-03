//! Run-time measurement (spec section 9: estimate 0.3-2 s per run on 1,000
//! bars, limit 3 s). Not a gate; run in release and read the printout:
//!
//!   cargo test --release --lib pine::bench_tests -- --ignored --nocapture

use std::time::Instant;

use serde_json::Value;

use super::tests::wave_bars;
use super::*;

const RUNS: usize = 5;

/// Median wall time of `RUNS` calls, ms.
fn median_ms(mut f: impl FnMut()) -> f64 {
    let mut t: Vec<f64> = (0..RUNS)
        .map(|_| {
            let s = Instant::now();
            f();
            s.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    t.sort_by(|a, b| a.partial_cmp(b).unwrap());
    t[RUNS / 2]
}

#[test]
#[ignore = "timing; run in release with --ignored --nocapture"]
fn run_last_bar_on_1000_bars() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/parity.json")).unwrap();
    let source = |name: &str| -> String {
        let c = fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap();
        c["source"].as_str().unwrap().to_string()
    };
    let bars = wave_bars(1_000);
    let none = InputOverrides::new();
    let overhead = median_ms(|| {
        drop(run_last_bar(
            "//@version=6\nindicator(\"x\")\nplot(close)\n",
            &[],
            &none,
            "X",
            3_600_000,
        ))
    });
    println!("fresh context + bundle + compile, 0 bars: {overhead:.1} ms");
    for name in [
        "EMA_CROSS",
        "RSI_SIGNALS",
        "MACD_ALERT",
        "SUPERTREND",
        "PIVOTS",
        "BB_SQUEEZE",
        "STOCH_RSI",
        "USER_RSI",
    ] {
        let src = source(name);
        let mut fired = 0;
        let ms = median_ms(|| {
            fired = run_last_bar(&src, &bars, &none, "BTCUSDT", 3_600_000)
                .unwrap()
                .fired
                .len()
        });
        println!("{name:<12} 1,000 bars: {ms:>7.1} ms (median of {RUNS})");
        assert!(ms < RUN_WALL_MS as f64);
    }
}
