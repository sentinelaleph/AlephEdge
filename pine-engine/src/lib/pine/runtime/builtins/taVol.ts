/**
 * Range and trend tools: tr, atr, bb, bbw, kc, kcw, dmi, sar, supertrend,
 * vwap. Where the reference uses `close[1]`, `high[1]` or `bar_index`, those
 * are the chart's bar series (bar-indexed), exactly as in the published
 * pseudocode; the smoothing state is still per call site.
 */
import { Hist, Rec } from "../series";
import { truthy } from "../values";
import { div, FixNan, gt, len, lt, n, P, smaOf, Smooth, stdevOf, sub, ta, trueRangeAt, type N } from "./taCore";
import { utcDayStart } from "./taVars";
import type { BuiltinFn, Rt } from "./types";

const src = P("series");
const simpleLength = P("length", { simple: true });

class Bands {
  h = new Hist(true);
}

function bands(rt: Rt, x: N, l: number | null, mult: N): [N, N, N] {
  const st = rt.state(() => new Bands());
  st.h.push(x, rt.bar);
  if (l === null || mult === null) return [null, null, null];
  const basis = smaOf(st.h, l);
  const w = st.h.window(l);
  const sd = w === null ? null : stdevOf(w, true, true);
  if (basis === null || sd === null) return [null, null, null];
  const dev = mult * sd;
  return [basis, basis + dev, basis - dev];
}

class Kc {
  basis = new Smooth("ema");
  range = new Smooth("ema");
}

function keltner(rt: Rt, x: N, l: number | null, mult: N, useTr: boolean): [N, N, N] {
  const st = rt.state(() => new Kc());
  const b = rt.bars[rt.bar];
  const span = useTr ? trueRangeAt(rt, rt.bar, false) : b.h - b.l;
  if (l === null) return [null, null, null];
  const basis = st.basis.step(x, l, rt.bar);
  const r = st.range.step(span, l, rt.bar);
  if (basis === null || r === null || mult === null) return [basis, null, null];
  return [basis, basis + r * mult, basis - r * mult];
}

class Dmi {
  hi = new Hist();
  lo = new Hist();
  tr = new Smooth("rma");
  plus = new Smooth("rma");
  minus = new Smooth("rma");
  fp = new FixNan();
  fm = new FixNan();
  adx = new Smooth("rma");
}

interface SarState { result: N; maxMin: N; accel: N; isBelow: boolean | null; out: N }

interface StState { atr: N; lower: N; upper: N; st: N }

class Supertrend {
  atr = new Smooth("rma");
  rec = new Rec<StState>({ atr: null, lower: null, upper: null, st: null });
}

class Vwap {
  rec = new Rec<{ day: number; pv: number; v: number }>({ day: NaN, pv: 0, v: 0 });
}

