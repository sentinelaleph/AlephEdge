/**
 * Every feature Pine-lite v1 rejects (spec sections 1 and 4) fails with a
 * named error at the right line/column; plus the checker's own errors.
 * Scripts below start their own code on line 3 (after the H header).
 */
import { describe, expect, it } from "vitest";

import { formatPineError } from "@/lib/pine";

import { compileErr, err } from "./helpers";

const H = '//@version=6\nindicator("e")\n';

type Case = [label: string, source: string, kind: string, line: number, col: number, contains: string];

const REJECTED: Case[] = [
  ["strategy() declaration", '//@version=6\nstrategy("s")', "unsupported", 2, 1, "strategy is not supported in Pine-lite v1"],
  ["strategy.* calls", H + 'x = 1\n    \nstrategy.entry("L", strategy.long)', "unsupported", 5, 1, "strategy.entry is not supported"],
  ["request.security", H + 'd = request.security(syminfo.ticker, "D", close)', "unsupported", 3, 5, "request.security is not supported"],
  ["request.* (financial)", H + 'f = request.financial("X", "Y", "FQ")', "unsupported", 3, 5, "request.financial"],
  ["library", '//@version=6\nlibrary("lib")', "unsupported", 2, 1, "library is not supported"],
  ["import", H + "import user/lib/1 as l", "unsupported", 3, 1, "import is not supported"],
  ["export", H + "export f(x) => x", "unsupported", 3, 1, "export is not supported"],
  ["user-defined types (type)", H + "type Point\n    float x", "unsupported", 3, 1, "User-defined types (type)"],
  ["user-defined type in a declaration", H + "Point p = na", "unsupported", 3, 1, "User-defined types (Point)"],
  ["methods", H + "method twice(float x) => x * 2", "unsupported", 3, 1, "Methods (method)"],
  ["maps", H + "m = map.new<string, float>()", "unsupported", 3, 5, "map.new<...> (collections)"],
  ["matrices", H + "m = matrix.new<float>(2, 2)", "unsupported", 3, 5, "matrix.new<...> (collections)"],
  ["arrays (call)", H + "a = array.from(1, 2, 3)", "unsupported", 3, 5, "array.from is not supported"],
  ["arrays (type)", H + "float[] a = na", "unsupported", 3, 1, "float[]: arrays"],
  ["arrays (generic type)", H + "array<float> a = na", "unsupported", 3, 1, "array: arrays"],
  ["for...in", H + "for x in arr\n    y = x", "unsupported", 3, 1, "for...in loops (arrays)"],
  ["table.*", H + "t = table.new(position.top_right, 2, 2)", "unsupported", 3, 5, "table.new is not supported"],
  ["line.*", H + "l = line.new(bar_index, low, bar_index, high)", "unsupported", 3, 5, "line.new is not supported"],
  ["label.*", H + 'label.new(bar_index, high, "x")', "unsupported", 3, 1, "label.new is not supported"],
  ["box.*", H + "b = box.new(0, 1, 2, 0)", "unsupported", 3, 5, "box.new is not supported"],
  ["polyline.*", H + "p = polyline.new(pts)", "unsupported", 3, 5, "polyline.new is not supported"],
  ["drawing type", H + "var line l = na", "unsupported", 3, 5, "line: drawing objects (line)"],
  ["varip", H + "varip int n = 0", "unsupported", 3, 1, "varip is not supported"],
  ["barstate.isrealtime", H + "plot(barstate.isrealtime ? 1 : 0)", "unsupported", 3, 6, "barstate.isrealtime is not supported"],
  ["timeframe.* other than period", H + "plot(timeframe.multiplier)", "unsupported", 3, 6, "timeframe.multiplier is not supported"],
  ["syminfo.* other than ticker/mintick", H + 'plot(syminfo.tickerid == "" ? 1 : 0)', "unsupported", 3, 6, "syminfo.tickerid is not supported"],
  ["Pine v4", '//@version=4\nstudy("old")\nplot(close)', "unsupported", 1, 1, "Pine v4 is not supported"],
  ["study() without a version", 'study("old")\nplot(close)', "unsupported", 1, 1, "study() is Pine v4"],
  ["ta.mode (not in the ta list)", H + "\nplot(ta.mode(close, 10))", "unsupported", 4, 6, "ta.mode is not supported in Pine-lite v1"],
  ["ta.cmo", H + "plot(ta.cmo(close, 9))", "unsupported", 3, 6, "ta.cmo is not supported"],
  ["ta.rci", H + "plot(ta.rci(close, 9))", "unsupported", 3, 6, "ta.rci is not supported"],
  ["str.* outside the subset", H + 's = str.replace("a", "a", "b")', "unsupported", 3, 5, "str.replace is not supported"],
  ["input.symbol", H + 's = input.symbol("BTCUSDT")', "unsupported", 3, 5, "input.symbol is not supported"],
  ["input.session", H + 's = input.session("0930-1600")', "unsupported", 3, 5, "input.session is not supported"],
  ["input.price", H + "p = input.price(100)", "unsupported", 3, 5, "input.price is not supported"],
  ["input.time", H + "t = input.time(0)", "unsupported", 3, 5, "input.time is not supported"],
  ["plot style outside the list", H + "plot(close, style = plot.style_steplinebr)", "unsupported", 3, 21, "plot.style_steplinebr is not supported"],
  ["plotcandle", H + "plotcandle(open, high, low, close)", "unsupported", 3, 1, "plotcandle is not supported"],
  ["plotbar", H + "plotbar(open, high, low, close)", "unsupported", 3, 1, "plotbar is not supported"],
  ["alert()", H + 'alert("x")', "unsupported", 3, 1, "alert is not supported"],
  ["gradient fill", H + "a = plot(close)\nb = plot(open)\nfill(a, b, top_value = 1, bottom_value = 0, top_color = color.red, bottom_color = color.blue)", "unsupported", 5, 12, "fill(top_value=...)"],
  ["plot(force_overlay)", H + "plot(close, force_overlay = true)", "unsupported", 3, 13, "plot(force_overlay=...)"],
  ["indicator(timeframe=...)", '//@version=6\nindicator("x", timeframe = "D")\nplot(close)', "unsupported", 2, 28, "indicator(timeframe=...)"],
  ["time() with arguments", H + 'plot(time("D"))', "unsupported", 3, 6, "time() with arguments"],
  ["dayofweek", H + "plot(dayofweek)", "unsupported", 3, 6, "dayofweek is not supported"],
];

