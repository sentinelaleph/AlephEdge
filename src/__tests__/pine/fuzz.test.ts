/**
 * Robustness: thousands of random and mutated sources must produce either a
 * result or PineErrors with positions — never an exception, never an
 * "Internal Pine-lite error", never a hang.
 */
import { describe, expect, it } from "vitest";

import { compile, runSync, type PineError } from "@/lib/pine";
import { mulberry32 } from "@/lib/pine/runtime/builtins/math";

import * as G from "./goldenScripts";
import { BARS } from "./helpers";

const KINDS = new Set(["syntax", "unsupported", "name", "type", "runtime", "limit", "input"]);

function assertClean(errors: PineError[], src: string): void {
  for (const e of errors) {
    expect(KINDS.has(e.kind), `${e.kind} for ${JSON.stringify(src)}`).toBe(true);
    expect(e.line).toBeGreaterThanOrEqual(1);
    expect(e.col).toBeGreaterThanOrEqual(1);
    expect(e.message.length).toBeGreaterThan(0);
    expect(e.message.startsWith("Internal"), `${e.message} for ${JSON.stringify(src)}`).toBe(false);
  }
}

const VOCAB = [
  "if", "else", "for", "to", "by", "while", "switch", "var", "varip", "and", "or", "not", "true", "false", "na", "break", "continue",
  "x", "y", "f", "close", "open", "ta.sma", "ta.ema", "math.max", "plot", "indicator", "input.int", "color.red", "strategy", "type",
  "0", "1", "14", "2.5", ".5", "1e3", '"s"', "'t'", "#FF0000", "#12",
  "+", "-", "*", "/", "%", "==", "!=", "<", ">", "<=", ">=", "=", ":=", "+=", "?", ":", "=>", "(", ")", "[", "]", ",", ".",
  "\n", "\n    ", "\n        ", "\n  ", "\t", " ", "//c\n", "@", "\\", '"',
];

describe("lexer / parser / checker fuzz", () => {
  it("3,000 random token soups give PineErrors only, quickly", () => {
    const rnd = mulberry32(12345);
    const started = Date.now();
    for (let n = 0; n < 3000; n++) {
      const len = 1 + Math.floor(rnd() * 60);
      let src = rnd() < 0.5 ? '//@version=6\nindicator("f")\n' : "";
      for (let k = 0; k < len; k++) src += VOCAB[Math.floor(rnd() * VOCAB.length)] + (rnd() < 0.7 ? " " : "");
      const t0 = Date.now();
      const r = compile(src);
      expect(Date.now() - t0, `slow compile for ${JSON.stringify(src)}`).toBeLessThan(500);
      if (!r.ok) assertClean(r.errors, src);
    }
    expect(Date.now() - started).toBeLessThan(15_000);
  });

  it("2,000 mutations of the golden scripts compile or fail cleanly, and run cleanly when they compile", () => {
    const rnd = mulberry32(777);
    const sources = Object.values(G);
    const bars = BARS.slice(0, 40);
    let ran = 0;
    for (let n = 0; n < 2000; n++) {
      let src = sources[Math.floor(rnd() * sources.length)];
      const edits = 1 + Math.floor(rnd() * 3);
      for (let e = 0; e < edits; e++) {
        const i = Math.floor(rnd() * src.length);
        const op = rnd();
        if (op < 0.3) src = src.slice(0, i) + src.slice(i + 1 + Math.floor(rnd() * 5));
        else if (op < 0.6) src = src.slice(0, i) + VOCAB[Math.floor(rnd() * VOCAB.length)] + src.slice(i);
        else if (op < 0.8) {
          const lines = src.split("\n");
          const a = Math.floor(rnd() * lines.length);
          lines.splice(a, 0, lines[Math.floor(rnd() * lines.length)]);
          src = lines.join("\n");
        } else src = src.slice(0, i) + src.slice(i, i + 8).split("").reverse().join("") + src.slice(i + 8);
      }
      const c = compile(src);
      if (!c.ok) {
        assertClean(c.errors, src);
        continue;
      }
      const r = runSync(src, bars, { limits: { maxWallMs: 500 } });
      ran++;
      if (!r.ok) assertClean(r.errors, src);
    }
    expect(ran).toBeGreaterThan(50);
  });

  it("pathological nesting is a limit error, not a stack overflow", () => {
    const H = '//@version=6\nindicator("n")\n';
    for (const body of ["x = " + "(".repeat(5000) + "1" + ")".repeat(5000), "x = " + "not ".repeat(5000) + "true", "x = " + "-".repeat(5000) + "1", "x = " + "[".repeat(3000)]) {
      const r = compile(H + body);
      expect(r.ok).toBe(false);
      if (!r.ok) {
        assertClean(r.errors, body.slice(0, 40));
        expect(["limit", "syntax"]).toContain(r.errors[0].kind);
      }
    }
    let nested = H;
    for (let d = 0; d < 300; d++) nested += "    ".repeat(d) + "if close > 0\n";
    nested += "    ".repeat(300) + "x = 1\n";
    const r = compile(nested);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.errors[0].kind).toBe("limit");
  });

  it("a very long flat expression compiles and runs", () => {
    const src = '//@version=6\nindicator("long")\nplot(' + Array.from({ length: 2000 }, () => "close").join(" + ") + ")";
    const r = runSync(src, BARS.slice(0, 10));
    expect(r.ok).toBe(true);
  });
});
