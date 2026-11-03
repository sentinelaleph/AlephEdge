/**
 * Window statistics: stdev, variance, dev, median, highest, lowest,
 * highestbars, lowestbars, percentrank, correlation, cog, range.
 *
 * highest/lowest (and the *bars forms, range, stoch, wpr) ignore na values
 * inside a full window and are na until `length` calls exist; ties resolve to
 * the most recent bar.
 */
import { Hist } from "../series";
import { truthy, type Value } from "../values";
import { extremeOf, len, n, P, stdevOf, ta } from "./taCore";
import type { BuiltinFn, Rt } from "./types";

const src = P("source");
const length = P("length");

function windowed(rt: Rt, a: Value[], fn: string): { w: number[] | null; l: number | null; h: Hist } {
  const h = rt.state(() => new Hist(true));
  h.push(n(a[0]), rt.bar);
  const l = len(rt, a[1], fn);
  return { w: l === null ? null : h.window(l), l, h };
}

/** highest(length) uses high, highest(source, length) the given source. */
function extreme(name: string, max: boolean, bars: boolean): BuiltinFn {
  return ta(
    name,
    [P("source", { req: false, src: max ? "high" : "low" }), length],
    (rt, a) => {
      const h = rt.state(() => new Hist(true));
      h.push(n(a[0]), rt.bar);
      const l = len(rt, a[1], `ta.${name}`);
      if (l === null) return null;
      const e = extremeOf(h, l, max);
      if (e.v === null) return null;
      return bars ? h.epochAt(e.at) - rt.bar : e.v;
    },
    { sigs: [["length"], ["source", "length"]], returns: bars ? "int" : "float" },
  );
}

class Pair {
  a = new Hist(true);
  b = new Hist(true);
}

export const STATS: BuiltinFn[] = [
  ta("stdev", [src, length, P("biased", { req: false, def: true })], (rt, a) => {
    const { w } = windowed(rt, a, "ta.stdev");
    return w === null ? null : stdevOf(w, truthy(a[2]), true);
  }),
  ta("variance", [src, length, P("biased", { req: false, def: true })], (rt, a) => {
    const { w } = windowed(rt, a, "ta.variance");
    return w === null ? null : stdevOf(w, truthy(a[2]), false);
  }),
  // Mean absolute deviation from the SMA.
  ta("dev", [src, length], (rt, a) => {
    const { w, l } = windowed(rt, a, "ta.dev");
    if (w === null || l === null) return null;
    let mean = 0;
    for (let i = 0; i < l; i++) mean += w[i] / l;
    let sum = 0;
    for (let i = 0; i < l; i++) sum += Math.abs(w[i] - mean);
    return sum / l;
  }),
  ta("median", [src, length], (rt, a) => {
    const { w, l } = windowed(rt, a, "ta.median");
    if (w === null || l === null) return null;
    const s = [...w].sort((x, y) => x - y);
    const mid = Math.floor(l / 2);
    return l % 2 === 1 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
  }),
  extreme("highest", true, false),
  extreme("lowest", false, false),
  extreme("highestbars", true, true),
  extreme("lowestbars", false, true),
  ta("range", [src, length], (rt, a) => {
    const h = rt.state(() => new Hist(true));
    h.push(n(a[0]), rt.bar);
    const l = len(rt, a[1], "ta.range");
    if (l === null) return null;
    const hi = extremeOf(h, l, true).v;
    const lo = extremeOf(h, l, false).v;
    return hi === null || lo === null ? null : hi - lo;
  }),
  // Percent of the previous `length` values that are <= the current value.
  ta("percentrank", [src, length], (rt, a) => {
    const h = rt.state(() => new Hist());
    const x = n(a[0]);
    h.push(x, rt.bar);
    const l = len(rt, a[1], "ta.percentrank");
    if (l === null || x === null) return null;
    const w = h.raw(l + 1);
    if (w === null) return null;
    let count = 0;
    for (let i = 1; i <= l; i++) {
      const v = w[i];
      if (v !== null && v <= x) count++;
    }
    return (count * 100) / l;
  }),
  ta("correlation", [P("source1"), P("source2"), length], (rt, a) => {
    const st = rt.state(() => new Pair());
    const both = n(a[0]) !== null && n(a[1]) !== null;
    st.a.push(both ? n(a[0]) : null, rt.bar);
    st.b.push(both ? n(a[1]) : null, rt.bar);
    const l = len(rt, a[2], "ta.correlation");
    if (l === null) return null;
    const x = st.a.window(l);
    const y = st.b.window(l);
    if (x === null || y === null) return null;
    let mx = 0;
    let my = 0;
    for (let i = 0; i < l; i++) {
      mx += x[i] / l;
      my += y[i] / l;
    }
    let sxy = 0;
    let sxx = 0;
    let syy = 0;
    for (let i = 0; i < l; i++) {
      sxy += (x[i] - mx) * (y[i] - my);
      sxx += (x[i] - mx) ** 2;
      syy += (y[i] - my) ** 2;
    }
    const den = Math.sqrt(sxx * syy);
    return den === 0 ? null : sxy / den;
  }),
  // -sum(source[i] * (i + 1)) / sum(source, length)
  ta("cog", [src, length], (rt, a) => {
    const { w, l } = windowed(rt, a, "ta.cog");
    if (w === null || l === null) return null;
    let sum = 0;
    let num = 0;
    for (let i = 0; i < l; i++) {
      sum += w[i];
      num += w[i] * (i + 1);
    }
    return sum === 0 ? null : -num / sum;
  }),
];
