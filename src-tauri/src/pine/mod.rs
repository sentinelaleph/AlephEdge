//! Pine bot script runner: the Pine-lite engine (pine-engine/, a copy of the
//! website's engine) bundled to one JS file and run in an embedded QuickJS
//! context (docs/aleph-edge-pine-lite-spec.md, decision D1-A).
//!
//! The context gets only the engine bundle, the bars and the input
//! overrides: no network, no file system, no `eval` of user text outside the
//! interpreter. Every call is blocking; callers on the async runtime use
//! `tokio::task::spawn_blocking`.
//!
//! INTERFACE CONTRACT (shared by the bot engine and the screens). Keep these
//! names and shapes. QuickJS plumbing is in `runtime`, the JSON shapes
//! exchanged with the bundle's `PineHost` (pine-engine/host.ts) in `host`.

use serde::{Deserialize, Serialize};

#[cfg(test)]
mod bench_tests;
mod host;
#[cfg(test)]
mod parity_tests;
mod runtime;
#[cfg(test)]
mod tests;

/// One closed bar. `t` is the open time in ms since epoch.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PineBar {
    pub t: u64,
    pub o: f64,
    pub h: f64,
    pub l: f64,
    pub c: f64,
    pub v: f64,
}

/// A named Pine-lite error (`kind` is the engine's PineErrorKind:
/// "syntax", "type", "runtime", "limit", "input", "unsupported", ...).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PineErrorInfo {
    pub kind: String,
    pub message: String,
    pub line: u32,
    pub col: u32,
}

/// Which engine output a trigger comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TriggerKind {
    Alert,
    Shape,
}

/// A condition the user can map to Buy / Sell / Close / Ignore.
/// `id` is stable for one script text: "alert#<n>" for the n-th
/// alertcondition and "shape#<n>" for the n-th plotshape / plotchar /
/// plotarrow, both counted from 0 in output order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerInfo {
    pub id: String,
    pub kind: TriggerKind,
    /// The title the script gave (alertcondition title, plot title), or
    /// empty; the screen shows "plotshape #n" for an empty one.
    pub title: String,
    pub line: u32,
}

/// One `input.*` declaration as the engine reports it (InputInfo), kept as
/// JSON so the form can render every input kind without a Rust mirror.
pub type PineInput = serde_json::Value;

/// What a compile found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompileInfo {
    pub ok: bool,
    pub error: Option<PineErrorInfo>,
    /// `indicator(title)` or empty.
    pub title: String,
    pub overlay: bool,
    pub inputs: Vec<PineInput>,
    pub triggers: Vec<TriggerInfo>,
    /// sha256 (hex) of the exact script text.
    pub hash: String,
}

/// Input overrides keyed by the engine's InputInfo.id (as a string key),
/// value as the form sent it. Validated by the engine, never clamped.
pub type InputOverrides = serde_json::Map<String, serde_json::Value>;

/// One run's result on the last closed bar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunVerdict {
    /// Trigger ids whose condition is true on the LAST bar of `bars`.
    pub fired: Vec<String>,
    pub bars: usize,
    pub ms: u64,
    /// Resolved numeric inputs (override or default) by title, for the
    /// stop / target read from the script (spec section 7).
    pub numeric_inputs: Vec<(String, f64)>,
}

/// Wall time one run may take inside QuickJS (spec section 9).
pub const RUN_WALL_MS: u64 = 3_000;

/// sha256 hex of a script text.
pub fn script_hash(source: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(source.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Compiles `source` and lists its inputs and triggers.
pub fn compile(source: &str) -> CompileInfo {
    let hash = script_hash(source);
    let failed = |error: PineErrorInfo| CompileInfo {
        ok: false,
        error: Some(error),
        title: String::new(),
        overlay: false,
        inputs: Vec::new(),
        triggers: Vec::new(),
        hash: hash.clone(),
    };
    let reply = match runtime::call(runtime::Entry::Compile, source) {
        Ok(json) => json,
        Err(e) => return failed(e),
    };
    match serde_json::from_str::<host::CompileReply>(&reply) {
        Ok(r) => CompileInfo {
            ok: r.ok,
            error: r.error,
            title: r.title,
            overlay: r.overlay,
            inputs: r.inputs,
            triggers: r.triggers,
            hash,
        },
        Err(e) => failed(host::reply_error(e)),
    }
}

/// Runs `source` over `bars` (oldest first, all closed) and reports which
/// triggers are true on the last bar. `symbol` and `interval_ms` feed
/// syminfo / timeframe (`interval_ms` 0 = derived from the bars).
///
/// Empty `bars` is not an error: nothing runs, nothing fires.
pub fn run_last_bar(
    source: &str,
    bars: &[PineBar],
    inputs: &InputOverrides,
    symbol: &str,
    interval_ms: u64,
) -> Result<RunVerdict, PineErrorInfo> {
    host::check_bars(bars)?;
    let started = std::time::Instant::now();
    let args = host::run_args(source, bars, inputs, symbol, interval_ms);
    let reply = runtime::call(runtime::Entry::Run, &args)?;
    let r = host::parse_run(&reply)?;
    Ok(RunVerdict {
        fired: r.fired,
        bars: r.bars,
        ms: started.elapsed().as_millis() as u64,
        numeric_inputs: r.numeric_inputs,
    })
}
