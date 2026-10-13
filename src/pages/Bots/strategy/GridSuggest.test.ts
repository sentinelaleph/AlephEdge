import { describe, expect, it } from "vitest";
import type { GridSuggestion } from "@/lib/ipc/strategy/backtest";
import type { StrategyConfig } from "@/lib/ipc/strategy/strategy";
import { applySuggestion, suggestionFits } from "./GridSuggest";

const grid = {
  name: "Grid",
  symbol: "BTCUSDT",
  budget: 200,
  params: {
    kind: "grid",
    range: { type: "absolute", lower: 90, upper: 110 },
    nGrids: 10,
    spacing: "arith",
    stopOutPct: 3,
    trailingUp: false,
    trailUpLimit: null,
    takeProfitPct: null,
    maxDurationMin: null,
  },
} as unknown as StrategyConfig;

const s: GridSuggestion = {
  price: 100,
  lowerPct: 6.2,
  upperPct: 4.8,
  nGrids: 24,
  stepPct: 0.45,
  netPerGridPct: 0.41,
  atrHourPct: 0.44,
  atrDayPct: 2.9,
  windowLow: 93.8,
  windowHigh: 104.8,
};

describe("applySuggestion", () => {
  it("replaces range, grid count and spacing only", () => {
    const next = applySuggestion(grid, s);
    expect(next.params).toEqual({
      ...grid.params,
      range: { type: "relative", lowerPct: 6.2, upperPct: 4.8 },
      nGrids: 24,
      spacing: "geom",
    });
    expect(next.budget).toBe(200);
  });

  it("leaves a DCA config alone", () => {
    const dca = { ...grid, params: { kind: "dca" } } as unknown as StrategyConfig;
    expect(applySuggestion(dca, s)).toBe(dca);
  });
});

describe("suggestionFits", () => {
  it("holds for the asked config and the applied one, and drops on any other edit", () => {
    const asked = JSON.stringify(grid);
    const applied = JSON.stringify(applySuggestion(grid, s));
    expect(suggestionFits(grid, asked, applied)).toBe(true);
    expect(suggestionFits(applySuggestion(grid, s), asked, applied)).toBe(true);
    expect(suggestionFits({ ...grid, budget: 300 }, asked, applied)).toBe(false);
  });
});