describe("rejected features", () => {
  for (const [label, src, kind, line, col, contains] of REJECTED) {
    it(label, () => {
      const e = compileErr(src);
      expect(e.kind, JSON.stringify(e)).toBe(kind);
      expect([e.line, e.col], e.message).toEqual([line, col]);
      expect(e.message).toContain(contains);
    });
  }

  it("input.timeframe other than the chart's fails at run time", () => {
    const e = err(H + 'tf = input.timeframe("240")\nplot(close)', undefined, { host: { timeframe: "60" } });
    expect(e).toMatchObject({ kind: "unsupported", line: 3, col: 6 });
    expect(e.message).toContain('only the chart timeframe is supported in Pine-lite v1 (got "240")');
  });

  it("formats messages as 'line L:C — message' and adds hints", () => {
    const e = compileErr(H + "\nplot(ta.mode(close, 10))");
    expect(formatPineError(e)).toBe("line 4:6 — ta.mode is not supported in Pine-lite v1");
    expect(compileErr(H + 'd = request.security("X", "D", close)').hint).toContain("chart's own candles");
  });
});

const CHECKER: Case[] = [
  ["missing indicator()", "//@version=6\nplot(close)", "type", 1, 1, "must declare indicator"],
  ["indicator() twice", H + 'indicator("again")', "type", 3, 1, "only be declared once"],
  ["undeclared identifier", H + "plot(foo)", "name", 3, 6, "Undeclared identifier 'foo'"],
  ["unknown function", H + "plot(foo(1))", "name", 3, 6, "Could not find function or function reference 'foo'"],
  [":= before declaration", H + "x := 1", "name", 3, 1, "Undeclared identifier 'x'"],
  ["redeclaration in the same scope", H + "x = 1\nx = 2", "name", 4, 1, "already declared"],
  ["modifying a built-in", H + "close := 1", "type", 3, 1, "Cannot modify the built-in 'close'"],
  ["modifying a global in a function", H + "g = 0\nf() =>\n    g := 1\n    g", "type", 5, 5, "Cannot modify global variable 'g' in a function"],
  ["recursion", H + "f(n) => n <= 1 ? 1 : n * f(n - 1)", "type", 3, 26, "recursion is not supported"],
  ["function in local scope", H + "if close > 0\n    f(x) => x", "type", 4, 5, "only be declared in the global scope"],
  ["plot in local scope", H + "if close > 0\n    plot(close)", "type", 4, 5, "Cannot use 'plot' in local scope"],
  ["plot inside an expression", H + "x = close > 0 ? plot(close) : na", "type", 3, 17, "Cannot use 'plot' in local scope"],
  ["input in local scope", H + "if close > 0\n    x = input.int(5)", "type", 4, 9, "inputs must be declared globally"],
  ["tuple outside a function", H + "x = [1, 2]", "type", 3, 5, "Tuples like [a, b]"],
  ["tuple size mismatch", H + "[a, b] = ta.macd(close, 12, 26, 9)", "type", 3, 1, "returns 3 values, but 2 names"],
  ["break outside a loop", H + "break", "syntax", 3, 1, "'break' outside of a loop"],
  ["series passed to a simple parameter", H + "len = bar_index + 1\nplot(ta.ema(close, len))", "type", 4, 20, "argument 'length' must be a simple value"],
  ["string + number", H + 's = "a" + 1', "type", 3, 9, "Cannot apply '+' to a string and a number"],
  ["negative history offset", H + "plot(close[-1])", "type", 3, 12, "History offsets must not be negative"],
  ["unknown named argument", H + "plot(close, colour = color.red)", "type", 3, 13, "plot: unknown argument 'colour'"],
  ["missing required argument", H + "plot(ta.sma(close))", "type", 3, 6, "ta.sma: missing argument 'length'"],
  ["too many arguments", H + "plot(ta.sma(close, 1, 2))", "type", 3, 23, "ta.sma: too many arguments"],
  ["non-constant input default", H + "x = input.int(bar_index)", "type", 3, 15, "must be a constant"],
  ["input default not in options", H + 's = input.string("C", options = ["A", "B"])', "type", 3, 18, "must be one of the options"],
  ["calling a function without ()", H + "plot(ta.sma)", "type", 3, 6, "'ta.sma' is a function"],
];

