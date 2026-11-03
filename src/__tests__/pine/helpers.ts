import { expect } from "vitest";

import { compile, runSync, type RunOptions, type RunResult } from "@/lib/pine";
import type { Output, PlotOutput } from "@/lib/pine/outputs";
import type { Bar } from "@/lib/ta/core";

import reference from "./fixtures/reference.json";

type Ref = { bars: number[][]; series: Record<string, (number | null)[]> };
const REF = reference as unknown as Ref;

/** The fixture's 300 bars (240 random-walk + 60 edge-case bars). */
export const BARS: Bar[] = REF.bars.map(([t, o, h, l, c, v]) => ({ t, o, h, l, c, v }));
export const SERIES = REF.series;

/** Runs and asserts success. */
export function ok(source: string, bars: Bar[] = BARS, opts: RunOptions = {}): Extract<RunResult, { ok: true }> {
  const r = runSync(source, bars, opts);
  if (!r.ok) throw new Error(`expected success, got ${JSON.stringify(r.errors)}`);
  return r;
}

/** Runs and returns the first error (asserting failure). */
export function err(source: string, bars: Bar[] = BARS.slice(0, 50), opts: RunOptions = {}) {
  const r = runSync(source, bars, opts);
  if (r.ok) throw new Error("expected an error, got success");
  return r.errors[0];
}

/** Compile-time error (no bars). */
export function compileErr(source: string) {
  const r = compile(source);
  if (r.ok) throw new Error("expected a compile error, got success");
  return r.errors[0];
}

export function plots(r: Extract<RunResult, { ok: true }>): Record<string, (number | null)[]> {
  const out: Record<string, (number | null)[]> = {};
  for (const o of r.outputs) if (o.kind === "plot") out[o.title] = o.values;
  return out;
}

export function plot(r: Extract<RunResult, { ok: true }>, title: string): PlotOutput {
  const p = r.outputs.find((o): o is PlotOutput => o.kind === "plot" && o.title === title);
  if (!p) throw new Error(`no plot titled ${title}`);
  return p;
}

/** The output descriptor type whose `kind` union includes K. */
type OutputOf<K extends Output["kind"]> = Output extends infer O ? (O extends { kind: infer KK } ? (K extends KK ? O : never) : never) : never;

export function output<K extends Output["kind"]>(r: Extract<RunResult, { ok: true }>, kind: K, index = 0): OutputOf<K> {
  const o = r.outputs.filter((x) => x.kind === kind)[index];
  if (!o) throw new Error(`no ${kind} output #${index}`);
  return o as OutputOf<K>;
}

/** Same length, na in the same places, values within 1e-9 relative (absolute below 1). */
export function expectSeries(actual: (number | null)[], expected: (number | null)[], name: string, from = 0): void {
  expect(actual.length, `${name} length`).toBe(expected.length);
  for (let i = from; i < expected.length; i++) {
    const a = actual[i];
    const e = expected[i];
    if (e === null || a === null) {
      expect(a, `${name}[${i}] (expected ${e})`).toBe(e);
      continue;
    }
    const tol = 1e-9 * Math.max(1, Math.abs(a), Math.abs(e));
    if (Math.abs(a - e) > tol) expect(a, `${name}[${i}]`).toBe(e);
  }
}

/** A script that plots each expression with its name as the title. */
export function plotScript(entries: [string, string][], prelude: string[] = []): string {
  return ["//@version=6", 'indicator("t")', ...prelude, ...entries.map(([n, e]) => `plot(${e}, title = "${n}")`)].join("\n");
}

/** Bars from closes (o = previous close, h/l = max/min ± 1, v = 100), hourly. */
export function barsFrom(closes: number[]): Bar[] {
  return closes.map((c, i) => {
    const o = i === 0 ? c : closes[i - 1];
    return { t: 1767225600000 + i * 3_600_000, o, h: Math.max(o, c) + 1, l: Math.min(o, c) - 1, c, v: 100 };
  });
}
