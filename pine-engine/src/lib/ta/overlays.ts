/**
 * Price-pane indicators. Standard definitions; parameters arrive already
 * clamped by the caller (app/chart/indicatorRegistry.ts).
 */

import { bollinger as bollingerBands } from "@/lib/indicators";

import { emaOf, highest, lowest, nulls, rma, smaOf, trueRange, typicalPrice, type Bar, type Series } from "./core";

export function sma(bars: Bar[], n: number): Series {
  return smaOf(bars.map((b) => b.c), n);
}

export function ema(bars: Bar[], n: number): Series {
  return emaOf(bars.map((b) => b.c), n);
}

/** Bollinger Bands: SMA(n) ± mult × population standard deviation of close. */
export function bollinger(bars: Bar[], n: number, mult: number): { upper: Series; middle: Series; lower: Series } {
  return bollingerBands(bars.map((b) => b.c), n, mult);
}

export type VwapAnchor = "day" | "week" | "month";

/** Start (ms) of the UTC session that contains `t`: day, ISO week (Monday) or month. */
export function sessionStart(t: number, anchor: VwapAnchor): number {
  const d = new Date(t);
  if (anchor === "month") return Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), 1);
  const day = Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate());
  if (anchor === "day") return day;
  const sinceMonday = (d.getUTCDay() + 6) % 7;
  return day - sinceMonday * 86_400_000;
}

/**
 * Anchored VWAP: Σ(typical price × volume) / Σ volume, reset at the start of
 * each UTC session (day by default). A session whose first bar is not in the
 * loaded window starts at the first loaded bar, so the first session can be
 * partial; zero volume so far gives null.
 */
export function vwap(bars: Bar[], anchor: VwapAnchor): Series {
  const out = nulls(bars.length);
  const tp = typicalPrice(bars);
  let session = NaN;
  let pv = 0;
  let vol = 0;
  for (let i = 0; i < bars.length; i++) {
    const s = sessionStart(bars[i].t, anchor);
    if (s !== session) {
      session = s;
      pv = 0;
      vol = 0;
    }
    pv += tp[i] * bars[i].v;
    vol += bars[i].v;
    out[i] = vol > 0 ? pv / vol : null;
  }
  return out;
}

export interface Ichimoku {
  tenkan: Series;
  kijun: Series;
  /** Unshifted; the caller plots them `displacement − 1` bars ahead. */
  spanA: Series;
  spanB: Series;
  /** Close; the caller plots it `displacement − 1` bars back. */
  chikou: Series;
}

const midpoint = (bars: Bar[], n: number): Series => {
  const hh = highest(bars.map((b) => b.h), n);
  const ll = lowest(bars.map((b) => b.l), n);
  return hh.map((h, i) => (h === null || ll[i] === null ? null : (h + (ll[i] as number)) / 2));
};

/**
 * Ichimoku Kinko Hyo: Tenkan = midpoint of the last `conv` highs/lows, Kijun
 * of `base`, Senkou A = (Tenkan + Kijun) / 2, Senkou B = midpoint of `spanB`.
 * Values are returned at the bar they were computed on; the display offset is
 * applied by the chart (TradingView convention: displacement − 1).
 */
export function ichimoku(bars: Bar[], conv: number, base: number, spanBLen: number): Ichimoku {
  const tenkan = midpoint(bars, conv);
  const kijun = midpoint(bars, base);
  const spanA = tenkan.map((t, i) => (t === null || kijun[i] === null ? null : (t + (kijun[i] as number)) / 2));
  return { tenkan, kijun, spanA, spanB: midpoint(bars, spanBLen), chikou: bars.map((b) => b.c) };
}

/**
 * SuperTrend (ATR = Wilder RMA of true range), the TradingView ta.supertrend
 * rules: bands at hl2 ± factor × ATR that only tighten while the trend holds;
 * the trend flips when the close crosses the active band. `up` holds the
 * line while the trend is up (line under price), `down` while it is down.
 */
export function supertrend(bars: Bar[], atrLen: number, factor: number): { up: Series; down: Series; trend: (1 | -1 | null)[] } {
  const n = bars.length;
  const up = nulls(n);
  const down = nulls(n);
  const trend: (1 | -1 | null)[] = new Array(n).fill(null);
  const atr = rma(trueRange(bars), atrLen);
  let prevUpper = 0;
  let prevLower = 0;
  let prevST: number | null = null;
  let prevAtr: number | null = null;
  for (let i = 0; i < n; i++) {
    const a = atr[i];
    if (a === null) continue;
    const hl2 = (bars[i].h + bars[i].l) / 2;
    const prevClose = i > 0 ? bars[i - 1].c : bars[i].c;
    let upper = hl2 + factor * a;
    let lower = hl2 - factor * a;
    lower = lower > prevLower || prevClose < prevLower ? lower : prevLower;
    upper = upper < prevUpper || prevClose > prevUpper ? upper : prevUpper;
    let dir: 1 | -1;
    if (prevAtr === null) dir = -1;
    else if (prevST === prevUpper) dir = bars[i].c > upper ? 1 : -1;
    else dir = bars[i].c < lower ? -1 : 1;
    const st = dir === 1 ? lower : upper;
    if (dir === 1) up[i] = st;
    else down[i] = st;
    trend[i] = dir;
    prevUpper = upper;
    prevLower = lower;
    prevST = st;
    prevAtr = a;
  }
  return { up, down, trend };
}

/**
 * Parabolic SAR (Wilder), the TradingView ta.sar algorithm: the first trend
 * is set by bar 1's close against bar 0's; the SAR never enters the last two
 * bars' range. First value at index 1.
 */
export function parabolicSar(bars: Bar[], start: number, inc: number, max: number): Series {
  const out = nulls(bars.length);
  if (bars.length < 2) return out;
  let result = 0;
  let maxMin = 0;
  let accel = start;
  let isBelow = false;
  for (let i = 1; i < bars.length; i++) {
    const b = bars[i];
    let firstTrendBar = false;
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
      firstTrendBar = true;
      accel = start;
    }
    result = result + accel * (maxMin - result);
    if (isBelow) {
      if (result > b.l) {
        firstTrendBar = true;
        isBelow = false;
        result = Math.max(b.h, maxMin);
        maxMin = b.l;
        accel = start;
      }
    } else if (result < b.h) {
      firstTrendBar = true;
      isBelow = true;
      result = Math.min(b.l, maxMin);
      maxMin = b.h;
      accel = start;
    }
    if (!firstTrendBar) {
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
    out[i] = result;
  }
  return out;
}

/** Keltner Channels: EMA(n) of close ± mult × ATR(atrLen) (Wilder). */
export function keltner(bars: Bar[], n: number, atrLen: number, mult: number): { upper: Series; middle: Series; lower: Series } {
  const middle = ema(bars, n);
  const atr = rma(trueRange(bars), atrLen);
  const upper = middle.map((m, i) => (m === null || atr[i] === null ? null : m + mult * (atr[i] as number)));
  const lower = middle.map((m, i) => (m === null || atr[i] === null ? null : m - mult * (atr[i] as number)));
  return { upper, middle, lower };
}
