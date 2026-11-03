/**
 * History storage.
 *
 * Pine's model: a series records one value per *execution* of the code that
 * produces it. Global code runs once per bar, so its history is per bar; code
 * in a user function, an `if` block or a conditionally called built-in only
 * records when it actually runs, which is why a skipped call does not advance
 * its state. Executing again on the same bar (loops, repeated calls in one
 * bar) overwrites that bar's value instead of adding one.
 *
 * Every store here therefore takes an `epoch` (the bar index): a write with a
 * new epoch first commits the previous value to history, a write with the
 * same epoch replaces the pending value.
 */
import type { Value } from "./values";

/** Default bound for histories read with a non-constant offset. */
export const DEFAULT_HISTORY = 5000;

/** A variable slot: the current value plus committed history. */
export class Slot {
  cur: Value = null;
  ep = -1;
  init = false;
  private hist: Value[] = [];

  constructor(private readonly depth: number) {}

  /** Starts (or restarts, same epoch) this execution's value. */
  declare(v: Value, ep: number): void {
    if (this.ep !== -1 && this.ep !== ep) {
      this.hist.push(this.cur);
      if (this.hist.length > this.depth * 2 + 16) this.hist.splice(0, this.hist.length - this.depth);
    }
    this.cur = v;
    this.ep = ep;
    this.init = true;
  }

  /** x[n]; na beyond the recorded (or retained) history. */
  read(n: number): Value {
    if (n === 0) return this.cur;
    if (n > this.depth) return null;
    const i = this.hist.length - n;
    return i >= 0 ? this.hist[i] : null;
  }
}

/**
 * Input history of a stateful built-in: one value per call. With `skipNa`
 * (the built-ins TradingView documents as "na values in the source series
 * are ignored"), na inputs are not recorded at all, so windows hold the last
 * `length` non-na values.
 */
export class Hist {
  private v: (number | null)[] = [];
  private e: number[] = [];
  private ep = -1;

  constructor(
    private readonly skipNa = false,
    private readonly max = DEFAULT_HISTORY + 1,
  ) {}

  push(x: number | null, ep: number): void {
    const same = ep === this.ep && this.v.length > 0;
    if (x === null && this.skipNa) {
      if (same) {
        this.v.pop();
        this.e.pop();
        this.ep = this.e.length > 0 ? this.e[this.e.length - 1] : -1;
      }
      return;
    }
    if (same) {
      this.v[this.v.length - 1] = x;
      return;
    }
    this.v.push(x);
    this.e.push(ep);
    this.ep = ep;
    if (this.v.length > this.max * 2) {
      this.v.splice(0, this.v.length - this.max);
      this.e.splice(0, this.e.length - this.max);
    }
  }

  /** Epoch (bar) at which the value `n` entries back was recorded. */
  epochAt(n: number): number {
    return this.e[this.e.length - 1 - n] ?? -1;
  }

  /** Value `n` calls ago (0 = this call); na beyond history. */
  at(n: number): number | null {
    const i = this.v.length - 1 - n;
    return i >= 0 ? this.v[i] : null;
  }

  /** Number of calls recorded (capped by retention). */
  get size(): number {
    return this.v.length;
  }

  /** The last `len` values, newest first; null if fewer exist or any is na. */
  window(len: number): number[] | null {
    if (len < 1 || this.v.length < len) return null;
    const out = new Array<number>(len);
    for (let k = 0; k < len; k++) {
      const x = this.v[this.v.length - 1 - k];
      if (x === null) return null;
      out[k] = x;
    }
    return out;
  }

  /**
   * The last `len` values, newest first, keeping na entries; null if fewer
   * than `len` calls exist.
   */
  raw(len: number): (number | null)[] | null {
    if (len < 1 || this.v.length < len) return null;
    const out = new Array<number | null>(len);
    for (let k = 0; k < len; k++) out[k] = this.v[this.v.length - 1 - k];
    return out;
  }
}

/**
 * Recursive state (ema, rma, sar, ...): `prev` is the value committed by the
 * previous call (epoch), `cur` the value of this call.
 */
export class Rec<T> {
  private prev: T;
  private cur: T;
  private ep = -1;

  constructor(private readonly initial: T) {
    this.prev = initial;
    this.cur = initial;
  }

  step(ep: number, f: (prev: T) => T): T {
    if (ep !== this.ep) {
      if (this.ep !== -1) this.prev = this.cur;
      this.ep = ep;
    }
    this.cur = f(this.prev);
    return this.cur;
  }

}
