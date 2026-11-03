//! The JSON exchanged with the bundle's `PineHost` (pine-engine/host.ts):
//! arguments out, replies in. Field names follow host.ts; the shapes mirror
//! the public structs in `mod.rs`.

use serde::Deserialize;

use super::{InputOverrides, PineBar, PineErrorInfo, PineInput, TriggerInfo, RUN_WALL_MS};

/// `PineHost.compile` reply: CompileInfo without the hash.
#[derive(Debug, Deserialize)]
pub(super) struct CompileReply {
    pub ok: bool,
    pub error: Option<PineErrorInfo>,
    pub title: String,
    pub overlay: bool,
    pub inputs: Vec<PineInput>,
    pub triggers: Vec<TriggerInfo>,
}

/// `PineHost.run` reply on success.
#[derive(Debug, Deserialize)]
pub(super) struct RunOk {
    pub fired: Vec<String>,
    pub bars: usize,
    pub numeric_inputs: Vec<(String, f64)>,
}

#[derive(Deserialize)]
struct RunReply {
    ok: bool,
    error: Option<PineErrorInfo>,
    #[serde(flatten)]
    result: Option<RunOk>,
}

/// A reply the runner could not read (a bundle / runner mismatch).
pub(super) fn reply_error(e: serde_json::Error) -> PineErrorInfo {
    PineErrorInfo {
        kind: "runtime".into(),
        message: format!("Pine runner: unreadable engine reply ({e})"),
        line: 1,
        col: 1,
    }
}

/// Rejects non-finite bar values before anything runs: JSON has no NaN or
/// Infinity, so they would reach the engine as `null` and change meaning.
/// Same kind and message as the engine's own check.
pub(super) fn check_bars(bars: &[PineBar]) -> Result<(), PineErrorInfo> {
    match bars
        .iter()
        .position(|b| ![b.o, b.h, b.l, b.c, b.v].iter().all(|x| x.is_finite()))
    {
        None => Ok(()),
        Some(i) => Err(PineErrorInfo {
            kind: "runtime".into(),
            message: format!("Bar {i} has a missing or non-finite value"),
            line: 1,
            col: 1,
        }),
    }
}

/// `PineHost.run` arguments. `interval_ms` 0 goes out as null so the
/// engine derives the timeframe from the bars.
pub(super) fn run_args(
    source: &str,
    bars: &[PineBar],
    inputs: &InputOverrides,
    symbol: &str,
    interval_ms: u64,
) -> String {
    serde_json::json!({
        "source": source,
        "bars": bars,
        "inputs": inputs,
        "symbol": symbol,
        "intervalMs": if interval_ms == 0 { None } else { Some(interval_ms) },
        "wallMs": RUN_WALL_MS,
    })
    .to_string()
}

/// Reads a `PineHost.run` reply. The engine catches an out-of-memory
/// inside its own error boundary and calls it an internal runtime error;
/// here it becomes the "limit" it is.
pub(super) fn parse_run(reply: &str) -> Result<RunOk, PineErrorInfo> {
    let r: RunReply = serde_json::from_str(reply).map_err(reply_error)?;
    match (r.ok, r.error, r.result) {
        (true, _, Some(ok)) => Ok(ok),
        (_, Some(mut e), _) => {
            if e.kind == "runtime" && e.message.contains("out of memory") {
                e.kind = "limit".into();
            }
            Err(e)
        }
        _ => Err(PineErrorInfo {
            kind: "runtime".into(),
            message: "Pine runner: engine reply without a result".into(),
            line: 1,
            col: 1,
        }),
    }
}
