/**
 * Lexer and parser: layout tokens, Pine line continuation, literals, AST
 * positions, and the round-trip property over every golden script.
 */
import { describe, expect, it } from "vitest";

import { lex } from "@/lib/pine/lexer";
import { parse } from "@/lib/pine/parser";

import * as G from "./goldenScripts";
import { printProgram, shape } from "./printer";

const kinds = (src: string) => lex(src).tokens.map((t) => (t.t === "ident" || t.t === "op" || t.t === "num" ? t.v : t.t));

describe("lexer", () => {
  it("emits INDENT / DEDENT for 4-space and tab blocks", () => {
    expect(kinds("if a\n    b\nc")).toEqual(["if", "a", "nl", "indent", "b", "nl", "dedent", "c", "nl", "eof"]);
    expect(kinds("if a\n\tb\n\t\tc")).toEqual(["if", "a", "nl", "indent", "b", "nl", "indent", "c", "nl", "dedent", "dedent", "eof"]);
  });

  it("joins wrapped lines: non-multiple-of-4 indent, open brackets, trailing operators", () => {
    expect(kinds("x = a +\n  b")).toEqual(["x", "=", "a", "+", "b", "nl", "eof"]);
    expect(kinds("x = f(a,\nb,\n        c)")).toEqual(["x", "=", "f", "(", "a", ",", "b", ",", "c", ")", "nl", "eof"]);
    expect(kinds("x = a\n     + b")).toEqual(["x", "=", "a", "+", "b", "nl", "eof"]);
    expect(kinds("x = a and\n    b")).toEqual(["x", "=", "a", "and", "b", "nl", "eof"]);
  });

  it("skips comments and blank lines, and reads //@version", () => {
    const r = lex("// header\n//@version=5\n\n   // indented comment\nx = 1 // trailing\n");
    expect(r.version).toBe(5);
    expect(r.versionPos).toEqual({ line: 2, col: 1 });
    expect(r.tokens.map((t) => t.v).filter(Boolean)).toEqual(["x", "=", "1"]);
  });

  it("reads numbers, strings with escapes, colours", () => {
    const t = lex('a = 1 + 1.5 + .5 + 1e3 + 2.5E-2\ns = "a\\"b\\n" + \'c\\\'d\'\nc = #ff0000 + #00FF0080').tokens;
    const nums = t.filter((x) => x.t === "num").map((x) => [x.n, x.int]);
    expect(nums).toEqual([[1, true], [1.5, false], [0.5, false], [1000, false], [0.025, false]]);
    expect(t.filter((x) => x.t === "str").map((x) => x.v)).toEqual(['a"b\n', "c'd"]);
    expect(t.filter((x) => x.t === "color").map((x) => x.v)).toEqual(["#FF0000", "#00FF0080"]);
  });
});

describe("parser", () => {
  it("puts line/col on every node", () => {
    const p = parse('//@version=6\nindicator("x")\nv = ta.sma(close, 14)[1] + 2');
    const decl = p.body[1];
    expect(decl).toMatchObject({ kind: "VarDecl", line: 3, col: 1 });
    if (decl.kind !== "VarDecl" || decl.init.kind !== "Binary") throw new Error("shape");
    expect(decl.init).toMatchObject({ line: 3, col: 26, op: "+" });
    expect(decl.init.left).toMatchObject({ kind: "Index", line: 3, col: 5 });
    const check = (n: unknown): void => {
      if (n === null || typeof n !== "object") return;
      if (Array.isArray(n)) return n.forEach(check);
      const o = n as Record<string, unknown>;
      if ("kind" in o) {
        expect(typeof o.line).toBe("number");
        expect(typeof o.col).toBe("number");
      }
      Object.values(o).forEach(check);
    };
    for (const src of Object.values(G)) check(parse(src).body);
  });

  it("respects precedence: ternary < or < and < comparison < additive < multiplicative < unary < history", () => {
    const p = parse("x = a or b and c == d + e * -f[1] ? g : h");
    expect(printProgram(p.body, null)).toBe("x = ((a or (b and (c == (d + (e * (-(f)[1])))))) ? g : h)\n");
  });

  it("parses else-if chains, switch, loops, typed and tuple declarations, functions", () => {
    const src = [
      "var float x = na",
      "[a, b] = f(1)",
      "g(int n, simple float k = 2.0) =>",
      "    y = n * k",
      "    [y, y]",
      "z = if a\n    1\nelse if b\n    2\nelse\n    3",
      "s = switch\n    a => 1\n    => 2",
      "for i = 0 to 10 by 2\n    if i > 4\n        break",
      "while a\n    continue",
    ].join("\n");
    const want = [
      "var float x = na",
      "[a, b] = f(1)",
      "g(int n, float k = 2.0) =>",
      "    y = (n * k)",
      "    [y, y]",
      "z = if a\n    1\nelse if b\n    2\nelse\n    3",
      "s = switch\n    a =>\n        1\n    =>\n        2",
      "for i = 0 to 10 by 2\n    if (i > 4)\n        break",
      "while a\n    continue",
    ].join("\n");
    expect(printProgram(parse(src).body, null)).toBe(want + "\n");
  });
});

describe("round trip", () => {
  for (const [name, src] of Object.entries(G)) {
    it(`print(parse(${name})) parses to the same AST`, () => {
      const a = parse(src);
      const printed = printProgram(a.body, a.version);
      const b = parse(printed);
      expect(shape(b.body)).toEqual(shape(a.body));
      expect(printProgram(b.body, b.version)).toBe(printed);
    });
  }
});
