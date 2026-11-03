/**
 * Built-in variables. All bars are historical in v1, so
 * barstate.isconfirmed is always true and barstate.islast marks the last
 * loaded bar. syminfo.* and timeframe.period come from RunOptions.host.
 */
import { TA_VARS } from "./taVars";
import { barValue, type BuiltinVar, type PType, type Qual, type Rt, type SourceName } from "./types";

const at = (rt: Rt, off: number): number => rt.bar - off;

const source = (name: SourceName): BuiltinVar => ({
  name,
  type: "float",
  q: "series",
  get: (rt, off) => barValue(rt.bars, at(rt, off), name),
});

const v = (name: string, type: PType, q: Qual, get: BuiltinVar["get"]): BuiltinVar => ({ name, type, q, get });

const inRange = (rt: Rt, i: number) => i >= 0 && i < rt.bars.length;

export const BAR_VARS: BuiltinVar[] = [
  ...(["open", "high", "low", "close", "volume", "hl2", "hlc3", "ohlc4", "hlcc4"] as SourceName[]).map(source),
  v("time", "int", "series", (rt, off) => (inRange(rt, at(rt, off)) ? rt.bars[at(rt, off)].t : null)),
  v("time_close", "int", "series", (rt, off) => (inRange(rt, at(rt, off)) ? rt.bars[at(rt, off)].t + rt.host.intervalMs : null)),
  v("bar_index", "int", "series", (rt, off) => (at(rt, off) >= 0 ? rt.host.barIndexBase + at(rt, off) : null)),
  v("last_bar_index", "int", "series", (rt) => rt.host.barIndexBase + rt.bars.length - 1),
  v("timeframe.period", "string", "simple", (rt) => rt.host.timeframe),
  v("syminfo.ticker", "string", "simple", (rt) => rt.host.ticker),
  v("syminfo.mintick", "float", "simple", (rt) => rt.host.mintick),
  v("barstate.isfirst", "bool", "series", (rt, off) => at(rt, off) === 0),
  v("barstate.islast", "bool", "series", (rt, off) => at(rt, off) === rt.bars.length - 1),
  v("barstate.isconfirmed", "bool", "series", () => true),
  ...TA_VARS,
];
