/**
 * Golden scripts (goldenScripts.ts) with expected outputs from the
 * independent reference fixture, lib/ta, or computations written here.
 */
import { describe, expect, it } from "vitest";

import { emaOf, smaOf, type Series } from "@/lib/ta/core";

import * as G from "./goldenScripts";
import { BARS, expectSeries, ok, output, plot, plots, SERIES } from "./helpers";

const N = BARS.length;
const C = BARS.map((b) => b.c);
const R = BARS.slice(0, 240);

describe("golden scripts", () => {
  it("RSI with bands: values, hlines, fill, overbought background, inputs", () => {
    const r = ok(G.RSI_BANDS);
    expectSeries(plot(r, "RSI").values, SERIES.rsi, "rsi");
    expect(output(r, "hline")).toMatchObject({ price: 70, title: "Upper Band" });
    expect(output(r, "hline", 1).price).toBe(30);
    expect(output(r, "fill")).toMatchObject({ between: "hlines", a: 1, b: 2 });
    const bg = output(r, "bgcolor").colors;
    SERIES.rsi.forEach((v, i) => expect(bg[i] !== null, `bg[${i}]`).toBe(v !== null && v > 70));
    expect(r.meta).toMatchObject({ title: "RSI with bands", shorttitle: "RSI", overlay: false, precision: 2, version: 6 });
    expect(r.inputs.map((i) => [i.kind, i.title, i.defval])).toEqual([
      ["int", "RSI Length", 14], ["source", "Source", "close"], ["int", "Overbought", 70], ["int", "Oversold", 30],
    ]);
  });

  it("MACD histogram colouring (v5, generic input())", () => {
    const r = ok(G.MACD_COLORS);
    const hist = plot(r, "Histogram");
    expectSeries(hist.values, SERIES.macd_hist, "hist");
    expectSeries(plot(r, "MACD").values, SERIES.macd, "macd");
    expectSeries(plot(r, "Signal").values, SERIES.macd_signal, "signal");
    expect(hist.style).toBe("columns");
    const h = SERIES.macd_hist;
    for (let i = 1; i < N; i++) {
      const v = h[i];
      if (v === null) continue;
      const rising = h[i - 1] !== null && (h[i - 1] as number) < v;
      const want = v >= 0 ? (rising ? "#26A69AFF" : "#B2DFDBFF") : rising ? "#FFCDD2FF" : "#FF5252FF";
      expect(hist.colors[i], `colour[${i}]`).toBe(want);
    }
    expect(r.inputs.map((i) => i.kind)).toEqual(["int", "int", "source", "int", "color", "color", "color", "color"]);
  });

  it("SuperTrend: up/down lines, fills, trend-change alert", () => {
    const r = ok(G.SUPERTREND);
    const dir = SERIES.st_dir;
    const want = (upSide: boolean) => SERIES.st.map((v, i) => (i === 0 || dir[i] === null ? null : (dir[i] as number) < 0 === upSide ? v : null));
    expectSeries(plot(r, "Up Trend").values, want(true), "up");
    expectSeries(plot(r, "Down Trend").values, want(false), "down");
    const flips = SERIES.st_dir.flatMap((d, i) => (i > 0 && d !== null && SERIES.st_dir[i - 1] !== null && (SERIES.st_dir[i - 1] as number) > d ? [i] : []));
    expect(output(r, "alertcondition").fired).toEqual(flips);
    expect(flips.length).toBeGreaterThan(0);
    expect(r.outputs.filter((o) => o.kind === "fill")).toHaveLength(2);
    expect(r.meta.overlay).toBe(true);
  });

  it("Bollinger squeeze: width and squeeze markers", () => {
    const r = ok(G.BB_SQUEEZE);
    expectSeries(plot(r, "Width").values, SERIES.bbw, "bbw");
    const w = SERIES.bbw;
    const marks = output(r, "plotshape").values;
    // ta.lowest ignores na: the window is the last 50 non-na widths.
    const seen: number[] = [];
    for (let i = 0; i < N; i++) {
      if (w[i] !== null) seen.push(w[i] as number);
      const min = seen.length >= 50 ? Math.min(...seen.slice(-50)) : null;
      const sq = w[i] !== null && min !== null && (w[i] as number) <= min;
      expect(marks[i], `squeeze[${i}]`).toBe(sq ? 1 : null);
    }
    expect(marks.filter((m) => m === 1).length).toBeGreaterThan(0);
  });

  it("EMA cross with plotshape signals and an alert", () => {
    const r = ok(G.EMA_CROSS);
    const fast = emaOf(C, 9);
    const slow = emaOf(C, 21);
    expectSeries(plot(r, "Fast").values, fast, "fast");
    const crossUp = (i: number) => i > 0 && [fast[i], slow[i], fast[i - 1], slow[i - 1]].every((v) => v !== null) && (fast[i] as number) > (slow[i] as number) && (fast[i - 1] as number) <= (slow[i - 1] as number);
    const crossDn = (i: number) => i > 0 && [fast[i], slow[i], fast[i - 1], slow[i - 1]].every((v) => v !== null) && (fast[i] as number) < (slow[i] as number) && (fast[i - 1] as number) >= (slow[i - 1] as number);
    const buys = output(r, "plotshape", 0).values;
    const sells = output(r, "plotshape", 1).values;
    for (let i = 0; i < N; i++) {
      expect(buys[i], `buy[${i}]`).toBe(crossUp(i) ? 1 : null);
      expect(sells[i], `sell[${i}]`).toBe(crossDn(i) ? 1 : null);
    }
    expect(output(r, "alertcondition").fired).toEqual([...Array(N).keys()].filter((i) => crossUp(i) || crossDn(i)));
  });

  it("Pivots: markers on the confirming bar, offset back by rightbars", () => {
    const r = ok(G.PIVOTS);
    const ph = output(r, "plotshape", 0);
    expect(ph).toMatchObject({ offset: -5, style: "labeldown", text: "H" });
    expect(ph.values).toEqual(SERIES.pivothigh.map((v) => (v === null ? null : 1)));
    expect(output(r, "plotshape", 1).values).toEqual(SERIES.pivotlow.map((v) => (v === null ? null : 1)));
  });

  it("ADX / DMI", () => {
    const p = plots(ok(G.DMI));
    expectSeries(p.ADX, SERIES.adx, "adx");
    expectSeries(p["+DI"], SERIES.dmi_plus, "+di");
    expectSeries(p["-DI"], SERIES.dmi_minus, "-di");
  });

  it("multi-line user function (hand-written RSI) equals ta.rsi where the loss average is non-zero", () => {
    const v = plot(ok(G.USER_RSI, R), "RSI").values;
    expectSeries(v, SERIES.rsi.slice(0, 240), "user rsi");
  });

  it("function returning a tuple", () => {
    const p = plots(ok(G.TUPLE_BANDS));
    expectSeries(p.Basis, SERIES.bb_mid, "basis");
    expectSeries(p.Upper, SERIES.bb_up, "upper");
    expectSeries(p.Lower, SERIES.bb_lo, "lower");
  });

  it("switch on an input string, with overrides", () => {
    const want: Record<string, Series> = { SMA: SERIES.sma, EMA: SERIES.ema, RMA: SERIES.rma };
    for (const [t, s] of Object.entries(want)) expectSeries(plot(ok(G.MA_SWITCH, BARS, { inputs: { 0: t } }), "MA").values, s, t);
    const wma = plot(ok(G.MA_SWITCH, BARS, { inputs: { 0: "WMA", 1: 9 } }), "MA").values;
    expectSeries(wma, SERIES.wma, "WMA");
  });

  it("for loop SMA equals ta.sma", () => {
    expectSeries(plot(ok(G.FOR_SMA), "Loop SMA").values, SERIES.sma, "loop sma");
  });

  it("while loop streak", () => {
    const v = plot(ok(G.WHILE_STREAK), "Streak").values;
    for (let i = 0; i < N; i++) {
      let n = 0;
      while (n < 50 && i - n - 1 >= 0 && C[i - n] > C[i - n - 1]) n++;
      expect(v[i], `streak[${i}]`).toBe(n);
    }
  });

  it("var counters", () => {
    const p = plots(ok(G.VAR_COUNTERS));
    let ups = 0;
    let hi = -Infinity;
    BARS.forEach((b, i) => {
      if (b.c > b.o) ups++;
      hi = Math.max(hi, b.h);
      expect(p.Ups[i]).toBe(ups);
      expect(p["Running high"][i]).toBe(hi);
    });
  });

  it("history operator chains", () => {
    const p = plots(ok(G.HISTORY_CHAINS));
    const sma10 = smaOf(C, 10);
    for (let i = 0; i < N; i++) {
      const mm = i >= 8 ? C[i] - C[i - 5] - (C[i - 3] - C[i - 8]) : null;
      if (mm === null) expect(p.MoM[i]).toBeNull();
      else expect(p.MoM[i]).toBeCloseTo(mm, 9);
      if (i < 2 || sma10[i - 2] === null) expect(p["Lagged SMA"][i]).toBeNull();
      else expect(p["Lagged SMA"][i]).toBeCloseTo(sma10[i - 2] as number, 9);
      expect(p["close[3]"][i]).toBe(i >= 3 ? C[i - 3] : null);
    }
  });

  it("na handling: fixnan, nz, na(), na arithmetic", () => {
    const p = plots(ok(G.NA_HANDLING));
    expectSeries(p.Fixed, SERIES.fixnan_na, "fixnan");
    expectSeries(p.Zeroed, SERIES.nz_na, "nz");
    expect(p.Gaps[N - 1]).toBe([...Array(N).keys()].filter((i) => i % 7 === 3).length);
    expect(p.Doubled[3]).toBeNull();
    expect(p.Doubled[4]).toBe(C[4] * 2);
  });

  it("Stochastic RSI (TradingView built-in script) equals lib/ta stochRsi", async () => {
    const { stochRsi } = await import("@/lib/ta/oscillators");
    const r = ok(G.STOCH_RSI, R);
    const lib = stochRsi(R, 14, 14, 3, 3);
    expectSeries(plot(r, "K").values, lib.k, "k");
    expectSeries(plot(r, "D").values, lib.d, "d");
    expect(r.outputs.filter((o) => o.kind === "hline").map((o) => (o.kind === "hline" ? o.price : 0))).toEqual([80, 20]);
  });
});
