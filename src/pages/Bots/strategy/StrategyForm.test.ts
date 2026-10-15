import { describe, expect, it } from "vitest";
import type { Preset, StrategyConfig } from "@/lib/ipc/strategy/strategy";
import { openingBudget } from "../StrategyCreatePage";
import { matchesPreset, rangeExitOf, withRangeExit } from "./StrategyForm";

const preset = (rule: string): Preset =>
  ({
    id: rule === "btcOnly" ? "dca_long_btc" : "dca_long_classic",
    universe: { rule, rankFrom: 1, topN: rule === "btcOnly" ? 1 : 5, volumeWindowDays: 90, reselect: "monthly", exclude: [] },
    config: {
      market: "futures",
      side: "long",
      leverage: 1,
      maxDrawdownPct: null,
      pauseOnBtcBreak: false,
      portfolioBreaker: false,
      start: { type: "immediately" },
      restart: { cooldownMin: 0 },
      params: { kind: "dca", tpPct: 2 },
    },
  }) as unknown as Preset;

const cfgOf = (p: Preset, symbol: string, patch: Partial<StrategyConfig> = {}): StrategyConfig =>
  ({ ...p.config, symbol, name: "Mine", budget: 200, ...patch }) as StrategyConfig;

describe("matchesPreset", () => {
  it("keeps a BTC-only template on BTCUSDT only", () => {
    const p = preset("btcOnly");
    expect(matchesPreset(cfgOf(p, "BTCUSDT"), p)).toBe(true);
    expect(matchesPreset(cfgOf(p, "ETHUSDT"), p)).toBe(false);
  });

  it("frees name and budget, not a simulated setting", () => {
    const p = preset("topUsdtPerpsByQuoteVolume");
    expect(matchesPreset(cfgOf(p, "ETHUSDT", { budget: 177 }), p)).toBe(true);
    expect(matchesPreset(cfgOf(p, "ETHUSDT", { leverage: 2 }), p)).toBe(false);
    expect(matchesPreset(cfgOf(p, "ETHUSDT", { portfolioBreaker: true }), p)).toBe(false);
  });
});

describe("openingBudget", () => {
  // Fresh install: 1000 balance, Cautious 20% cap = 200 USDT free.
  it("lowers a 1000 USDT template to what is free when that clears the minimum", () => {
    expect(openingBudget(1000, 200, 176.98)).toEqual({ budget: 200, short: false });
    expect(openingBudget(1000, 187.53, 180)).toEqual({ budget: 187, short: false });
    // the whole USDT would fall under the minimum: keep the cents
    expect(openingBudget(1000, 180.5, 180.2)).toEqual({ budget: 180.5, short: false });
  });

  it("keeps the budget and reports the shortfall when no budget under the cap passes", () => {
    // dca_long_safe needs 218.10
    expect(openingBudget(1000, 200, 218.1)).toEqual({ budget: 1000, short: true });
    expect(openingBudget(1000, 0, 176.98)).toEqual({ budget: 1000, short: true });
  });

  it("never raises a budget that already fits", () => {
    expect(openingBudget(100, 200, 50)).toEqual({ budget: 100, short: false });
    expect(openingBudget(1000, 4000, null)).toEqual({ budget: 1000, short: false });
  });
});

describe("grid range exit", () => {
  const grid = (stopOutPct: number | null, afterStop: boolean) =>
    ({
      market: "futures",
      restart: { cooldownMin: 0, maxCycles: null, afterStop, afterLiquidation: false, priceBand: null, endAtMs: null },
      params: { kind: "grid", range: { type: "relative", lowerPct: 8, upperPct: 8 }, nGrids: 20, spacing: "geom", stopOutPct, trailingUp: false, trailUpLimit: null, takeProfitPct: null, maxDurationMin: null },
    }) as unknown as StrategyConfig;
  const exitOf = (c: StrategyConfig) => (c.params.kind === "grid" ? rangeExitOf(c.params, c.restart.afterStop) : null);

  it("reads the two engine settings as one choice", () => {
    expect(exitOf(grid(null, true))).toBe("wait");
    expect(exitOf(grid(null, false))).toBe("wait");
    expect(exitOf(grid(5, false))).toBe("close");
    expect(exitOf(grid(5, true))).toBe("restart");
  });

  it("writes each choice back to exactly those settings", () => {
    const close = withRangeExit(grid(null, true), "close");
    expect(close.params).toMatchObject({ stopOutPct: 3 });
    expect(close.restart.afterStop).toBe(false);
    const restart = withRangeExit(grid(5, false), "restart");
    expect(restart.params).toMatchObject({ stopOutPct: 5 });
    expect(restart.restart.afterStop).toBe(true);
    const wait = withRangeExit(grid(5, false), "wait");
    expect(wait.params).toMatchObject({ stopOutPct: null });
    expect(wait.restart.afterStop).toBe(false);
    for (const e of ["wait", "close", "restart"] as const) expect(exitOf(withRangeExit(grid(4, true), e))).toBe(e);
  });

  it("leaves a DCA config alone", () => {
    const dca = { params: { kind: "dca" }, restart: { afterStop: true } } as unknown as StrategyConfig;
    expect(withRangeExit(dca, "close")).toBe(dca);
  });
});
