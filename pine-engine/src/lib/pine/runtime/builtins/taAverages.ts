/**
 * Moving averages, per the Pine v5/v6 reference definitions:
 * sma, ema (SMA-seeded), rma (SMA-seeded, alpha 1/len), wma, vwma, hma, alma,
 * swma, linreg.
 */
import { Hist } from "../series";
import { truthy } from "../values";
import { len, n, P, Sma, smaOf, Smooth, ta, wmaOf } from "./taCore";
import type { BuiltinFn } from "./types";

const src = P("source");
const length = P("length");
const simpleLength = P("length", { simple: true });

class Hma {
  h = new Hist(true);
  raw = new Hist(true);
}

class Vwma {
  pv = new Sma();
  v = new Sma();
}

export const AVERAGES: BuiltinFn[] = [
  ta("sma", [src, length], (rt, a) => {
    const l = len(rt, a[1], "ta.sma");
    const st = rt.state(() => new Hist(true));
    st.push(n(a[0]), rt.bar);
    return l === null ? null : smaOf(st, l);
  }),
  ta("ema", [src, simpleLength], (rt, a) => {
    const l = len(rt, a[1], "ta.ema");
    const st = rt.state(() => new Smooth("ema"));
    return l === null ? null : st.step(n(a[0]), l, rt.bar);
  }),
  ta("rma", [src, simpleLength], (rt, a) => {
    const l = len(rt, a[1], "ta.rma");
    const st = rt.state(() => new Smooth("rma"));
    return l === null ? null : st.step(n(a[0]), l, rt.bar);
  }),
  ta("wma", [src, length], (rt, a) => {
    const l = len(rt, a[1], "ta.wma");
    const st = rt.state(() => new Hist(true));
    st.push(n(a[0]), rt.bar);
    return l === null ? null : wmaOf(st, l);
  }),
  ta("vwma", [src, length], (rt, a) => {
    const l = len(rt, a[1], "ta.vwma");
    const st = rt.state(() => new Vwma());
    const x = n(a[0]);
    const vol = rt.bars[rt.bar].v;
    if (l === null) return null;
    const pv = st.pv.step(x === null ? null : x * vol, l, rt.bar);
    const v = st.v.step(x === null ? null : vol, l, rt.bar);
    return pv === null || v === null || v === 0 ? null : pv / v;
  }),
  // ta.wma(2 * ta.wma(src, length / 2) - ta.wma(src, length), math.floor(math.sqrt(length)))
  ta("hma", [src, length], (rt, a) => {
    const l = len(rt, a[1], "ta.hma", "length", 2);
    const st = rt.state(() => new Hma());
    st.h.push(n(a[0]), rt.bar);
    if (l === null) return null;
    const half = wmaOf(st.h, Math.floor(l / 2));
    const full = wmaOf(st.h, l);
    st.raw.push(half === null || full === null || n(a[0]) === null ? null : 2 * half - full, rt.bar);
    return wmaOf(st.raw, Math.floor(Math.sqrt(l)));
  }),
  ta(
    "alma",
    [src, length, P("offset"), P("sigma"), P("floor", { req: false, def: false })],
    (rt, a) => {
      const l = len(rt, a[1], "ta.alma");
      const st = rt.state(() => new Hist());
      st.push(n(a[0]), rt.bar);
      const offset = n(a[2]);
      const sigma = n(a[3]);
      if (l === null || offset === null || sigma === null) return null;
      const w = st.window(l);
      if (w === null) return null;
      const m = truthy(a[4]) ? Math.floor(offset * (l - 1)) : offset * (l - 1);
      const s = l / sigma;
      let norm = 0;
      let sum = 0;
      for (let i = 0; i < l; i++) {
        const weight = Math.exp((-1 * Math.pow(i - m, 2)) / (2 * Math.pow(s, 2)));
        norm += weight;
        sum += w[l - i - 1] * weight;
      }
      return norm === 0 ? null : sum / norm;
    },
  ),
  ta("swma", [src], (rt, a) => {
    const st = rt.state(() => new Hist());
    st.push(n(a[0]), rt.bar);
    const w = st.window(4);
    if (w === null) return null;
    return (w[3] * 1) / 6 + (w[2] * 2) / 6 + (w[1] * 2) / 6 + (w[0] * 1) / 6;
  }),
  // Least squares over x = 0 (oldest) .. length - 1 (current); value at length - 1 - offset.
  ta("linreg", [src, length, P("offset")], (rt, a) => {
    const l = len(rt, a[1], "ta.linreg");
    const st = rt.state(() => new Hist());
    st.push(n(a[0]), rt.bar);
    const off = n(a[2]);
    if (l === null || off === null) return null;
    const w = st.window(l);
    if (w === null) return null;
    let sx = 0;
    let sy = 0;
    let sxy = 0;
    let sxx = 0;
    for (let k = 0; k < l; k++) {
      const x = l - 1 - k;
      const y = w[k];
      sx += x;
      sy += y;
      sxy += x * y;
      sxx += x * x;
    }
    const den = l * sxx - sx * sx;
    if (den === 0) return null;
    const slope = (l * sxy - sx * sy) / den;
    const intercept = (sy - slope * sx) / l;
    return intercept + slope * (l - 1 - Math.trunc(off));
  }),
];

