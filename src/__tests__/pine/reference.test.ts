/**
 * Every ta.* and math.* built-in against fixtures/reference.json, produced by
 * fixtures/gen_reference.py: an independent Python implementation of
 * TradingView's documented definitions. Full series are compared (warm-up
 * na positions included) on 300 bars that end in an edge segment (flat range,
 * zero volume, a 30 h time gap, a price gap, monotone runs).
 */
import { describe, expect, it } from "vitest";

import { FUNCTIONS, VARIABLES } from "@/lib/pine";

import { expectSeries, ok, plots, plotScript, SERIES } from "./helpers";

const PRELUDE = [
  "nasrc = bar_index % 7 == 3 ? na : close",
  "e10 = ta.ema(close, 10)",
  "[m, s, h] = ta.macd(close, 12, 26, 9)",
  "[bbm, bbu, bbl] = ta.bb(close, 20, 2)",
  "[kcm, kcu, kcl] = ta.kc(close, 20, 1.5)",
  "[kcm2, kcu2, kcl2] = ta.kc(close, 20, 1.5, false)",
  "[dp, dm, adx] = ta.dmi(14, 14)",
  "[st, stdir] = ta.supertrend(3, 10)",
];

const GROUPS: Record<string, [string, string][]> = {
  averages: [
    ["sma", "ta.sma(close, 14)"], ["ema", "ta.ema(close, 14)"], ["rma", "ta.rma(close, 14)"], ["wma", "ta.wma(close, 9)"],
    ["swma", "ta.swma(close)"], ["vwma", "ta.vwma(close, 20)"], ["hma", "ta.hma(close, 9)"], ["alma", "ta.alma(close, 9, 0.85, 6)"],
    ["alma_floor", "ta.alma(close, 10, 0.85, 6, true)"], ["linreg", "ta.linreg(close, 20, 0)"], ["linreg2", "ta.linreg(close, 20, 2)"],
  ],
  statistics: [
    ["stdev", "ta.stdev(close, 20)"], ["stdev_u", "ta.stdev(close, 20, false)"], ["variance", "ta.variance(close, 20)"],
    ["dev", "ta.dev(close, 20)"], ["median10", "ta.median(close, 10)"], ["median9", "ta.median(close, 9)"], ["highest", "ta.highest(14)"],
    ["highest_src", "ta.highest(close, 10)"], ["lowest", "ta.lowest(14)"], ["highestbars", "ta.highestbars(high, 14)"],
    ["lowestbars", "ta.lowestbars(14)"], ["range", "ta.range(close, 10)"], ["percentrank", "ta.percentrank(close, 20)"],
    ["correlation", "ta.correlation(close, volume, 20)"], ["cog", "ta.cog(close, 10)"],
  ],
  oscillators: [
    ["rsi", "ta.rsi(close, 14)"], ["macd", "m"], ["macd_signal", "s"], ["macd_hist", "h"], ["stoch", "ta.stoch(close, high, low, 14)"],
    ["tsi", "ta.tsi(close, 13, 25)"], ["cci", "ta.cci(hlc3, 20)"], ["mfi", "ta.mfi(hlc3, 14)"], ["wpr", "ta.wpr(14)"],
    ["mom", "ta.mom(close, 10)"], ["change", "ta.change(close)"], ["change3", "ta.change(close, 3)"], ["roc", "ta.roc(close, 9)"],
  ],
  volatility: [
    ["tr_false", "ta.tr(false)"], ["tr_true", "ta.tr(true)"], ["atr", "ta.atr(14)"], ["bb_mid", "bbm"], ["bb_up", "bbu"], ["bb_lo", "bbl"],
    ["bbw", "ta.bbw(close, 20, 2)"], ["kc_mid", "kcm"], ["kc_up", "kcu"], ["kc_lo", "kcl"], ["kc_hl_up", "kcu2"], ["kcw", "ta.kcw(close, 20, 1.5)"],
    ["dmi_plus", "dp"], ["dmi_minus", "dm"], ["adx", "adx"], ["sar", "ta.sar(0.02, 0.02, 0.2)"], ["st", "st"], ["st_dir", "stdir"],
    ["vwap_close", "ta.vwap(close)"],
  ],
  series_vars: [["tr_false", "ta.tr"], ["obv", "ta.obv"], ["accdist", "ta.accdist"], ["vwap_var", "ta.vwap"], ["cum", "ta.cum(volume)"]],
  events: [
    ["crossover", "ta.crossover(close, e10) ? 1 : 0"], ["crossunder", "ta.crossunder(close, e10) ? 1 : 0"], ["cross", "ta.cross(close, e10) ? 1 : 0"],
    ["rising", "ta.rising(close, 3) ? 1 : 0"], ["falling", "ta.falling(close, 3) ? 1 : 0"], ["barssince", "ta.barssince(close > open)"],
    ["valuewhen", "ta.valuewhen(close > open, close, 1)"], ["pivothigh", "ta.pivothigh(high, 5, 5)"], ["pivotlow", "ta.pivotlow(5, 5)"],
  ],
  na_inputs: [
    ["sma_na", "ta.sma(nasrc, 5)"], ["ema_na", "ta.ema(nasrc, 5)"], ["rma_na", "ta.rma(nasrc, 5)"], ["highest_na", "ta.highest(nasrc, 5)"],
    ["cum_na", "ta.cum(nasrc)"], ["fixnan_na", "fixnan(nasrc)"], ["nz_na", "nz(nasrc, -1)"],
  ],
  math: [
    ["abs", "math.abs(close - open)"], ["ceil", "math.ceil(close)"], ["floor", "math.floor(close)"], ["round", "math.round(close)"],
    ["round2", "math.round(close / 7, 2)"], ["sign", "math.sign(close - open)"], ["sqrt", "math.sqrt(close)"], ["pow", "math.pow(close / 100, 3)"],
    ["exp", "math.exp(close / 100 - 1)"], ["log", "math.log(close)"], ["log10", "math.log10(close)"], ["sin", "math.sin(close)"],
    ["cos", "math.cos(close)"], ["tan", "math.tan(close)"], ["asin", "math.asin((close - open) / (high - low + 1))"],
    ["acos", "math.acos((close - open) / (high - low + 1))"], ["atan", "math.atan(close - open)"], ["max", "math.max(open, close, hl2)"],
    ["min", "math.min(open, close, hl2)"], ["avg", "math.avg(open, high, low)"], ["sum", "math.sum(close, 10)"],
    ["round_to_mintick", "math.round_to_mintick(close / 3)"], ["todegrees", "math.todegrees(close / 100)"], ["toradians", "math.toradians(close)"],
  ],
};

describe("Pine-lite built-ins match the independent reference", () => {
  for (const [group, entries] of Object.entries(GROUPS)) {
    it(`${group} (${entries.length} series)`, () => {
      const r = ok(plotScript(entries, PRELUDE));
      const got = plots(r);
      for (const [name] of entries) {
        expect(SERIES[name], `fixture has ${name}`).toBeDefined();
        expectSeries(got[name], SERIES[name], name);
      }
    });
  }

  it("covers every ta.* and math.* function and ta.* variable in the registry", () => {
    const text = Object.values(GROUPS).flat().map(([, e]) => e).concat(PRELUDE).join("\n");
    const used = new Set(text.match(/\b(?:ta|math)\.[a-z_0-9]+/g));
    const untested = [...FUNCTIONS.keys(), ...VARIABLES.keys()].filter((n) => /^(ta|math)\./.test(n) && !used.has(n));
    // math.random is deterministic but has no external reference; it is covered in api.test.ts.
    expect(untested).toEqual(["math.random"]);
  });
});
