/**
 * Value rules: v5 vs v6 na and division differences, strings, colours,
 * built-in variables, host info, math.random determinism.
 */
import { describe, expect, it } from "vitest";

import { formatNumber, toStr } from "@/lib/pine/runtime/builtins/core";

import { barsFrom, err, ok, output, plot, plots } from "./helpers";

const B = barsFrom([10, 12, 11, 15, 14]);
const run = (body: string, version = 6, opts = {}) => ok(`//@version=${version}\nindicator("v")\n${body}`, B, opts);

describe("v5 vs v6 differences (documented in the spec, section 3)", () => {
  it("comparison with na: v6 false, v5 na", () => {
    const body = 'plot(na(close[1] > 0) ? 1 : 0, "isna")\nplot(not (close[1] > 0) ? 1 : 0, "neg")';
    const v6 = plots(run(body, 6));
    const v5 = plots(run(body, 5));
    expect([v6.isna[0], v6.neg[0]]).toEqual([0, 1]);
    expect([v5.isna[0], v5.neg[0]]).toEqual([1, 0]);
  });

  it("dividing two const ints: v5 truncates, v6 gives a float; series ints divide as floats in both", () => {
    const body = 'len = 9\nplot(5 / 2, "lit")\nplot(len / 2, "var")\nplot(bar_index / 2, "series")';
    const v5 = plots(run(body, 5));
    const v6 = plots(run(body, 6));
    expect([v5.lit[0], v5.var[0], v5.series[1]]).toEqual([2, 4, 0.5]);
    expect([v6.lit[0], v6.var[0], v6.series[1]]).toEqual([2.5, 4.5, 0.5]);
  });

  it("missing //@version defaults to 6 with a warning", () => {
    const r = ok('indicator("x")\nplot(5 / 2)', B);
    expect(r.meta.version).toBe(6);
    expect(r.warnings[0]).toMatchObject({ kind: "warning", line: 1, col: 1 });
    expect(plots(r)[""][0]).toBe(2.5);
  });
});

describe("strings and formatting", () => {
  it("str.tostring, str.format subset, str.length, str.contains, concatenation", () => {
    const p = plots(
      run(
        [
          's = str.format("{0} and {1,number,#.##}", 1, 2.345)',
          'plot(str.contains(s, "1 and 2.35") ? 1 : 0, "fmt")',
          'plot(str.length(str.tostring(close) + "!"), "len")',
          'plot(str.contains(syminfo.ticker, "BTC") ? 1 : 0, "tick")',
        ].join("\n"),
        6,
        { host: { ticker: "BTCUSDT" } },
      ),
    );
    expect(p.fmt[0]).toBe(1);
    expect(p.len[0]).toBe(3);
    expect(p.tick[0]).toBe(1);
  });

  it("number formatting rules", () => {
    expect(toStr(null, 1)).toBe("1");
    expect(toStr(null, 1 / 3)).toBe("0.33333333");
    expect(toStr(null, null)).toBe("NaN");
    expect(toStr(null, true)).toBe("true");
    expect(formatNumber(null, 2.5, "0.00")).toBe("2.50");
    expect(formatNumber(null, 2.5, "#.##")).toBe("2.5");
    expect(formatNumber(null, 0.1234, "percent")).toBe("0.12%");
    expect(formatNumber(null, 1_250_000, "volume")).toBe("1.25M");
  });

  it("str.format rejects placeholders outside the subset at run time", () => {
    const e = err('//@version=6\nindicator("v")\ns = str.format("{0,date,yyyy}", time)\nplot(str.length(s))', B);
    expect(e).toMatchObject({ kind: "runtime", line: 3, col: 5 });
    expect(e.message).toContain("{0,date,yyyy}");
  });
});

describe("colours", () => {
  it("color.new, color.rgb, literals, per-bar series colours and from_gradient", () => {
    const r = run(
      [
        'plot(close, "p", color = close > open ? color.new(color.green, 50) : #FF000080)',
        'plot(close, "g", color = color.from_gradient(close, 10, 15, color.rgb(0, 0, 0), color.rgb(255, 255, 255)))',
        'plot(color.r(color.rgb(12, 34, 56, 20)), "r")',
        'plot(color.t(color.new(color.red, 30)), "t")',
      ].join("\n"),
    );
    expect(plot(r, "p").colors.slice(0, 3)).toEqual(["#FF000080", "#4CAF5080", "#FF000080"]);
    expect(plot(r, "g").colors[0]).toBe("#000000FF");
    expect(plot(r, "g").colors[3]).toBe("#FFFFFFFF");
    expect(plot(r, "r").values[0]).toBe(12);
    expect(plot(r, "t").values[0]).toBe(30);
  });
});

