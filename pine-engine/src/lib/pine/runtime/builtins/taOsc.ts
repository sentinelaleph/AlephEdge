/**
 * Oscillators and differences: rsi, macd, stoch, tsi, cci, mfi, wpr, mom,
 * change, roc.
 */
import { Hist } from "../series";
import { div, extremeOf, len, n, P, sub, Smooth, Sum, ta, type N } from "./taCore";
import type { Value } from "../values";
import type { BuiltinFn, Rt } from "./types";

const src = P("source");
const length = P("length");
const simpleLength = P("length", { simple: true });

class Rsi {
  h = new Hist();
  up = new Smooth("rma");
  down = new Smooth("rma");
}

class Macd {
  fast = new Smooth("ema");
  slow = new Smooth("ema");
  sig = new Smooth("ema");
}

class Stoch {
  hi = new Hist(true);
  lo = new Hist(true);
  src = new Hist(true);
}

class Tsi {
  h = new Hist();
  e1 = new Smooth("ema");
  e2 = new Smooth("ema");
  a1 = new Smooth("ema");
  a2 = new Smooth("ema");
}

class Mfi {
  h = new Hist();
  upper = new Sum();
  lower = new Sum();
}

export const OSCILLATORS: BuiltinFn[] = [
  // u = max(x - x[1], 0), d = max(x[1] - x, 0), smoothed with rma; TradingView's
  // built-in guards: rma(d) == 0 -> 100, rma(u) == 0 -> 0.
  ta("rsi", [src, simpleLength], (rt, a) => {
    const l = len(rt, a[1], "ta.rsi");
    const st = rt.state(() => new Rsi());
    const x = n(a[0]);
    st.h.push(x, rt.bar);
    if (l === null) return null;
    const ch = sub(x, st.h.at(1));
    const u = st.up.step(ch === null ? null : Math.max(ch, 0), l, rt.bar);
    const d = st.down.step(ch === null ? null : Math.max(-ch, 0), l, rt.bar);
    if (u === null || d === null) return null;
    if (d === 0) return 100;
    if (u === 0) return 0;
    return 100 - 100 / (1 + u / d);
  }),
  ta(
    "macd",
    [src, P("fastlen", { simple: true }), P("slowlen", { simple: true }), P("siglen", { simple: true })],
    (rt, a) => {
      const st = rt.state(() => new Macd());
      const x = n(a[0]);
      const f = len(rt, a[1], "ta.macd", "fastlen");
      const s = len(rt, a[2], "ta.macd", "slowlen");
      const g = len(rt, a[3], "ta.macd", "siglen");
      if (f === null || s === null || g === null) return [null, null, null];
      const m = sub(st.fast.step(x, f, rt.bar), st.slow.step(x, s, rt.bar));
      const signal = st.sig.step(m, g, rt.bar);
      return [m, signal, sub(m, signal)];
    },
    { returns: "tuple", tuple: 3 },
  ),
  // 100 * (source - lowest(low, length)) / (highest(high, length) - lowest(low, length))
  ta("stoch", [src, P("high"), P("low"), length], (rt, a) => {
    const st = rt.state(() => new Stoch());
    st.hi.push(n(a[1]), rt.bar);
    st.lo.push(n(a[2]), rt.bar);
    st.src.push(n(a[0]), rt.bar);
    const l = len(rt, a[3], "ta.stoch");
    if (l === null) return null;
    const hh = extremeOf(st.hi, l, true).v;
    const ll = extremeOf(st.lo, l, false).v;
    const d = sub(st.src.at(0), ll);
    return div(d === null ? null : 100 * d, sub(hh, ll));
  }),
  // ema(ema(pc, long), short) / ema(ema(abs(pc), long), short), pc = change(source)
  ta(
    "tsi",
    [src, P("short_length", { simple: true }), P("long_length", { simple: true })],
    (rt, a) => {
      const st = rt.state(() => new Tsi());
      const x = n(a[0]);
      st.h.push(x, rt.bar);
      const s = len(rt, a[1], "ta.tsi", "short_length");
      const l = len(rt, a[2], "ta.tsi", "long_length");
      if (s === null || l === null) return null;
      const pc = sub(x, st.h.at(1));
      const num = st.e2.step(st.e1.step(pc, l, rt.bar), s, rt.bar);
      const den = st.a2.step(st.a1.step(pc === null ? null : Math.abs(pc), l, rt.bar), s, rt.bar);
      return div(num, den);
    },
  ),
  // (source - sma(source, length)) / (0.015 * dev(source, length))
  ta("cci", [src, length], (rt, a) => {
    const h = rt.state(() => new Hist(true));
    h.push(n(a[0]), rt.bar);
    const l = len(rt, a[1], "ta.cci");
    if (l === null) return null;
    const w = h.window(l);
    if (w === null) return null;
    const x = w[0];
    let mean = 0;
    for (let i = 0; i < l; i++) mean += w[i] / l;
    let dev = 0;
    for (let i = 0; i < l; i++) dev += Math.abs(w[i] - mean);
    dev /= l;
    return div(x - mean, 0.015 * dev);
  }),
  // upper = sum(volume * (change(src) <= 0 ? 0 : src)), lower likewise with >= 0;
  // mfi = 100 - 100 / (1 + upper / lower). An na change counts on both sides.
  ta("mfi", [P("series"), length], (rt, a) => {
    const st = rt.state(() => new Mfi());
    const x = n(a[0]);
    st.h.push(x, rt.bar);
    const l = len(rt, a[1], "ta.mfi");
    if (l === null) return null;
    const ch = sub(x, st.h.at(1));
    const vol = rt.bars[rt.bar].v;
    // na source values are skipped by the sums (TradingView: "na values ... are ignored").
    const upTerm = x === null ? null : vol * (ch !== null && ch <= 0 ? 0 : x);
    const downTerm = x === null ? null : vol * (ch !== null && ch >= 0 ? 0 : x);
    const upper = st.upper.step(upTerm, l, rt.bar);
    const lower = st.lower.step(downTerm, l, rt.bar);
    const ratio = div(upper, lower);
    return ratio === null ? null : 100 - 100 / (1 + ratio);
  }),
  // 100 * (close - highest(length)) / (highest(length) - lowest(length))
  ta("wpr", [length], (rt, a) => {
    const st = rt.state(() => new Stoch());
    const b = rt.bars[rt.bar];
    st.hi.push(b.h, rt.bar);
    st.lo.push(b.l, rt.bar);
    const l = len(rt, a[0], "ta.wpr");
    if (l === null) return null;
    const hh = extremeOf(st.hi, l, true).v;
    const ll = extremeOf(st.lo, l, false).v;
    const d = sub(b.c, hh);
    return div(d === null ? null : 100 * d, sub(hh, ll));
  }),
  ta("mom", [src, length], (rt, a) => diffOf(rt, a[0], a[1], "ta.mom", false)),
  ta(
    "change",
    [src, P("length", { req: false, def: 1 })],
    (rt, a) => (typeof a[0] === "boolean" ? boolChange(rt, a[0], a[1]) : diffOf(rt, a[0], a[1], "ta.change", false)),
  ),
  ta("roc", [src, length], (rt, a) => diffOf(rt, a[0], a[1], "ta.roc", true)),
];

function diffOf(rt: Rt, v: Value, lv: Value, fn: string, pct: boolean): N {
  const h = rt.state(() => new Hist());
  const x = n(v);
  h.push(x, rt.bar);
  const l = len(rt, lv, fn, "length", fn === "ta.change" ? 0 : 1);
  if (l === null) return null;
  const prev = h.at(l);
  if (!pct) return sub(x, prev);
  const d = sub(x, prev);
  return div(d === null ? null : 100 * d, prev);
}

/** ta.change on a bool series: true when the value differs from `length` calls ago. */
function boolChange(rt: Rt, v: boolean, lv: Value): boolean {
  const h = rt.state(() => new Hist());
  h.push(v ? 1 : 0, rt.bar);
  const l = len(rt, lv, "ta.change", "length", 0) ?? 1;
  const prev = h.at(l);
  return prev !== null && prev !== (v ? 1 : 0);
}
