/**
 * Pine-lite's stateful, per-call-site ta.* must agree with the chart's
 * vectorised lib/ta wherever both define the same indicator, so the two can
 * never drift apart. Compared on the fixture's 240 random-walk bars; the edge
 * segment is excluded only where lib/ta documents a different convention
 * (flat-range RSI = 50 in lib/ta vs TradingView's 100).
 *
 * Not cross-checked, by definition: Keltner (lib/ta uses ATR, ta.kc uses an
 * EMA of the true range) and MFI's first window (ta.mfi counts bar 0's flow).
 */
import { describe, expect, it } from "vitest";

import { emaOf, highest, lowest, rma, smaOf, trueRange, type Series } from "@/lib/ta/core";
import { adx, atr, cci, macd, mfi, obv, rsi, stochastic, williamsR } from "@/lib/ta/oscillators";
import { bollinger, parabolicSar, supertrend, vwap } from "@/lib/ta/overlays";

import { BARS, expectSeries, ok, plots, plotScript } from "./helpers";

const B = BARS.slice(0, 240);
const closes = B.map((b) => b.c);

const r = ok(
  plotScript(
    [
      ["sma", "ta.sma(close, 20)"], ["ema", "ta.ema(close, 21)"], ["rma", "ta.rma(close, 14)"], ["rsi", "ta.rsi(close, 14)"],
      ["macd", "m"], ["signal", "s"], ["hist", "h"], ["bbu", "bbu"], ["bbm", "bbm"], ["bbl", "bbl"], ["atr", "ta.atr(14)"],
      ["pdi", "dp"], ["mdi", "dm"], ["adx", "adx"], ["cci", "ta.cci(hlc3, 20)"], ["wpr", "ta.wpr(14)"], ["obv", "ta.obv"],
      ["vwap", "ta.vwap"], ["st", "st"], ["stdir", "stdir"], ["sar", "ta.sar(0.02, 0.02, 0.2)"], ["stoch", "ta.stoch(close, high, low, 14)"],
      ["hh", "ta.highest(high, 14)"], ["ll", "ta.lowest(low, 14)"], ["tr", "ta.tr(true)"], ["mfi", "ta.mfi(hlc3, 14)"],
    ],
    ["[m, s, h] = ta.macd(close, 12, 26, 9)", "[bbm, bbu, bbl] = ta.bb(close, 20, 2)", "[dp, dm, adx] = ta.dmi(14, 14)", "[st, stdir] = ta.supertrend(3, 10)"],
  ),
  B,
);
const p = plots(r);

describe("Pine-lite ta.* agrees with lib/ta on the same bars", () => {
  it("moving averages", () => {
    expectSeries(p.sma, smaOf(closes, 20), "sma");
    expectSeries(p.ema, emaOf(closes, 21), "ema");
    expectSeries(p.rma, rma(closes, 14), "rma");
  });

  it("RSI and MACD", () => {
    expectSeries(p.rsi, rsi(B, 14), "rsi");
    const m = macd(B, 12, 26, 9);
    expectSeries(p.macd, m.macd, "macd");
    expectSeries(p.signal, m.signal, "signal");
    expectSeries(p.hist, m.histogram, "hist");
  });

  it("Bollinger Bands, ATR, true range, highest/lowest", () => {
    const bb = bollinger(B, 20, 2);
    expectSeries(p.bbu, bb.upper, "bb upper");
    expectSeries(p.bbm, bb.middle, "bb middle");
    expectSeries(p.bbl, bb.lower, "bb lower");
    expectSeries(p.atr, atr(B, 14), "atr");
    expectSeries(p.tr, trueRange(B), "tr");
    expectSeries(p.hh, highest(B.map((b) => b.h), 14), "highest");
    expectSeries(p.ll, lowest(B.map((b) => b.l), 14), "lowest");
  });

  it("DMI / ADX, CCI, Williams %R, stochastic %K", () => {
    const a = adx(B, 14, 14);
    expectSeries(p.pdi, a.plusDi, "+DI");
    expectSeries(p.mdi, a.minusDi, "-DI");
    expectSeries(p.adx, a.adx, "adx");
    expectSeries(p.cci, cci(B, 20), "cci");
    expectSeries(p.wpr, williamsR(B, 14), "wpr");
    expectSeries(p.stoch, stochastic(B, 14, 1, 1).k, "stoch");
  });

  it("OBV, VWAP (UTC day), Parabolic SAR", () => {
    expectSeries(p.obv, obv(B), "obv");
    expectSeries(p.vwap, vwap(B, "day"), "vwap");
    expectSeries(p.sar, parabolicSar(B, 0.02, 0.02, 0.2), "sar");
  });

  it("SuperTrend (lib/ta trend 1 = up = Pine direction -1)", () => {
    const st = supertrend(B, 10, 3);
    const line: Series = st.up.map((u, i) => u ?? st.down[i]);
    expectSeries(p.st, line, "supertrend");
    expectSeries(p.stdir, st.trend.map((t) => (t === null ? null : -t)), "direction");
  });

  it("MFI once the window no longer contains bar 0", () => {
    const lib = mfi(B, 14);
    for (let i = 15; i < B.length; i++) if (lib[i] !== null && lib[i] !== 100) expect(p.mfi[i]).toBeCloseTo(lib[i] as number, 9);
  });
});
