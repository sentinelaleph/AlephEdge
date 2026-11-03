/**
 * Aleph Edge host entry for the Pine-lite engine copy (src/, see SOURCE.md).
 *
 * Bundled by `npm run pine:bundle` into src-tauri/src/pine/engine.bundle.js
 * (one IIFE) and evaluated in an embedded QuickJS context by the Rust runner
 * (src-tauri/src/pine/runtime.rs). Everything crosses the boundary as JSON
 * strings, so the Rust side needs no JS object marshalling and the shapes
 * match its serde structs (CompileInfo minus `hash`, RunVerdict minus `ms`).
 *
 * Trigger ids: "alert#n" for the n-th alertcondition output, "shape#n" for
 * the n-th plotshape / plotchar / plotarrow output, both counted from 0 in
 * output order (= source order: output calls are global statements).
 */
import type { Program, Stmt } from "./src/lib/pine/ast";
import { compile as compileScript, runCompiled } from "./src/lib/pine/engine";
import type { PineError } from "./src/lib/pine/errors";
import type { InputInfo, InputOverride } from "./src/lib/pine/runtime/builtins/input";
import type { Output } from "./src/lib/pine/outputs";
import type { Bar } from "./src/lib/ta/core";

type TriggerKind = "alert" | "shape";
interface Trigger { id: string; kind: TriggerKind; title: string; line: number }
interface ErrorInfo { kind: string; message: string; line: number; col: number }

interface CompileReply {
  ok: boolean;
  error: ErrorInfo | null;
  title: string;
  overlay: boolean;
  inputs: InputInfo[];
  triggers: Trigger[];
}

interface RunArgs {
  source: string;
  bars: Bar[];
  inputs: Record<string, InputOverride>;
  symbol: string;
  /** null: derived from the bars. */
  intervalMs: number | null;
  wallMs: number;
}

type RunReply =
  | { ok: true; fired: string[]; bars: number; numeric_inputs: [string, number][] }
  | { ok: false; error: ErrorInfo };

const SHAPE_KINDS = new Set(["plotshape", "plotchar", "plotarrow"]);

/**
 * QuickJS has no Intl: Number#toLocaleString ignores "en-US" and prints
 * "10000", where the browser prints "10,000" in the engine's limit
 * messages. The engine only formats integers, so a digit-grouping stand-in
 * makes the messages identical. Under node (fixtures) nothing is patched.
 */
if ((10000).toLocaleString("en-US") !== "10,000") {
  Object.defineProperty(Number.prototype, "toLocaleString", {
    value(this: number): string {
      const [int, frac] = String(this).split(".");
      return int.replace(/\B(?=(\d{3})+(?!\d))/g, ",") + (frac === undefined ? "" : `.${frac}`);
    },
    writable: true,
    configurable: true,
  });
}

const errorInfo = (e: PineError): ErrorInfo => ({ kind: e.kind, message: e.message, line: e.line, col: e.col });

function triggerKind(kind: string): TriggerKind | null {
  if (kind === "alertcondition") return "alert";
  return SHAPE_KINDS.has(kind) ? "shape" : null;
}

/** Numbers outputs per trigger kind; `undefined` for outputs that are not triggers. */
function triggerIds(kinds: string[]): (string | undefined)[] {
  const next: Record<TriggerKind, number> = { alert: 0, shape: 0 };
  return kinds.map((k) => {
    const t = triggerKind(k);
    return t === null ? undefined : `${t}#${next[t]++}`;
  });
}

/**
 * Triggers from a 2-bar dry run, which yields the exact runtime titles
 * (titles may be constant expressions). A script that fails on the
 * synthetic bars falls back to the AST: same order, literal titles only.
 */
function listTriggers(script: Extract<ReturnType<typeof compileScript>, { ok: true }>["script"]): Trigger[] {
  const dry: Bar[] = [0, 60_000].map((t) => ({ t, o: 1, h: 1, l: 1, c: 1, v: 1 }));
  const run = runCompiled(script, dry, {});
  const found: { kind: string; title: string; line: number }[] = run.ok ? run.outputs : astOutputs(script.program);
  const ids = triggerIds(found.map((o) => o.kind));
  const out: Trigger[] = [];
  found.forEach((o, i) => {
    const id = ids[i];
    if (id !== undefined) out.push({ id, kind: triggerKind(o.kind) as TriggerKind, title: o.title, line: o.line });
  });
  return out;
}