export const VOLATILITY: BuiltinFn[] = [
  ta("tr", [P("handle_na", { req: false, def: false })], (rt, a) => trueRangeAt(rt, rt.bar, truthy(a[0]))),
  // ta.rma(ta.tr(true), length)
  ta("atr", [simpleLength], (rt, a) => {
    const st = rt.state(() => new Smooth("rma"));
    const l = len(rt, a[0], "ta.atr");
    return l === null ? null : st.step(trueRangeAt(rt, rt.bar, true), l, rt.bar);
  }),
  ta("bb", [src, P("length"), P("mult")], (rt, a) => bands(rt, n(a[0]), len(rt, a[1], "ta.bb"), n(a[2])), { returns: "tuple", tuple: 3 }),
  // In percent, like TradingView (its built-in returns (upper - lower) /
  // basis * 100; checked side by side on BINANCE:BTCUSDT 4h, 1 Nov 2026).
  ta("bbw", [src, P("length"), P("mult")], (rt, a) => {
    const [basis, up, low] = bands(rt, n(a[0]), len(rt, a[1], "ta.bbw"), n(a[2]));
    const w = div(sub(up, low), basis);
    return w === null ? null : w * 100;
  }),
  ta(
    "kc",
    [src, simpleLength, P("mult"), P("useTrueRange", { req: false, def: true })],
    (rt, a) => keltner(rt, n(a[0]), len(rt, a[1], "ta.kc"), n(a[2]), truthy(a[3])),
    { returns: "tuple", tuple: 3 },
  ),
  ta("kcw", [src, simpleLength, P("mult"), P("useTrueRange", { req: false, def: true })], (rt, a) => {
    const [basis, up, low] = keltner(rt, n(a[0]), len(rt, a[1], "ta.kcw"), n(a[2]), truthy(a[3]));
    return div(sub(up, low), basis);
  }),
  ta(
    "dmi",
    [P("diLength", { simple: true }), P("adxSmoothing", { simple: true })],
    (rt, a) => {
      const st = rt.state(() => new Dmi());
      const b = rt.bars[rt.bar];
      st.hi.push(b.h, rt.bar);
      st.lo.push(b.l, rt.bar);
      const di = len(rt, a[0], "ta.dmi", "diLength");
      const adxLen = len(rt, a[1], "ta.dmi", "adxSmoothing");
      if (di === null || adxLen === null) return [null, null, null];
      const up = sub(b.h, st.hi.at(1));
      const downCh = sub(b.l, st.lo.at(1));
      const down = downCh === null ? null : -downCh;
      const plusDM = up === null ? null : gt(up, down) && up > 0 ? up : 0;
      const minusDM = down === null ? null : gt(down, up) && down > 0 ? down : 0;
      const trur = st.tr.step(trueRangeAt(rt, rt.bar, false), di, rt.bar);
      const pr = st.plus.step(plusDM, di, rt.bar);
      const mr = st.minus.step(minusDM, di, rt.bar);
      const plus = st.fp.step(div(pr === null ? null : 100 * pr, trur), rt.bar);
      const minus = st.fm.step(div(mr === null ? null : 100 * mr, trur), rt.bar);
      const sum = plus === null || minus === null ? null : plus + minus;
      const ratio = sum === null || plus === null || minus === null ? null : Math.abs(plus - minus) / (sum === 0 ? 1 : sum);
      const adx = st.adx.step(ratio, adxLen, rt.bar);
      return [plus, minus, adx === null ? null : 100 * adx];
    },
    { returns: "tuple", tuple: 3 },
  ),
  ta("sar", [P("start"), P("inc"), P("max")], (rt, a) => {
    const rec = rt.state(() => new Rec<SarState>({ result: null, maxMin: null, accel: null, isBelow: null, out: null }));
    const start = n(a[0]);
    const inc = n(a[1]);
    const max = n(a[2]);
    if (start === null || inc === null || max === null) return null;
    return rec.step(rt.bar, (p) => sarStep(rt, p, start, inc, max)).out;
  }),
  ta(
    "supertrend",
    [P("factor"), P("atrPeriod", { simple: true })],
    (rt, a) => {
      const st = rt.state(() => new Supertrend());
      const factor = n(a[0]);
      const l = len(rt, a[1], "ta.supertrend", "atrPeriod");
      if (factor === null || l === null) return [null, null];
      const atr = st.atr.step(trueRangeAt(rt, rt.bar, true), l, rt.bar);
      let dir: number | null = null;
      const s = st.rec.step(rt.bar, (p) => {
        const b = rt.bars[rt.bar];
        const close1 = rt.bar > 0 ? rt.bars[rt.bar - 1].c : null;
        const hl2 = (b.h + b.l) / 2;
        const prevLower = p.lower ?? 0;
        const prevUpper = p.upper ?? 0;
        let lower: N = atr === null ? null : hl2 - factor * atr;
        let upper: N = atr === null ? null : hl2 + factor * atr;
        lower = gt(lower, prevLower) || lt(close1, prevLower) ? lower : prevLower;
        upper = lt(upper, prevUpper) || gt(close1, prevUpper) ? upper : prevUpper;
        if (p.atr === null) dir = 1;
        else if (p.st !== null && p.st === prevUpper) dir = gt(b.c, upper) ? -1 : 1;
        else dir = lt(b.c, lower) ? 1 : -1;
        return { atr, lower, upper, st: dir === -1 ? lower : upper };
      });
      // During the ATR warm-up the reference pseudocode yields artefacts (0 / na bands);
      // Pine-lite reports na for both outputs until the ATR exists.
      return atr === null ? [null, null] : [s.st, dir];
    },
    { returns: "tuple", tuple: 2 },
  ),
  // Anchored to the UTC day of the bar's open time.
  ta("vwap", [P("source")], (rt, a) => {
    const st = rt.state(() => new Vwap());
    const x = n(a[0]);
    const b = rt.bars[rt.bar];
    const day = utcDayStart(b.t);
    const s = st.rec.step(rt.bar, (p) => {
      const base = p.day === day ? p : { day, pv: 0, v: 0 };
      return x === null ? base : { day, pv: base.pv + x * b.v, v: base.v + b.v };
    });
    return x === null || s.v === 0 ? null : s.pv / s.v;
  }),
];

/** One step of TradingView's published pine_sar. */
function sarStep(rt: Rt, p: SarState, start: number, inc: number, max: number): SarState {
  const i = rt.bar;
  const bars = rt.bars;
  const b = bars[i];
  let { result, maxMin, accel, isBelow } = p;
  let first = false;
  if (i === 1) {
    if (b.c > bars[0].c) {
      isBelow = true;
      maxMin = b.h;
      result = bars[0].l;
    } else {
      isBelow = false;
      maxMin = b.l;
      result = bars[0].h;
    }
    first = true;
    accel = start;
  }
  if (result === null || maxMin === null || accel === null || isBelow === null) return { ...p, out: null };
  result = result + accel * (maxMin - result);
  if (isBelow) {
    if (result > b.l) {
      first = true;
      isBelow = false;
      result = Math.max(b.h, maxMin);
      maxMin = b.l;
      accel = start;
    }
  } else if (result < b.h) {
    first = true;
    isBelow = true;
    result = Math.min(b.l, maxMin);
    maxMin = b.h;
    accel = start;
  }
  if (!first) {
    if (isBelow && b.h > maxMin) {
      maxMin = b.h;
      accel = Math.min(accel + inc, max);
    } else if (!isBelow && b.l < maxMin) {
      maxMin = b.l;
      accel = Math.min(accel + inc, max);
    }
  }
  if (isBelow) {
    result = Math.min(result, bars[i - 1].l);
    if (i > 1) result = Math.min(result, bars[i - 2].l);
  } else {
    result = Math.max(result, bars[i - 1].h);
    if (i > 1) result = Math.max(result, bars[i - 2].h);
  }
  return { result, maxMin, accel, isBelow, out: result };
}
