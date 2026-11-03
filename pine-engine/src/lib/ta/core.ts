/**
 * Shared building blocks for the /chart indicators. Every function returns
 * one value per input bar and `null` until it has enough history (the warm-up
 * bars), so outputs line up index for index with the candles and nothing is
 * drawn that was not computed. Pure, no DOM, no network.
 */

export type Series = (number | null)[];

/** One candle; the shape of services/api MarketCandle. */
export interface Bar {
  /** Open time, ms since epoch. */
  t: number;
  o: number;
  h: number;
  l: number;
  c: number;
  v: number;
}

export function nulls(n: number): Series {
  return new Array<number | null>(n).fill(null);
}

/** Simple average of the last `n` values; null while any of them is null. */
export function smaOf(values: Series, n: number): Series {
  const out = nulls(values.length);
  if (n < 1) return out;
  let sum = 0;
  let run = 0;
  for (let i = 0; i < values.length; i++) {
    const v = values[i];
    if (v === null) {
      sum = 0;
      run = 0;
      continue;
    }
    sum += v;
    run += 1;
    if (run > n) sum -= values[i - n] as number;
    if (run >= n) out[i] = sum / n;
  }
  return out;
}

/**
 * Recursive average seeded with the simple average of the first `n`
 * consecutive values: alpha 2/(n+1) for EMA, 1/n for Wilder's RMA. A null
 * inside the series restarts the seed.
 */
function seededAverage(values: Series, n: number, alpha: number): Series {
  const out = nulls(values.length);
  if (n < 1) return out;
  let prev: number | null = null;
  let seed = 0;
  let run = 0;
  for (let i = 0; i < values.length; i++) {
    const v = values[i];
    if (v === null) {
      prev = null;
      seed = 0;
      run = 0;
      continue;
    }
    if (prev === null) {
      seed += v;
      run += 1;
      if (run === n) {
        prev = seed / n;
        out[i] = prev;
      }
      continue;
    }
    prev = alpha * v + (1 - alpha) * prev;
    out[i] = prev;
  }
  return out;
}

/** Exponential moving average, SMA-seeded. */
export function emaOf(values: Series, n: number): Series {
  return seededAverage(values, n, 2 / (n + 1));
}

/** Wilder's smoothing (RMA): (prev × (n − 1) + value) / n, SMA-seeded. */
export function rma(values: Series, n: number): Series {
  return seededAverage(values, n, 1 / n);
}

/** Highest of the last `n` values; null until `n` values exist. */
export function highest(values: number[], n: number): Series {
  const out = nulls(values.length);
  for (let i = n - 1; i < values.length; i++) {
    let m = -Infinity;
    for (let j = i - n + 1; j <= i; j++) if (values[j] > m) m = values[j];
    out[i] = m;
  }
  return out;
}

/** Lowest of the last `n` values; null until `n` values exist. */
export function lowest(values: number[], n: number): Series {
  const out = nulls(values.length);
  for (let i = n - 1; i < values.length; i++) {
    let m = Infinity;
    for (let j = i - n + 1; j <= i; j++) if (values[j] < m) m = values[j];
    out[i] = m;
  }
  return out;
}

/** True range; the first bar has no previous close, so it is high − low. */
export function trueRange(bars: Bar[]): number[] {
  return bars.map((b, i) => {
    if (i === 0) return b.h - b.l;
    const pc = bars[i - 1].c;
    return Math.max(b.h - b.l, Math.abs(b.h - pc), Math.abs(b.l - pc));
  });
}

/** Typical price (high + low + close) / 3. */
export function typicalPrice(bars: Bar[]): number[] {
  return bars.map((b) => (b.h + b.l + b.c) / 3);
}

/** a op b per index; null where either side is null. */
export function combine(a: Series, b: Series, op: (x: number, y: number) => number | null): Series {
  return a.map((x, i) => {
    const y = b[i];
    return x === null || y === null ? null : op(x, y);
  });
}

/** Clamp an integer parameter to [min, max]; NaN gives the fallback. */
export function intParam(v: unknown, fallback: number, min = 1, max = 500): number {
  const n = typeof v === "number" ? v : Number(v);
  if (!Number.isFinite(n)) return fallback;
  return Math.min(max, Math.max(min, Math.round(n)));
}

/** Clamp a decimal parameter to [min, max]; NaN gives the fallback. */
export function numParam(v: unknown, fallback: number, min: number, max: number): number {
  const n = typeof v === "number" ? v : Number(v);
  if (!Number.isFinite(n)) return fallback;
  return Math.min(max, Math.max(min, n));
}
