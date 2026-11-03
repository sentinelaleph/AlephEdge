/**
 * The built-in registry contract. One BuiltinFn per supported function; the
 * checker binds and validates calls against it, the interpreter calls
 * `impl`, and the future "supported features" page is generated from the
 * same list, so documentation cannot drift from behaviour.
 */
import type { Bar } from "@/lib/ta/core";

import type { Output } from "../../outputs";
import type { Value, Version } from "../values";

export type PType = "int" | "float" | "bool" | "string" | "color" | "void" | "tuple" | "handle" | "any";
export type Qual = "const" | "input" | "simple" | "series";

export type SourceName = "open" | "high" | "low" | "close" | "volume" | "hl2" | "hlc3" | "ohlc4" | "hlcc4";

export interface Param {
  name: string;
  /** Default when omitted (na when absent and not required). */
  def?: Value;
  /** Default is the current bar's value of this source (ta.highest(length) -> high). */
  src?: SourceName;
  req?: boolean;
  /** Pine `simple`: a series argument is a compile-time error. */
  simple?: boolean;
  /** Must be a compile-time constant (titles, styles, input defaults). */
  constant?: boolean;
}

export interface BuiltinFn {
  name: string;
  params: Param[];
  /** Alternative positional orders (names from params); default = params order. */
  sigs?: string[][];
  /** All positional args are passed through as-is (math.max, str.format). */
  variadic?: boolean;
  /** Keeps per-call-site state (ta.*, math.sum, fixnan, ...). */
  stateful?: boolean;
  /** plot-family: global scope only, statement level, counts toward the plot cap. */
  output?: boolean;
  returns: PType;
  /** Tuple size when returns === "tuple". */
  tuple?: number;
  /** Named arguments accepted and ignored because they only affect TradingView's UI. */
  ignored?: string[];
  /** Real Pine arguments Pine-lite v1 rejects by name (gradient fills, force_overlay, ...). */
  unsupportedArgs?: string[];
  impl(rt: Rt, a: Value[]): Value;
}

export interface BuiltinVar {
  name: string;
  type: PType;
  q: Qual;
  /** Value `off` bars back (bar-indexed, not per call). */
  get(rt: Rt, off: number): Value;
}

export interface HostInfo {
  /** syminfo.ticker */
  ticker: string;
  /** syminfo.mintick */
  mintick: number;
  /** timeframe.period, Pine format ("1", "60", "240", "1D", "1W", ...). */
  timeframe: string;
  /** Bar duration in ms, for time_close. */
  intervalMs: number;
  /**
   * bar_index of the first loaded bar (default 0, the chart's own count). A
   * host that re-runs on a sliding window of bars (the Aleph Edge Pine bot)
   * anchors it to the bar time, so bar_index grows by one per bar between
   * runs as it does on a live chart.
   */
  barIndexBase: number;
}

/** What built-in implementations may use from the interpreter. */
export interface Rt {
  readonly bars: Bar[];
  /** Current bar index (also the history epoch). */
  readonly bar: number;
  readonly version: Version;
  readonly host: HostInfo;
  /** Per (function instance, call site) state, created on first use. */
  state<T>(make: () => T): T;
  /** Run-time error at the current call. */
  fail(message: string): never;
  /** Seeded uniform [0, 1) for math.random without a seed. */
  random(): number;
  /** Bar-indexed built-in series (ta.tr, ta.obv, ta.accdist, ta.vwap), computed once per run. */
  series(name: "tr" | "obv" | "accdist" | "vwap"): (number | null)[];
  /** Registers an output descriptor; returns its id. */
  addOutput(o: Output): number;
  output(id: number): Output;
}

/** OHLCV/derived value of bar `i`; na outside the loaded bars. */
export function barValue(bars: Bar[], i: number, s: SourceName): number | null {
  if (i < 0 || i >= bars.length) return null;
  const b = bars[i];
  switch (s) {
    case "open":
      return b.o;
    case "high":
      return b.h;
    case "low":
      return b.l;
    case "close":
      return b.c;
    case "volume":
      return b.v;
    case "hl2":
      return (b.h + b.l) / 2;
    case "hlc3":
      return (b.h + b.l + b.c) / 3;
    case "ohlc4":
      return (b.o + b.h + b.l + b.c) / 4;
    case "hlcc4":
      return (b.h + b.l + b.c + b.c) / 4;
  }
}
