//! QuickJS plumbing for the Pine runner: one fresh Runtime + Context per
//! call, the engine bundle evaluated into it, one `PineHost` function called
//! with a string, a JSON string back.
//!
//! A fresh context per call is the isolation guarantee: nothing a script
//! run leaves behind (globals, caches, a half-finished interpreter after an
//! interrupt) can reach the next run. Building one costs a few ms against a
//! run of tens to hundreds of ms (measured in `bench_tests`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rquickjs::{Context, Ctx, Function, Object, Runtime};

use super::{PineErrorInfo, RUN_WALL_MS};

/// The Pine-lite engine + host entry, built by `npm run pine:bundle`.
const BUNDLE: &str = include_str!("engine.bundle.js");

/// Heap cap per run. 5,000 bars x 64 plots of series stay far below it; a
/// script that allocates without bound fails instead of taking the PC's RAM.
const MEMORY_LIMIT: usize = 256 * 1024 * 1024;

/// The engine is a tree-walking interpreter, so script nesting becomes JS
/// recursion. Each call runs on its own thread with this stack so a deep but
/// legal script behaves as in the browser; QuickJS's own check stops at
/// `JS_STACK` (a catchable RangeError the engine reports as a "limit" error)
/// well before the thread's real end.
const THREAD_STACK: usize = 16 * 1024 * 1024;
const JS_STACK: usize = 8 * 1024 * 1024;

/// Backstop past the engine's own wall-time check (which fires first and
/// reports a "limit" error with the line it stopped on): the interrupt
/// handler aborts a run that is stuck where the engine does not check.
const INTERRUPT_SLACK_MS: u64 = 1_000;

/// Which `PineHost` entry to call.
#[derive(Clone, Copy)]
pub(super) enum Entry {
    Compile,
    Run,
    /// Every output over all bars (parity tests only).
    #[cfg(test)]
    Outputs,
}

impl Entry {
    fn name(self) -> &'static str {
        match self {
            Entry::Compile => "compile",
            Entry::Run => "run",
            #[cfg(test)]
            Entry::Outputs => "outputs",
        }
    }
}

/// The error a run reports when the interrupt handler stopped it.
pub(super) fn wall_limit_error() -> PineErrorInfo {
    PineErrorInfo {
        kind: "limit".into(),
        message: format!("Run stopped: wall time limit of {RUN_WALL_MS} ms exceeded"),
        line: 1,
        col: 1,
    }
}

fn internal(message: String) -> PineErrorInfo {
    PineErrorInfo {
        kind: "runtime".into(),
        message,
        line: 1,
        col: 1,
    }
}

/// Calls `PineHost.<entry>(arg)` in a fresh context on a dedicated thread
/// and returns the JSON string it produced.
pub(super) fn call(entry: Entry, arg: &str) -> Result<String, PineErrorInfo> {
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .name("pine-quickjs".into())
            .stack_size(THREAD_STACK)
            .spawn_scoped(s, || call_here(entry, arg))
            .map_err(|e| internal(format!("Pine runner thread failed to start: {e}")))?
            .join()
            .unwrap_or_else(|_| Err(internal("Pine runner thread panicked".into())))
    })
}

fn call_here(entry: Entry, arg: &str) -> Result<String, PineErrorInfo> {
    let rt = Runtime::new().map_err(|e| internal(format!("QuickJS runtime: {e}")))?;
    rt.set_memory_limit(MEMORY_LIMIT);
    rt.set_max_stack_size(JS_STACK);
    let interrupted = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&interrupted);
    let deadline = Instant::now() + Duration::from_millis(RUN_WALL_MS + INTERRUPT_SLACK_MS);
    rt.set_interrupt_handler(Some(Box::new(move || {
        let over = Instant::now() > deadline;
        if over {
            flag.store(true, Ordering::Relaxed);
        }
        over
    })));
    let ctx = Context::full(&rt).map_err(|e| internal(format!("QuickJS context: {e}")))?;
    let out = ctx.with(|ctx| invoke(&ctx, entry, arg));
    if interrupted.load(Ordering::Relaxed) {
        return Err(wall_limit_error());
    }
    out
}

fn invoke(ctx: &Ctx<'_>, entry: Entry, arg: &str) -> Result<String, PineErrorInfo> {
    let js = |e: rquickjs::Error| internal(format!("Pine engine: {}", describe(ctx, e)));
    ctx.eval::<(), _>(BUNDLE).map_err(js)?;
    let host: Object = ctx.globals().get("PineHost").map_err(js)?;
    let f: Function = host.get(entry.name()).map_err(js)?;
    f.call::<_, String>((arg,)).map_err(js)
}

/// Readable text for a QuickJS error; a thrown exception is fetched from
/// the context (only an engine bug or an out-of-memory gets this far: the
/// engine returns every script error as data).
fn describe(ctx: &Ctx<'_>, e: rquickjs::Error) -> String {
    if !matches!(e, rquickjs::Error::Exception) {
        return e.to_string();
    }
    let v = ctx.catch();
    match v.as_exception() {
        Some(ex) => ex.message().unwrap_or_else(|| "exception".into()),
        None => format!("{v:?}"),
    }
}