describe("checker errors", () => {
  for (const [label, src, kind, line, col, contains] of CHECKER) {
    it(label, () => {
      const e = compileErr(src);
      expect(e.kind, JSON.stringify(e)).toBe(kind);
      expect([e.line, e.col], e.message).toEqual([line, col]);
      expect(e.message).toContain(contains);
    });
  }
});

const SYNTAX: Case[] = [
  ["unterminated string", H + 'x = "abc', "syntax", 3, 5, "Unterminated string"],
  ["bad colour literal", H + "c = #12345", "syntax", 3, 5, "#RRGGBB"],
  ["unexpected character", H + "x = 1 @ 2", "syntax", 3, 7, "Unexpected character '@'"],
  ["inconsistent dedent", H + "if close > 0\n        x = 1\n    y = 2", "syntax", 5, 5, "Indentation does not match"],
  ["missing block", H + "if close > 0\nx = 1", "syntax", 4, 1, "Expected an indented block"],
  ["unbalanced parenthesis", H + "plot((close)", "syntax", 3, 13, "Expected ')'"],
  ["stray indentation", H + "    x = 1", "syntax", 3, 5, "Unexpected indentation"],
  ["positional after named", H + "plot(title = \"a\", close)", "syntax", 3, 19, "Positional arguments cannot follow named arguments"],
];

describe("syntax errors", () => {
  for (const [label, src, kind, line, col, contains] of SYNTAX) {
    it(label, () => {
      const e = compileErr(src);
      expect(e.kind, JSON.stringify(e)).toBe(kind);
      expect([e.line, e.col], e.message).toEqual([line, col]);
      expect(e.message).toContain(contains);
    });
  }
});
