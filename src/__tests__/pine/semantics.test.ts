/**
 * Pine execution-model semantics: var vs per-bar init, :=/compound
 * assignment, history, if/switch as expressions, loops, user functions,
 * per-call-site and per-instance state, skipped conditional calls.
 */
import { describe, expect, it } from "vitest";

import { barsFrom, ok, plots } from "./helpers";

const CLOSES = [10, 12, 11, 15, 14, 18, 17, 20, 19, 25];
const B = barsFrom(CLOSES);
const run = (body: string, version = 6) => plots(ok(`//@version=${version}\nindicator("s")\n${body}`, B));

describe("declarations and assignment", () => {
  it("var initialises once; a plain declaration re-initialises every bar", () => {
    const p = run('var int count = 0\ncount += 1\nx = 0\nx += 1\nplot(count, "count")\nplot(x, "x")');
    expect(p.count).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    expect(p.x).toEqual(new Array(10).fill(1));
  });

  it(":= and every compound operator", () => {
    const p = run('a = 10\na := a * 2\na -= 5\na /= 3\na %= 4\nb = 1.5\nb *= 4\nplot(a, "a")\nplot(b, "b")');
    expect(p.a[0]).toBe(1);
    expect(p.b[0]).toBe(6);
  });

  it("typed declarations, na, and v6 bool na -> false", () => {
    const p = run('float f = na\nint i = 3\nbool b = na\nplot(na(f) ? 1 : 0, "f")\nplot(i, "i")\nplot(b ? 1 : 0, "b")\nplot(na(b) ? 1 : 0, "bna")');
    expect(p.f[0]).toBe(1);
    expect(p.i[0]).toBe(3);
    expect(p.bna[0]).toBe(0);
  });
});

describe("history operator", () => {
  it("reads past bars and is na beyond history", () => {
    const p = run('plot(close[2], "c2")\ny = close * 2\nplot(y[1], "y1")\nplot(bar_index[3], "b3")');
    expect(p.c2.slice(0, 4)).toEqual([null, null, 10, 12]);
    expect(p.y1.slice(0, 3)).toEqual([null, 20, 24]);
    expect(p.b3.slice(0, 5)).toEqual([null, null, null, 0, 1]);
  });

  it("var history, dynamic offsets and expression history", () => {
    const p = run('var c = 0\nc += 1\nplot(c[1], "c1")\nplot(close[bar_index % 3], "dyn")\nplot((close - open)[1], "e1")\nplot(ta.sma(close, 2)[1], "s1")');
    expect(p.c1.slice(0, 3)).toEqual([null, 1, 2]);
    expect(p.dyn.slice(0, 6)).toEqual([10, 10, 10, 15, 15, 15]);
    expect(p.e1.slice(0, 3)).toEqual([null, 0, 2]);
    expect(p.s1.slice(0, 4)).toEqual([null, null, 11, 11.5]);
  });

  it("history chains: x[1][1] reads two bars back", () => {
    const p = run('d = close - close[1]\nplot(d[1][1], "chain")');
    expect(p.chain.slice(0, 4)).toEqual([null, null, null, 2]);
  });

  it("a variable's history inside a block records only executions of that block", () => {
    const p = run('r = 0.0\nif bar_index % 2 == 0\n    v = close\n    r := nz(v[1], -1)\nplot(r, "r")');
    expect(p.r.slice(0, 6)).toEqual([-1, 0, 10, 0, 11, 0]);
  });
});