function astOutputs(program: Program): { kind: string; title: string; line: number }[] {
  const out: { kind: string; title: string; line: number }[] = [];
  program.body.forEach((s: Stmt) => {
    if (s.kind !== "ExprStmt" || s.expr.kind !== "Call") return;
    const call = s.expr;
    if (triggerKind(call.callee) === null) return;
    const t = call.bound?.[1];
    out.push({ kind: call.callee, title: t && t.kind === "Str" ? t.value : "", line: call.line });
  });
  return out;
}

function compileJson(source: string): string {
  const c = compileScript(source);
  const reply: CompileReply = c.ok
    ? { ok: true, error: null, title: c.meta.title, overlay: c.meta.overlay, inputs: c.inputs, triggers: listTriggers(c.script) }
    : { ok: false, error: errorInfo(c.errors[0]), title: "", overlay: false, inputs: [], triggers: [] };
  return JSON.stringify(reply);
}

/** True when the output draws / fires on bar `last`. */
function firedOn(o: Output, last: number): boolean {
  if (o.kind === "alertcondition") return o.fired.includes(last);
  if (o.kind === "plotshape" || o.kind === "plotchar" || o.kind === "plotarrow") {
    const v = o.values[last];
    return v !== null && v !== undefined && v !== 0 && !Number.isNaN(v);
  }
  return false;
}

function numericInputs(infos: InputInfo[], overrides: Record<string, InputOverride>): [string, number][] {
  return infos
    .filter((i) => i.kind === "int" || i.kind === "float")
    .map((i) => {
      const o = overrides[String(i.id)];
      return [i.title, typeof o === "number" ? o : (i.defval as number)];
    });
}

function runArgs(argsJson: string): { a: RunArgs; r: ReturnType<typeof runCompiled> } {
  const a = JSON.parse(argsJson) as RunArgs;
  const c = compileScript(a.source);
  if (!c.ok) return { a, r: c };
  const r = runCompiled(c.script, a.bars, {
    inputs: a.inputs as unknown as Record<number, InputOverride>,
    limits: { maxWallMs: a.wallMs },
    host: { ticker: a.symbol, intervalMs: a.intervalMs ?? undefined, barIndexBase: barIndexBase(a) },
  });
  return { a, r };
}

/**
 * bar_index of the first bar in the window. The bot re-runs on the last N
 * closed bars, so counting from the window start would give the newest bar
 * the same bar_index on every run (N - 1). Anchored to the bar's open time
 * it grows by one per bar, as on a live chart; only the absolute value
 * differs from TradingView, which counts from the chart's first loaded bar.
 */
function barIndexBase(a: RunArgs): number {
  const first = a.bars[0];
  return a.intervalMs && first ? Math.floor(first.t / a.intervalMs) : 0;
}

function runJson(argsJson: string): string {
  const { a, r } = runArgs(argsJson);
  if (!r.ok) return JSON.stringify({ ok: false, error: errorInfo(r.errors[0]) } satisfies RunReply);
  const last = a.bars.length - 1;
  const ids = triggerIds(r.outputs.map((o) => o.kind));
  const fired = r.outputs.flatMap((o, i) => (ids[i] !== undefined && firedOn(o, last) ? [ids[i] as string] : []));
  return JSON.stringify({ ok: true, fired, bars: a.bars.length, numeric_inputs: numericInputs(r.inputs, a.inputs) } satisfies RunReply);
}

/**
 * Every plot / marker / alert output over all bars. Used only by the Rust
 * parity tests, which compare it with the same call under node.
 */
function outputsJson(argsJson: string): string {
  const { r } = runArgs(argsJson);
  if (!r.ok) return JSON.stringify({ ok: false, error: errorInfo(r.errors[0]) });
  type Dump = { kind: string; title: string; fired?: number[]; values?: (number | null)[] };
  const outputs = r.outputs.flatMap((o): Dump[] => {
    if (o.kind === "alertcondition") return [{ kind: o.kind, title: o.title, fired: o.fired }];
    if (o.kind === "plot" || SHAPE_KINDS.has(o.kind)) return [{ kind: o.kind, title: o.title, values: (o as { values: (number | null)[] }).values }];
    return [];
  });
  return JSON.stringify({ ok: true, outputs });
}

(globalThis as unknown as { PineHost: unknown }).PineHost = { compile: compileJson, run: runJson, outputs: outputsJson };
