/**
 * The single built-in registry. The checker resolves names against it, the
 * interpreter runs what it returns, and the supported-features page (UI
 * phase) is generated from it. Anything Pine has that is not here is
 * rejected by name: `unsupportedReason` gives the message and a hint.
 */
import { NOT_IN_V1 } from "./errors";
import { COLOR_FNS } from "./runtime/builtins/color";
import { CONSTANTS } from "./runtime/builtins/constants";
import { CORE } from "./runtime/builtins/core";
import { INPUT_FNS } from "./runtime/builtins/input";
import { MATH } from "./runtime/builtins/math";
import { PLOT_FNS } from "./runtime/builtins/plot";
import { AVERAGES } from "./runtime/builtins/taAverages";
import { EVENTS } from "./runtime/builtins/taEvents";
import { OSCILLATORS } from "./runtime/builtins/taOsc";
import { STATS } from "./runtime/builtins/taStats";
import { VOLATILITY } from "./runtime/builtins/taVol";
import type { BuiltinFn, BuiltinVar } from "./runtime/builtins/types";
import { BAR_VARS } from "./runtime/builtins/vars";
import type { Value } from "./runtime/values";

const ALL_FNS: BuiltinFn[] = [...AVERAGES, ...STATS, ...OSCILLATORS, ...VOLATILITY, ...EVENTS, ...MATH, ...CORE, ...COLOR_FNS, ...INPUT_FNS, ...PLOT_FNS];

export const FUNCTIONS: ReadonlyMap<string, BuiltinFn> = new Map(ALL_FNS.map((f) => [f.name, f]));
export const VARIABLES: ReadonlyMap<string, BuiltinVar> = new Map(BAR_VARS.map((v) => [v.name, v]));
export const CONSTANT_VALUES: Readonly<Record<string, Value>> = CONSTANTS;

/** The script declaration functions handled by the checker itself. */
export const DECLARATIONS = new Set(["indicator"]);

const HINTS: [RegExp, string][] = [
  [/^strategy(\.|$)/, "strategy() scripts and order simulation are not in v1; use indicator() and plotshape() for signals"],
  [/^request(\.|$)/, "Other symbols and timeframes are not available; Pine-lite runs on the chart's own candles"],
  [/^(array|matrix|map)\./, "Collections are not in v1"],
  [/^(line|label|box|table|polyline|linefill|chart)\./, "Drawing objects are not in v1; use plot, plotshape, hline, fill or bgcolor"],
  [/^timeframe\./, "Only timeframe.period is available"],
  [/^syminfo\./, "Only syminfo.ticker and syminfo.mintick are available"],
  [/^barstate\./, "All bars are historical in v1; barstate.isfirst, islast and isconfirmed are available"],
  [/^plot\.style_/, "Supported styles: line, linebr, stepline, histogram, columns, circles, cross, area"],
];

/** Namespaces whose members Pine-lite knows to be Pine built-ins. */
const PINE_NAMESPACES = /^(ta|math|str|color|input|plot|shape|location|size|hline|display|format|timeframe|syminfo|barstate|request|strategy|array|matrix|map|line|label|box|table|polyline|linefill|chart|ticker|session|runtime|log|alert|currency|dayofweek|earnings|dividends|splits|position|xloc|yloc|extend|font|text|order|adjustment|backadjustment|scale|barmerge)\./;

/** Bare Pine built-ins that v1 does not provide. */
const PINE_BARE = new Set([
  "strategy", "library", "alert", "plotcandle", "plotbar", "dayofweek", "dayofmonth", "hour", "minute", "second", "month", "year",
  "weekofyear", "timenow", "time_tradingday", "last_bar_time", "max_bars_back", "timestamp", "ask", "bid", "linefill", "varip",
]);

export interface Unsupported {
  message: string;
  hint?: string;
}

/** Why a Pine name is rejected, or null when it is not a known Pine built-in. */
export function unsupportedReason(name: string): Unsupported | null {
  if (name === "study") return { message: "study() is Pine v4; Pine-lite runs v5 and v6 scripts", hint: "Use indicator() and //@version=5 or 6" };
  if (!PINE_NAMESPACES.test(name) && !PINE_BARE.has(name)) return null;
  const hint = HINTS.find(([re]) => re.test(name))?.[1];
  return hint === undefined ? { message: `${name} ${NOT_IN_V1}` } : { message: `${name} ${NOT_IN_V1}`, hint };
}

/**
 * What Pine-lite rejects, for the supported-features page. Each entry names
 * a feature and one `sample` that must fail to compile (a statement placed
 * after a v6 indicator() header, or a whole script when it starts with
 * //@version) (tests/lib/pine/api.test.ts checks every sample), so
 * this list cannot claim a rejection the checker does not make.
 */
export interface UnsupportedFeature {
  feature: string;
  note: string;
  sample: string;
}

export const UNSUPPORTED_FEATURES: readonly UnsupportedFeature[] = [
  { feature: "strategy() and strategy.*", note: "Order simulation is not in v1; use indicator() and plotshape() for signals", sample: 'strategy.entry("L", strategy.long)' },
  { feature: "request.security and request.*", note: "Other symbols and timeframes; Pine-lite runs on the chart's own candles", sample: 'x = request.security("BTCUSDT", "D", close)' },
  { feature: "library, import, export", note: "Libraries are not in v1", sample: "import user/lib/1" },
  { feature: "type (user-defined types), method", note: "Not in v1", sample: "type P\n    float x" },
  { feature: "array.*, matrix.*, map.*", note: "Collections are not in v1", sample: "a = array.new_float(0)" },
  { feature: "line.*, label.*, box.*, table.*, polyline.*, linefill.*", note: "Drawing objects; use plot, plotshape, hline, fill or bgcolor", sample: "l = label.new(bar_index, high, \"x\")" },
  { feature: "varip", note: "All bars are historical in v1", sample: "varip int n = 0" },
  { feature: "timeframe.* except timeframe.period", note: "Only the chart timeframe is known", sample: "x = timeframe.isdaily" },
  { feature: "syminfo.* except ticker and mintick", note: "Only syminfo.ticker and syminfo.mintick are supplied", sample: "x = syminfo.currency" },
  { feature: "plotcandle, plotbar, alert()", note: "Not in v1; alertcondition() is listed in the panel", sample: "plotcandle(open, high, low, close)" },
  { feature: "plot.style_steplinebr, plot.style_areabr and other break styles", note: "Supported styles: line, linebr, stepline, histogram, columns, circles, cross, area", sample: "plot(close, style = plot.style_steplinebr)" },
  { feature: "Gradient fill(top_value, bottom_value, ...)", note: "fill() takes two plots or two hlines and one colour", sample: "a = plot(close)\nb = plot(open)\nfill(a, b, top_value = 1, bottom_value = 0, top_color = color.red, bottom_color = color.blue)" },
  { feature: "Pine v4 and older (study(), no ta. namespace)", note: "Use //@version=5 or 6 and indicator()", sample: '//@version=4\nstudy("x")\nplot(close)' },
];