describe("control flow", () => {
  it("if / else if / else as an expression, and if without else gives na", () => {
    const p = run('v = if close > 15\n    2\nelse if close > 11\n    1\nelse\n    0\nw = if close > 100\n    1\nplot(v, "v")\nplot(w, "w")');
    expect(p.v.slice(0, 4)).toEqual([0, 1, 0, 1]);
    expect(p.w[0]).toBeNull();
  });

  it("switch with and without a subject, default case", () => {
    const p = run(
      'm = "b"\nk = switch m\n    "a" => 1\n    "b" => 2\n    => 3\nj = switch\n    close > 15 => 10\n    close > 11 =>\n        x = 5\n        x * 4\n    => -1\nplot(k, "k")\nplot(j, "j")',
    );
    expect(p.k[0]).toBe(2);
    expect(p.j.slice(0, 4)).toEqual([-1, 20, -1, 20]);
  });

  it("for with step and reverse range, break and continue, while", () => {
    const p = run(
      [
        "s = 0",
        "for i = 0 to 10 by 2",
        "    s += i",
        "r = 0",
        "for i = 5 to 1",
        "    if i == 2",
        "        break",
        "    if i == 4",
        "        continue",
        "    r += i",
        "n = 0",
        "k = 1",
        "while k < 100",
        "    k *= 3",
        "    n += 1",
        "t = for i = 1 to 3",
        "    i * 10",
        'plot(s, "s")',
        'plot(r, "r")',
        'plot(n, "n")',
        'plot(t, "t")',
      ].join("\n"),
    );
    expect(p.s[0]).toBe(30);
    expect(p.r[0]).toBe(8);
    expect(p.n[0]).toBe(5);
    expect(p.t[0]).toBe(30);
  });

  it("ternary and na propagation", () => {
    const p = run('plot(close > 12 ? 1 : 0, "t")\nplot(close + na, "n")\nplot(close / 0, "z")\nplot(nz(close[1], -1), "nz")');
    expect(p.t.slice(0, 3)).toEqual([0, 0, 0]);
    expect(p.n[3]).toBeNull();
    expect(p.z[3]).toBeNull();
    expect(p.nz.slice(0, 2)).toEqual([-1, 10]);
  });
});

describe("user functions", () => {
  it("defaults, named arguments, multi-line bodies and tuple returns", () => {
    const p = run(
      'f(a, b = 2) => a * b\ng(x) =>\n    y = x + 1\n    [y, y * 2]\n[u, v] = g(close)\nplot(f(3), "f1")\nplot(f(3, b = 5), "f2")\nplot(u, "u")\nplot(v, "v")',
    );
    expect(p.f1[0]).toBe(6);
    expect(p.f2[0]).toBe(15);
    expect(p.u[0]).toBe(11);
    expect(p.v[0]).toBe(22);
  });

  it("each call site has its own state, also inside functions", () => {
    const p = run('f(x) => ta.sma(x, 2)\na = f(close)\nb = f(open)\nplot(a, "a")\nplot(b, "b")\nplot(ta.ema(close, 3), "e1")\nplot(ta.ema(close, 3), "e2")');
    expect(p.a.slice(0, 3)).toEqual([null, 11, 11.5]);
    expect(p.b.slice(0, 3)).toEqual([null, 10, 11]);
    expect(p.e1).toEqual(p.e2);
  });

  it("var inside a function is per call site", () => {
    const p = run('counter() =>\n    var n = 0\n    n += 1\n    n\na = counter()\nb = bar_index % 2 == 0 ? counter() : -1\nplot(a, "a")\nplot(b, "b")');
    expect(p.a.slice(0, 4)).toEqual([1, 2, 3, 4]);
    expect(p.b.slice(0, 5)).toEqual([1, -1, 2, -1, 3]);
  });

  it("a function's history refers to its previous call", () => {
    const p = run('prev(x) => x[1]\nr = bar_index % 2 == 0 ? prev(close) : -1\nplot(r, "r")');
    expect(p.r.slice(0, 5)).toEqual([null, -1, 10, -1, 11]);
  });
});

describe("conditional calls", () => {
  it("a skipped stateful call does not advance its state, and the checker warns", () => {
    const src = '//@version=6\nindicator("c")\ns = 0.0\nif bar_index % 2 == 0\n    s := ta.sma(close, 2)\nplot(s, "s")';
    const r = ok(src, B);
    expect(plots(r).s.slice(0, 5)).toEqual([null, 0, 10.5, 0, 12.5]);
    expect(r.warnings.some((w) => w.kind === "warning" && w.line === 5 && /ta\.sma.*each calculation/.test(w.message))).toBe(true);
  });

  it("v6 and/or are lazy; v5 evaluates both sides", () => {
    const body = 't = bar_index % 2 == 0 and ta.cum(1) % 2 == 0\nplot(t ? 1 : 0, "t")';
    expect(run(body, 6).t.slice(0, 7)).toEqual([0, 0, 1, 0, 0, 0, 1]);
    expect(run(body, 5).t.slice(0, 7)).toEqual([0, 0, 0, 0, 0, 0, 0]);
  });
});
