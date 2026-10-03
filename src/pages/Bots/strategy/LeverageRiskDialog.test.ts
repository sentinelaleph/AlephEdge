import { describe, expect, it } from "vitest";
import type { StrategyConfig } from "@/lib/ipc/strategy/strategy";
import { needsLeverageConfirm } from "./LeverageRiskDialog";

const dca = (leverage: number, slPct: number | null): StrategyConfig =>
  ({
    leverage,
    side: "long",
    params: { kind: "dca", slPct },
  }) as unknown as StrategyConfig;

describe("needsLeverageConfirm", () => {
  it("asks only for a leveraged DCA with the stop off", () => {
    expect(needsLeverageConfirm(dca(5, null))).toBe(true);
    expect(needsLeverageConfirm(dca(5, 11))).toBe(false);
    expect(needsLeverageConfirm(dca(1, null))).toBe(false);
  });

  it("never asks for a grid", () => {
    const grid = { leverage: 5, side: "neutral", params: { kind: "grid" } } as unknown as StrategyConfig;
    expect(needsLeverageConfirm(grid)).toBe(false);
  });
});
