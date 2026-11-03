/**
 * Pine-lite public API (main thread).
 *
 *   compile(source)                 -> CompileResult   static analysis only (no bar runs):
 *                                                      meta, inputs (for the inputs form), warnings, errors
 *   runAsync(source, bars, options) -> Promise<RunResult>  interprets the script in a Web Worker;
 *                                                      falls back to runSync where Worker is unavailable (tests, SSR)
 *   runSync(source, bars, options)  -> RunResult       same, in the calling thread (tests / worker)
 *   formatPineError(e)              -> "line 12:5 — ta.mode is not supported in Pine-lite v1"
 *
 * Results are plain data. A successful RunResult carries:
 *   meta     ScriptMeta   title, shorttitle, overlay (true = price pane, false = own pane), format, precision, version
 *   inputs   InputInfo[]  id (override key), kind, title, defval, minval/maxval/step/options, tooltip/group/inline, line/col
 *   outputs  Output[]     in source order; each per-bar array is index-aligned with `bars`, null = nothing at that bar:
 *     plot            values[], colors[], style, linewidth, offset, histbase, showLast, display
 *     plotshape/char/arrow  values[] (1 = marker; price for location "absolute"; signed value for plotarrow),
 *                     colors[], textColors[], style/char, location, size, text, offset
 *     hline           price, color, linestyle, linewidth
 *     fill            between "plots" | "hlines", a, b (output ids), colors[]
 *     bgcolor/barcolor colors[], offset
 *     alertcondition  fired: bar indices, title, message
 *   warnings PineError[]  (kind "warning")
 *   stats    { bars, ms, nodes }
 * A failed result carries errors: PineError[] ({kind, message, line, col, hint?}).
 * Colours are "#RRGGBBAA". `offset` is reported, not applied.
 */
import type { Bar } from "@/lib/ta/core";

import { limitsOf, runSync, timeoutError, type RunOptions, type RunResult } from "./engine";
import { pineError } from "./errors";
import type { RunRequest, RunResponse } from "./protocol";

export { compile, runSync, type CompileOptions, type CompileResult, type RunOptions, type RunResult, type RunStats } from "./engine";
export { formatPineError, type PineError, type PineErrorKind } from "./errors";
export type { ScriptMeta } from "./checkTypes";
export type { InputInfo, InputKind, InputOverride } from "./runtime/builtins/input";
export type { HostInfo } from "./runtime/builtins/types";
export type { Limits } from "./runtime/evaluator";
export { DEFAULT_LIMITS } from "./runtime/evaluator";
export type * from "./outputs";
export { FUNCTIONS, VARIABLES, CONSTANT_VALUES } from "./registry";

/** The subset of Worker the runner uses (injectable for tests). */
export interface WorkerLike {
  onmessage: ((e: MessageEvent<RunResponse>) => void) | null;
  onerror: ((e: unknown) => void) | null;
  postMessage(message: RunRequest): void;
  terminate(): void;
}

interface Pending {
  resolve: (r: RunResult) => void;
  timer: ReturnType<typeof setTimeout>;
}

/**
 * Runs scripts in a dedicated worker. A run that exceeds its wall-time limit
 * plus `slackMs` terminates the worker (the interpreter's own deadline should
 * fire first; this is the backstop for a stuck worker) and the next run gets
 * a fresh one.
 */
export class PineRunner {
  private worker: WorkerLike | null = null;
  private pending = new Map<number, Pending>();
  private nextId = 1;

  constructor(
    private readonly createWorker: () => WorkerLike = defaultWorker,
    private readonly slackMs = 1000,
  ) {}

  run(source: string, bars: Bar[], options: RunOptions = {}): Promise<RunResult> {
    const w = this.ensure();
    const id = this.nextId++;
    const ms = limitsOf(options.limits).maxWallMs;
    return new Promise<RunResult>((resolve) => {
      const timer = setTimeout(() => this.restart({ ok: false, errors: [timeoutError(ms)], warnings: [] }), ms + this.slackMs);
      this.pending.set(id, { resolve, timer });
      w.postMessage({ id, source, bars, options });
    });
  }

  dispose(): void {
    this.restart({ ok: false, errors: [pineError("runtime", "Pine-lite runner was stopped", 1, 1)], warnings: [] });
  }

  private ensure(): WorkerLike {
    if (this.worker) return this.worker;
    const w = this.createWorker();
    w.onmessage = (e) => {
      const p = this.pending.get(e.data.id);
      if (!p) return;
      clearTimeout(p.timer);
      this.pending.delete(e.data.id);
      p.resolve(e.data.result);
    };
    w.onerror = () => this.restart({ ok: false, errors: [pineError("runtime", "The Pine-lite worker crashed", 1, 1)], warnings: [] });
    this.worker = w;
    return w;
  }

  /** Terminates the worker and settles every pending run with `result`. */
  private restart(result: RunResult): void {
    this.worker?.terminate();
    this.worker = null;
    for (const p of this.pending.values()) {
      clearTimeout(p.timer);
      p.resolve(result);
    }
    this.pending.clear();
  }
}

function defaultWorker(): WorkerLike {
  return new Worker(new URL("./worker.ts", import.meta.url), { type: "module" }) as unknown as WorkerLike;
}

let shared: PineRunner | null = null;

/** Runs in a Web Worker when available, otherwise synchronously (tests, SSR). */
export function runAsync(source: string, bars: Bar[], options: RunOptions = {}): Promise<RunResult> {
  if (typeof Worker === "undefined") return Promise.resolve(runSync(source, bars, options));
  shared ??= new PineRunner();
  return shared.run(source, bars, options);
}
