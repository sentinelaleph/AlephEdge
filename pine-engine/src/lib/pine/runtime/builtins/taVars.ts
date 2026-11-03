/**
 * The ta.* built-in *variables* (bar-indexed series, not per call site):
 * ta.tr = ta.tr(false); ta.obv = ta.cum(math.sign(ta.change(close)) * volume);
 * ta.accdist = ta.cum(close == high and close == low or high == low ? 0 :
 * ((2 * close - low - high) / (high - low)) * volume); ta.vwap = VWAP of hlc3
 * anchored to the UTC day.
 */
import type { Bar } from "@/lib/ta/core";

import type { BuiltinVar } from "./types";

export const utcDayStart = (t: number): number => Math.floor(t / 86_400_000) * 86_400_000;

export type TaSeriesName = "tr" | "obv" | "accdist" | "vwap";

export function computeTaSeries(name: TaSeriesName, bars: Bar[]): (number | null)[] {
  const out: (number | null)[] = new Array(bars.length).fill(null);
  let total = 0;
  let day = NaN;
  let pv = 0;
  let vol = 0;
  for (let i = 0; i < bars.length; i++) {
    const b = bars[i];
    switch (name) {
      case "tr":
        out[i] = i === 0 ? null : Math.max(b.h - b.l, Math.abs(b.h - bars[i - 1].c), Math.abs(b.l - bars[i - 1].c));
        break;
      case "obv":
        if (i > 0) total += Math.sign(b.c - bars[i - 1].c) * b.v;
        out[i] = total;
        break;
      case "accdist":
        total += (b.c === b.h && b.c === b.l) || b.h === b.l ? 0 : ((2 * b.c - b.l - b.h) / (b.h - b.l)) * b.v;
        out[i] = total;
        break;
      case "vwap": {
        const d = utcDayStart(b.t);
        if (d !== day) {
          day = d;
          pv = 0;
          vol = 0;
        }
        pv += ((b.h + b.l + b.c) / 3) * b.v;
        vol += b.v;
        out[i] = vol === 0 ? null : pv / vol;
        break;
      }
    }
  }
  return out;
}

const taVar = (name: TaSeriesName): BuiltinVar => ({
  name: `ta.${name}`,
  type: "float",
  q: "series",
  get: (rt, off) => {
    const i = rt.bar - off;
    return i < 0 ? null : rt.series(name)[i];
  },
});

export const TA_VARS: BuiltinVar[] = [taVar("tr"), taVar("obv"), taVar("accdist"), taVar("vwap")];
