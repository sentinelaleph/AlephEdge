/**
 * Synchronous pipeline: compile (lex -> parse -> check) and run (resolve
 * inputs -> interpret every bar). Used directly by tests and by the Web
 * Worker; the browser UI goes through index.ts, which runs this in the
 * worker. Nothing here throws: every failure comes back as PineError data.
 */
import type { Bar } from "@/lib/ta/core";

import type { CheckedScript, ScriptMeta } from "./checkTypes";
import { check } from "./checker";
import { fail, pineError, toPineError, type PineError } from "./errors";
import type { Output } from "./outputs";
import { parse } from "./parser";
import { resolveInputs, type InputInfo, type InputOverride } from "./runtime/builtins/input";
import type { HostInfo } from "./runtime/builtins/types";
import { DEFAULT_LIMITS, type Limits } from "./runtime/evaluator";
import { Interpreter } from "./runtime/interpreter";

export interface CompileOptions {
  limits?: Partial<Limits>;
}

export type CompileResult =
  | { ok: true; meta: ScriptMeta; inputs: InputInfo[]; warnings: PineError[]; script: CheckedScript }
  | { ok: false; errors: PineError[]; warnings: PineError[] };

export interface RunOptions {
  /** Input overrides keyed by InputInfo.id. */
  inputs?: Record<number, InputOverride>;
  limits?: Partial<Limits>;
  /** syminfo / timeframe values; derived from the bars when omitted. */
  host?: Partial<HostInfo>;
  /** Seed for math.random without an explicit seed (default 1). */
  seed?: number;
}

export interface RunStats {
  bars: number;
  ms: number;
  nodes: number;
}

export type RunResult =
  | { ok: true; meta: ScriptMeta; inputs: InputInfo[]; outputs: Output[]; warnings: PineError[]; stats: RunStats }
  | { ok: false; errors: PineError[]; warnings: PineError[] };

export function limitsOf(partial?: Partial<Limits>): Limits {
  return { ...DEFAULT_LIMITS, ...partial };
}

export function compile(source: string, opts: CompileOptions = {}): CompileResult {
  const limits = limitsOf(opts.limits);
  try {
    if (source.length > limits.maxSourceBytes)
      fail("limit", `Script is too large: ${source.length.toLocaleString("en-US")} characters (limit ${limits.maxSourceBytes.toLocaleString("en-US")})`, { line: 1, col: 1 });
    const script = check(parse(source), { maxPlots: limits.maxPlots });
    return { ok: true, meta: script.meta, inputs: script.inputs, warnings: script.warnings, script };
  } catch (e) {
    return { ok: false, errors: [toPineError(e)], warnings: [] };
  }
}

/** Pine timeframe string for a bar duration ("1", "60", "1D", "1W", ...). */
export function timeframeOf(ms: number): string {
  const min = Math.round(ms / 60_000);
  if (min <= 0) return "";
  if (min % 10_080 === 0) return `${min / 10_080}W`;
  if (min % 1_440 === 0) return `${min / 1_440}D`;
  return String(min);
}

export function hostOf(bars: Bar[], host: Partial<HostInfo> = {}): HostInfo {
  const intervalMs = host.intervalMs ?? (bars.length > 1 ? bars[1].t - bars[0].t : 60_000);
  return {
    ticker: host.ticker ?? "",
    mintick: host.mintick ?? 0.01,
    timeframe: host.timeframe ?? timeframeOf(intervalMs),
    intervalMs,
    barIndexBase: host.barIndexBase ?? 0,
  };
}

export function runCompiled(script: CheckedScript, bars: Bar[], opts: RunOptions = {}): RunResult {
  const limits = limitsOf(opts.limits);
  const started = Date.now();
  try {
    if (bars.length > limits.maxBars)
      fail("limit", `Too many bars: ${bars.length.toLocaleString("en-US")} (limit ${limits.maxBars.toLocaleString("en-US")})`, { line: 1, col: 1 });
    bars.forEach((b, i) => {
      if (![b.t, b.o, b.h, b.l, b.c, b.v].every(Number.isFinite)) fail("runtime", `Bar ${i} has a missing or non-finite value`, { line: 1, col: 1 });
    });
    const host = hostOf(bars, opts.host);
    const inputs = resolveInputs(script.inputs, opts.inputs, host);
    const res = new Interpreter(script, { bars, inputs, host, limits, seed: opts.seed ?? 1 }).run();
    return {
      ok: true,
      meta: script.meta,
      inputs: script.inputs,
      outputs: res.outputs,
      warnings: script.warnings,
      stats: { bars: bars.length, ms: Date.now() - started, nodes: res.nodes },
    };
  } catch (e) {
    return { ok: false, errors: [toPineError(e)], warnings: script.warnings };
  }
}

/** Compile and run in one call (synchronously, in the calling thread). */
export function runSync(source: string, bars: Bar[], opts: RunOptions = {}): RunResult {
  const c = compile(source, { limits: opts.limits });
  if (!c.ok) return c;
  return runCompiled(c.script, bars, opts);
}

export const timeoutError = (ms: number): PineError => pineError("limit", `Run stopped: wall time limit of ${ms} ms exceeded`, 1, 1);
