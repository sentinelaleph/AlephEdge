/**
 * Public API, inputs, limits, the worker protocol / runner, and the
 * no-eval guarantee.
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it, vi } from "vitest";

import { compile, PineRunner, runAsync, runSync, type WorkerLike } from "@/lib/pine";
import { handleRunRequest, type RunRequest, type RunResponse } from "@/lib/pine/protocol";
import { UNSUPPORTED_FEATURES } from "@/lib/pine/registry";

import { BARS, err, ok, plot } from "./helpers";

const H = '//@version=6\nindicator("api")\n';

describe("compile()", () => {
  it("lists inputs with type, default, title, limits, options and layout fields", () => {
    const c = compile(
      H +
        [
          'len = input.int(14, "Length", minval = 1, maxval = 200, step = 1, tooltip = "bars", group = "Main", inline = "a")',
          "mult = input.float(2.0, minval = 0.1)",
          'mode = input.string("EMA", "Mode", options = ["EMA", "SMA"])',
          'show = input.bool(true, "Show")',
          'col = input.color(color.new(color.red, 50), "Colour")',
          'src = input.source(hl2, "Source")',
          'tf = input.timeframe("", "Timeframe")',
          "plot(close)",
        ].join("\n"),
    );
    if (!c.ok) throw new Error(JSON.stringify(c.errors));
    expect(c.inputs).toEqual([
      { id: 0, kind: "int", title: "Length", defval: 14, minval: 1, maxval: 200, step: 1, tooltip: "bars", group: "Main", inline: "a", line: 3, col: 7 },
      { id: 1, kind: "float", title: "mult", defval: 2, minval: 0.1, line: 4, col: 8 },
      { id: 2, kind: "string", title: "Mode", defval: "EMA", options: ["EMA", "SMA"], line: 5, col: 8 },
      { id: 3, kind: "bool", title: "Show", defval: true, line: 6, col: 8 },
      { id: 4, kind: "color", title: "Colour", defval: "#F2364580", line: 7, col: 7 },
      { id: 5, kind: "source", title: "Source", defval: "hl2", line: 8, col: 7 },
      { id: 6, kind: "timeframe", title: "Timeframe", defval: "", line: 9, col: 6 },
    ]);
    expect(c.meta).toEqual({ title: "api", shorttitle: "api", overlay: false, format: "inherit", precision: null, version: 6 });
  });
});

describe("input overrides", () => {
  const src = H + 'len = input.int(3, "Length", minval = 1, maxval = 10)\nsrc = input.source(close, "Src")\nplot(ta.sma(src, len), "s")';

  it("defaults apply without overrides; overrides change the result", () => {
    const def = plot(ok(src), "s").values;
    expect(def[2]).toBeCloseTo((BARS[0].c + BARS[1].c + BARS[2].c) / 3, 9);
    const o = plot(ok(src, BARS, { inputs: { 0: 2, 1: "open" } }), "s").values;
    expect(o[1]).toBeCloseTo((BARS[0].o + BARS[1].o) / 2, 9);
  });

  it("rejects overrides that do not fit the declaration, at the input's position", () => {
    expect(err(src, undefined, { inputs: { 0: 11 } })).toMatchObject({ kind: "input", line: 3, col: 7, message: 'Input "Length" must be at most 10' });
    expect(err(src, undefined, { inputs: { 0: 2.5 } }).message).toBe('Input "Length" must be an integer');
    expect(err(src, undefined, { inputs: { 1: "vwap" } }).message).toContain("must be one of open, high");
  });
});

describe("limits", () => {
  it("bars", () => {
    expect(err(H + "plot(close)", BARS, { limits: { maxBars: 100 } })).toMatchObject({ kind: "limit", message: "Too many bars: 300 (limit 100)" });
  });

  it("loop iterations per loop per bar", () => {
    const e = err(H + "n = 0\nwhile true\n    n += 1\nplot(n)");
    expect(e).toMatchObject({ kind: "limit", line: 4, col: 1 });
    expect(e.message).toContain("more than 10,000 iterations on bar 0");
  });

  it("evaluation steps per bar", () => {
    const e = err(H + "s = 0\nfor i = 1 to 9000\n    for j = 1 to 200\n        s += 1\nplot(s)");
    expect(e.kind).toBe("limit");
    expect(e.message).toContain("evaluation steps on bar 0");
  });

  it("wall time", () => {
    const e = err(H + "s = 0.0\nfor i = 1 to 9000\n    s += math.sqrt(i)\nplot(s)", BARS, { limits: { maxWallMs: 5 } });
    expect(e).toMatchObject({ kind: "limit", message: "Run stopped: wall time limit of 5 ms exceeded" });
  });

  it("plots per script and source size are compile-time limits", () => {
    const many = H + Array.from({ length: 65 }, (_, i) => `plot(close + ${i})`).join("\n");
    expect(compile(many)).toMatchObject({ ok: false, errors: [{ kind: "limit", line: 67, col: 1 }] });
    expect(compile(H + "// " + "x".repeat(500_001))).toMatchObject({ ok: false, errors: [{ kind: "limit", line: 1, col: 1 }] });
  });

  it("bad bars and run-time argument errors are reported with positions", () => {
    const bad = BARS.slice(0, 5).map((b, i) => (i === 3 ? { ...b, c: NaN } : b));
    expect(err(H + "plot(close)", bad)).toMatchObject({ kind: "runtime", message: "Bar 3 has a missing or non-finite value" });
    expect(err(H + "x = 0\nplot(ta.sma(close, x))")).toMatchObject({ kind: "runtime", line: 4, col: 6, message: "ta.sma: length must be at least 1 (got 0)" });
  });

  it("5,000 bars of typical scripts finish inside the default 2 s wall-time limit", async () => {
    const G = await import("./goldenScripts");
    const bars = Array.from({ length: 5000 }, (_, i) => ({ ...BARS[i % 240], t: BARS[0].t + i * 3_600_000 }));
    for (const src of [G.MACD_COLORS, G.SUPERTREND, G.STOCH_RSI, G.MA_SWITCH, G.FOR_SMA]) {
      const r = ok(src, bars);
      expect(r.stats.ms).toBeLessThan(2000);
    }
  });

  it("stats are reported on success", () => {
    const r = ok(H + "plot(close)");
    expect(r.stats.bars).toBe(300);
    expect(r.stats.nodes).toBeGreaterThan(300);
  });
});

describe("worker protocol and runner", () => {
  it("handleRunRequest answers with the request id and a plain result", () => {
    const res = handleRunRequest({ id: 7, source: H + "plot(close)", bars: BARS.slice(0, 3), options: {} });
    expect(res.id).toBe(7);
    expect(res.result.ok).toBe(true);
    expect(JSON.parse(JSON.stringify(res))).toEqual(res);
  });

  class FakeWorker implements WorkerLike {
    onmessage: ((e: MessageEvent<RunResponse>) => void) | null = null;
    onerror: ((e: unknown) => void) | null = null;
    terminated = false;
    constructor(private readonly respond: boolean) {}
    postMessage(m: RunRequest): void {
      if (this.respond) queueMicrotask(() => this.onmessage?.({ data: handleRunRequest(m) } as MessageEvent<RunResponse>));
    }
    terminate(): void {
      this.terminated = true;
    }
  }

  it("resolves with the worker's result", async () => {
    const runner = new PineRunner(() => new FakeWorker(true));
    const r = await runner.run(H + "plot(close)", BARS.slice(0, 5));
    expect(r.ok).toBe(true);
    runner.dispose();
  });

  it("terminates a stuck worker after the wall-time limit plus slack and starts a fresh one next time", async () => {
    const made: FakeWorker[] = [];
    let respond = false;
    const runner = new PineRunner(() => {
      const w = new FakeWorker(respond);
      made.push(w);
      return w;
    }, 5);
    const r = await runner.run(H + "plot(close)", BARS, { limits: { maxWallMs: 10 } });
    expect(r).toMatchObject({ ok: false, errors: [{ kind: "limit", message: "Run stopped: wall time limit of 10 ms exceeded" }] });
    expect(made[0].terminated).toBe(true);
    respond = true;
    expect((await runner.run(H + "plot(close)", BARS.slice(0, 3))).ok).toBe(true);
    expect(made).toHaveLength(2);
  });

  it("runAsync falls back to a synchronous run where Worker is unavailable", async () => {
    vi.stubGlobal("Worker", undefined);
    const r = await runAsync(H + "plot(close)", BARS.slice(0, 3));
    expect(r.ok).toBe(true);
    vi.unstubAllGlobals();
  });

  it("runSync never throws, even for nonsense input", () => {
    expect(runSync("\u0000\u0001", BARS.slice(0, 2)).ok).toBe(false);
  });
});

describe("safety", () => {
  it("no eval / Function constructor anywhere in the engine sources", () => {
    // Aleph Edge: the engine copy lives in pine-engine/ (only change from the website test).
    const root = join(process.cwd(), "pine-engine", "src", "lib", "pine");
    const files: string[] = [];
    const walk = (d: string) => readdirSync(d).forEach((f) => (statSync(join(d, f)).isDirectory() ? walk(join(d, f)) : files.push(join(d, f))));
    walk(root);
    expect(files.length).toBeGreaterThan(20);
    for (const f of files) {
      const text = readFileSync(f, "utf8");
      expect(/\beval\s*\(/.test(text), `${f} uses eval(`).toBe(false);
      expect(/new\s+Function\b/.test(text), `${f} uses new Function`).toBe(false);
      expect(/\bFunction\s*\(/.test(text), `${f} uses Function(`).toBe(false);
      expect(/import\s*\(/.test(text), `${f} uses dynamic import`).toBe(false);
    }
  });
});

describe("UNSUPPORTED_FEATURES (the reference page's rejected list)", () => {
  it.each(UNSUPPORTED_FEATURES.map((f) => [f.feature, f.sample] as const))("%s is rejected by the checker", (_name, sample) => {
    const source = sample.startsWith("//@version") ? sample : H + sample + "\nplot(close)";
    const c = compile(source);
    expect(c.ok).toBe(false);
    if (!c.ok) expect(c.errors[0].kind).toBe("unsupported");
  });
});