describe("built-in variables and host info", () => {
  it("bar_index, last_bar_index, barstate, time, time_close, timeframe, mintick", () => {
    const r = run(
      [
        'plot(bar_index, "bi")',
        'plot(last_bar_index, "lbi")',
        'plot(barstate.isfirst ? 1 : 0, "first")',
        'plot(barstate.islast ? 1 : 0, "last")',
        'plot(barstate.isconfirmed ? 1 : 0, "conf")',
        'plot(time_close - time, "dur")',
        'plot(timeframe.period == "60" ? 1 : 0, "tf")',
        'plot(syminfo.mintick, "mt")',
        'plot(ohlc4 - (open + high + low + close) / 4, "o4")',
      ].join("\n"),
      6,
      { host: { mintick: 0.5 } },
    );
    const p = plots(r);
    expect(p.bi).toEqual([0, 1, 2, 3, 4]);
    expect(p.lbi).toEqual([4, 4, 4, 4, 4]);
    expect(p.first).toEqual([1, 0, 0, 0, 0]);
    expect(p.last).toEqual([0, 0, 0, 0, 1]);
    expect(p.conf).toEqual([1, 1, 1, 1, 1]);
    expect(p.dur[0]).toBe(3_600_000);
    expect(p.tf[0]).toBe(1);
    expect(p.mt[0]).toBe(0.5);
    expect(p.o4[2]).toBe(0);
  });

  it("a host barIndexBase shifts bar_index and last_bar_index, nothing else (Aleph Edge sliding window)", () => {
    const body = 'plot(bar_index, "bi")\nplot(last_bar_index, "lbi")\nplot(barstate.isfirst ? 1 : 0, "first")\nplot(bar_index % 2, "par")';
    const p = plots(run(body, 6, { host: { barIndexBase: 1001 } }));
    expect(p.bi).toEqual([1001, 1002, 1003, 1004, 1005]);
    expect(p.lbi).toEqual([1005, 1005, 1005, 1005, 1005]);
    expect(p.first).toEqual([1, 0, 0, 0, 0]);
    expect(p.par).toEqual([1, 0, 1, 0, 1]);
  });

  it("math.random is deterministic per seed and per explicit seed", () => {
    const body = 'plot(math.random(), "a")\nplot(math.random(0, 10, 42), "b")';
    const a = plots(run(body, 6, { seed: 7 }));
    const b = plots(run(body, 6, { seed: 7 }));
    const c = plots(run(body, 6, { seed: 8 }));
    expect(a.a).toEqual(b.a);
    expect(a.a).not.toEqual(c.a);
    expect(a.b).toEqual(c.b);
    for (const v of a.b) expect(v as number).toBeGreaterThanOrEqual(0);
  });
});

describe("output descriptors", () => {
  it("plot styles, hline, fill, bgcolor, barcolor, plotshape/char/arrow, alertcondition", () => {
    const r = run(
      [
        'p1 = plot(close, "c", style = plot.style_histogram, linewidth = 2, offset = 1, histbase = 10)',
        'p2 = plot(open, "o", display = display.none)',
        "fill(p1, p2, color = color.new(color.blue, 80))",
        'h1 = hline(70, "up", linestyle = hline.style_dashed)',
        "h2 = hline(30)",
        "fill(h1, h2)",
        "bgcolor(close > open ? color.green : na)",
        "barcolor(color.red)",
        'plotshape(close > open, "s", shape.triangleup, location.belowbar, size = size.small, text = "B")',
        'plotchar(close, "ch", "x", location.absolute)',
        "plotarrow(close - open)",
        'alertcondition(close > 14, "hi", "close above 14")',
      ].join("\n"),
    );
    const p = output(r, "plot");
    expect(p).toMatchObject({ style: "histogram", linewidth: 2, offset: 1, histbase: 10, line: 3 });
    expect(output(r, "plot", 1).display).toBe("none");
    expect(output(r, "fill")).toMatchObject({ between: "plots", a: 0, b: 1 });
    expect(output(r, "hline")).toMatchObject({ price: 70, linestyle: "dashed", title: "up" });
    expect(output(r, "fill", 1)).toMatchObject({ between: "hlines" });
    expect(output(r, "bgcolor").colors.slice(0, 2)).toEqual([null, "#4CAF50FF"]);
    expect(output(r, "barcolor").colors[0]).toBe("#F23645FF");
    expect(output(r, "plotshape")).toMatchObject({ style: "triangleup", location: "belowbar", size: "small", text: "B" });
    expect(output(r, "plotshape").values.slice(0, 3)).toEqual([null, 1, null]);
    expect(output(r, "plotchar")).toMatchObject({ char: "x", location: "absolute" });
    expect(output(r, "plotchar").values[1]).toBe(12);
    expect(output(r, "plotarrow").values.slice(0, 3)).toEqual([null, 2, -1]);
    expect(output(r, "alertcondition")).toMatchObject({ title: "hi", message: "close above 14", fired: [3] });
  });

  it("plot.style_linebr (v1.1): a line whose na bars stay na, so the chart breaks it there", () => {
    const r = run('plot(close > 11 ? close : na, "br", style = plot.style_linebr)\nplot(close > 11 ? close : na, "ln", style = plot.style_line)');
    const br = output(r, "plot");
    expect(br.style).toBe("linebr");
    expect(br.values).toEqual([null, 12, null, 15, 14]);
    expect(br.colors[2]).toBeNull();
    expect(output(r, "plot", 1)).toMatchObject({ style: "line", values: [null, 12, null, 15, 14] });
  });
});
