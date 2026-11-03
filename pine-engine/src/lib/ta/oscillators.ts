/**
 * Own-pane indicators. Standard definitions, Wilder smoothing where the
 * original uses it (RSI, ATR, ADX/DMI).
 */

import { macd as macdLines, rsi as rsiCloses } from "@/lib/indicators";

import { combine, highest, lowest, nulls, rma, smaOf, trueRange, typicalPrice, type Bar, type Series } from "./core";

/** RSI (Wilder), first value at index n. */
export function rsi(bars: Bar[], n: number): Series {
  return rsiCloses(bars.map((b) => b.c), n);
}

/** MACD: EMA(fast) − EMA(slow), its EMA(signal), and the difference. */
export function macd(bars: Bar[], fast: number, slow: number, signal: number): { macd: Series; signal: Series; histogram: Series } {
  return macdLines(bars.map((b) => b.c), fast, slow, signal);
}

/** Where `src` sits in its own n-bar high/low range, 0..100; null on a flat range. */
function stochOf(src: Series, hi: Series, lo: Series): Series {
  return src.map((v, i) => {
    const h = hi[i];
    const l = lo[i];
    if (v === null || h === null || l === null || h === l) return null;
    return (100 * (v - l)) / (h - l);
  });
}

/** Rolling max/min of a nullable series; null while any value in the window is null. */
function rolling(values: Series, n: number, pick: (a: number, b: number) => number): Series {
  const out = nulls(values.length);
  for (let i = n - 1; i < values.length; i++) {
    let m: number | null = null;
    for (let j = i - n + 1; j <= i; j++) {
      const v = values[j];
      if (v === null) {
        m = null;
        break;
      }
      m = m === null ? v : pick(m, v);
    }
    out[i] = m;
  }
  return out;
}

/** Stochastic (full): %K = SMA(raw %K over kLen, smooth), %D = SMA(%K, d). */
export function stochastic(bars: Bar[], kLen: number, smooth: number, d: number): { k: Series; d: Series } {
  const raw = stochOf(bars.map((b) => b.c), highest(bars.map((b) => b.h), kLen), lowest(bars.map((b) => b.l), kLen));
  const k = smaOf(raw, smooth);
  return { k, d: smaOf(k, d) };
}

/** Stochastic RSI: the stochastic of RSI(rsiLen) over stochLen, %K = SMA(smoothK), %D = SMA(%K, d). */
export function stochRsi(bars: Bar[], rsiLen: number, stochLen: number, smoothK: number, d: number): { k: Series; d: Series } {
  const r = rsi(bars, rsiLen);
  const raw = stochOf(r, rolling(r, stochLen, Math.max), rolling(r, stochLen, Math.min));
  const k = smaOf(raw, smoothK);
  return { k, d: smaOf(k, d) };
}

/** Average True Range, Wilder's RMA of the true range; first value at index n − 1. */
export function atr(bars: Bar[], n: number): Series {
  return rma(trueRange(bars), n);
}

/**
 * ADX / DMI (Wilder). +DM/−DM from bar 1; TR, +DM and −DM smoothed with RMA
 * over diLen, so +DI/−DI start at index diLen; ADX is the RMA of DX over
 * adxLen and starts at index diLen + adxLen − 1.
 */
export function adx(bars: Bar[], diLen: number, adxLen: number): { adx: Series; plusDi: Series; minusDi: Series } {
  const n = bars.length;
  const tr: Series = nulls(n);
  const plusDm: Series = nulls(n);
  const minusDm: Series = nulls(n);
  const trAll = trueRange(bars);
  for (let i = 1; i < n; i++) {
    const upMove = bars[i].h - bars[i - 1].h;
    const downMove = bars[i - 1].l - bars[i].l;
    plusDm[i] = upMove > downMove && upMove > 0 ? upMove : 0;
    minusDm[i] = downMove > upMove && downMove > 0 ? downMove : 0;
    tr[i] = trAll[i];
  }
  const str = rma(tr, diLen);
  const pdi = combine(rma(plusDm, diLen), str, (p, t) => (t === 0 ? null : (100 * p) / t));
  const mdi = combine(rma(minusDm, diLen), str, (m, t) => (t === 0 ? null : (100 * m) / t));
  const dx = combine(pdi, mdi, (p, m) => (p + m === 0 ? 0 : (100 * Math.abs(p - m)) / (p + m)));
  return { adx: rma(dx, adxLen), plusDi: pdi, minusDi: mdi };
}

/** CCI: (TP − SMA(TP, n)) / (0.015 × mean absolute deviation of TP). */
export function cci(bars: Bar[], n: number): Series {
  const tp = typicalPrice(bars);
  const mean = smaOf(tp, n);
  return mean.map((m, i) => {
    if (m === null) return null;
    let dev = 0;
    for (let j = i - n + 1; j <= i; j++) dev += Math.abs(tp[j] - m);
    dev /= n;
    return dev === 0 ? null : (tp[i] - m) / (0.015 * dev);
  });
}

/** Williams %R: −100 × (highest high − close) / (highest high − lowest low), over n. */
export function williamsR(bars: Bar[], n: number): Series {
  const hh = highest(bars.map((b) => b.h), n);
  const ll = lowest(bars.map((b) => b.l), n);
  return bars.map((b, i) => {
    const h = hh[i];
    const l = ll[i];
    if (h === null || l === null || h === l) return null;
    return (-100 * (h - b.c)) / (h - l);
  });
}

/** On-Balance Volume from the first loaded bar (0 there): + volume on an up close, − on a down close. */
export function obv(bars: Bar[]): Series {
  let total = 0;
  return bars.map((b, i) => {
    if (i > 0) total += b.c > bars[i - 1].c ? b.v : b.c < bars[i - 1].c ? -b.v : 0;
    return total;
  });
}

/**
 * Money Flow Index: raw money flow = TP × volume, positive when TP rose,
 * negative when it fell; MFI = 100 − 100 / (1 + Σpositive / Σnegative) over n
 * bars. First value at index n; no flow at all gives null.
 */
export function mfi(bars: Bar[], n: number): Series {
  const out = nulls(bars.length);
  const tp = typicalPrice(bars);
  for (let i = n; i < bars.length; i++) {
    let pos = 0;
    let neg = 0;
    for (let j = i - n + 1; j <= i; j++) {
      const flow = tp[j] * bars[j].v;
      if (tp[j] > tp[j - 1]) pos += flow;
      else if (tp[j] < tp[j - 1]) neg += flow;
    }
    out[i] = neg === 0 ? (pos === 0 ? null : 100) : 100 - 100 / (1 + pos / neg);
  }
  return out;
}
