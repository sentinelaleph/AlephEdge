/**
 * Building blocks for the stateful ta.* functions. Each function call site
 * owns its own instances of these (Pine's per-call-site state), so a call that
 * is skipped on a bar does not advance them. Comparisons involving na are
 * written out explicitly: in JS `null < 5` is true, in Pine it is not.
 */
import { Hist, Rec } from "../series";
import { num, type Value } from "../values";
import type { BuiltinFn, Param, PType, Rt } from "./types";

export type N = number | null;

export const gt = (a: N, b: N): boolean => a !== null && b !== null && a > b;
export const lt = (a: N, b: N): boolean => a !== null && b !== null && a < b;
export const ge = (a: N, b: N): boolean => a !== null && b !== null && a >= b;
export const le = (a: N, b: N): boolean => a !== null && b !== null && a <= b;
export const sub = (a: N, b: N): N => (a === null || b === null ? null : a - b);
export const fin = (x: number): N => (Number.isFinite(x) ? x : null);
export const div = (a: N, b: N): N => (a === null || b === null || b === 0 ? null : fin(a / b));

/** A length argument: truncated to an int, at least `min`; na stays na. */
export function len(rt: Rt, v: Value, fn: string, what = "length", min = 1): number | null {
  const x = num(v);
  if (x === null) return null;
  const n = Math.trunc(x);
  if (n < min) rt.fail(`${fn}: ${what} must be at least ${min} (got ${x})`);
  return n;
}

/** pine_sma: sum of x[i] / y over the window; na if the window is short or holds na. */
export function smaOf(h: Hist, n: number): N {
  const w = h.window(n);
  if (w === null) return null;
  let s = 0;
  for (let i = 0; i < n; i++) s += w[i] / n;
  return s;
}

/** Window max/min (na entries skipped); `at` = entries back; na if the window is short. */
export function extremeOf(h: Hist, n: number, max: boolean): { v: N; at: number } {
  const w = h.raw(n);
  if (w === null) return { v: null, at: 0 };
  let v: N = null;
  let at = 0;
  for (let k = 0; k < n; k++) {
    const x = w[k];
    if (x === null) continue;
    if (v === null || (max ? x > v : x < v)) {
      v = x;
      at = k;
    }
  }
  return { v, at };
}

/**
 * Seeded recursive average (Pine's documented ema/rma):
 * sum := na(sum[1]) ? ta.sma(src, length) : alpha * src + (1 - alpha) * nz(sum[1])
 * with TradingView's documented remark that na inputs are ignored: an na input
 * leaves the average unchanged (and the seed counts non-na values only).
 */
export class Smooth {
  private h = new Hist(true);
  private r = new Rec<N>(null);

  constructor(private readonly kind: "ema" | "rma") {}

  step(x: N, n: number, ep: number): N {
    this.h.push(x, ep);
    const alpha = this.kind === "ema" ? 2 / (n + 1) : 1 / n;
    if (x === null) return this.r.step(ep, (prev) => prev);
    return this.r.step(ep, (prev) => (prev === null ? smaOf(this.h, n) : alpha * x + (1 - alpha) * prev));
  }
}

/** Rolling simple average of a value stream (na inputs ignored). */
export class Sma {
  private h = new Hist(true);
  step(x: N, n: number, ep: number): N {
    this.h.push(x, ep);
    return smaOf(this.h, n);
  }
}

/** Rolling sum (math.sum), na inputs ignored; na until `length` values exist. */
export class Sum {
  private h = new Hist(true);
  step(x: N, n: number, ep: number): N {
    this.h.push(x, ep);
    const w = this.h.window(n);
    if (w === null) return null;
    let s = 0;
    for (let i = 0; i < n; i++) s += w[i];
    return s;
  }
}

/** x - x[n] over the call history. */
export class Change {
  readonly h = new Hist();
  step(x: N, n: number, ep: number): N {
    this.h.push(x, ep);
    return sub(x, this.h.at(n));
  }
}

/** fixnan: the last non-na value. */
export class FixNan {
  private r = new Rec<N>(null);
  step(x: N, ep: number): N {
    return this.r.step(ep, (prev) => (x === null ? prev : x));
  }
}

/** Population (biased) or sample standard deviation per the documented pine_stdev. */
export function stdevOf(w: number[], biased: boolean, sqrt: boolean): N {
  const n = w.length;
  let avg = 0;
  for (let i = 0; i < n; i++) avg += w[i] / n;
  let ss = 0;
  for (let i = 0; i < n; i++) {
    const d = Math.abs(w[i] - avg) <= 1e-10 ? 0 : w[i] - avg;
    ss += d * d;
  }
  const denom = biased ? n : n - 1;
  if (denom <= 0) return null;
  return sqrt ? Math.sqrt(ss / denom) : ss / denom;
}

/** pine_wma: weights (len - i) * len, newest first. */
export function wmaOf(h: Hist, n: number): N {
  const w = h.window(n);
  if (w === null) return null;
  let norm = 0;
  let sum = 0;
  for (let i = 0; i < n; i++) {
    const weight = (n - i) * n;
    norm += weight;
    sum += w[i] * weight;
  }
  return sum / norm;
}

/** True range of bar i: na(close[1]) ? (handleNa ? high - low : na) : max(...). */
export function trueRangeAt(rt: Rt, i: number, handleNa: boolean): N {
  const b = rt.bars[i];
  if (i === 0) return handleNa ? b.h - b.l : null;
  const pc = rt.bars[i - 1].c;
  return Math.max(b.h - b.l, Math.abs(b.h - pc), Math.abs(b.l - pc));
}

const P = (name: string, extra: Partial<Param> = {}): Param => ({ name, req: true, ...extra });
export { P };

/** Shorthand for a stateful ta.* definition. */
export function ta(name: string, params: Param[], impl: (rt: Rt, a: Value[]) => Value, extra: Partial<BuiltinFn> = {}): BuiltinFn {
  return { name: `ta.${name}`, params, stateful: true, returns: "float" as PType, impl, ...extra };
}

export const n = num;
