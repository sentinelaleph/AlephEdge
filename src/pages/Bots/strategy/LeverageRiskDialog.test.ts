import { describe, expect, it } from "vitest";
import type { PreviewDto, StrategyConfig } from "@/lib/ipc/strategy/strategy";
import { needsLiquidationConfirm } from "./LeverageRiskDialog";

const dca = (leverage: number, slPct: number | null, side = "long", maxDrawdownPct: number | null = null): StrategyConfig =>
  ({
    leverage,
    side,
    maxDrawdownPct,
    params: { kind: "dca", slPct },
  }) as unknown as StrategyConfig;

const grid = (leverage: number, stopOutPct: number | null, side = "neutral"): StrategyConfig =>
  ({ leverage, side, maxDrawdownPct: 25, params: { kind: "grid", stopOutPct } }) as unknown as StrategyConfig;

const dcaPreview = (liqPrice: number | null) => ({ dca: { liqPrice }, grid: null }) as unknown as PreviewDto;
const gridPreview = (liqPriceBottom: number | null, liqPriceTop: number | null) =>
  ({ dca: null, grid: { liqPriceBottom, liqPriceTop } }) as unknown as PreviewDto;

describe("needsLiquidationConfirm", () => {
  it("asks for a DCA with the stop off whenever the preview has a liquidation price", () => {
    expect(needsLiquidationConfirm(dca(5, null), dcaPreview(80.4))).toBe(true);
    // a 1x short is liquidated near twice its average: asked too
    expect(needsLiquidationConfirm(dca(1, null, "short"), dcaPreview(211.9))).toBe(true);
    // a 1x long has no liquidation price
    expect(needsLiquidationConfirm(dca(1, null), dcaPreview(null))).toBe(false);
    // a stop loss in front of it
    expect(needsLiquidationConfirm(dca(5, 11), dcaPreview(80.4))).toBe(false);
    expect(needsLiquidationConfirm(dca(1, 17, "short"), dcaPreview(211.9))).toBe(false);
    // the drawdown stop does not lift the ask (no loosening)
    expect(needsLiquidationConfirm(dca(3, null, "long", 25), dcaPreview(67))).toBe(true);
  });

  it("falls back to leverage or a short side without a preview", () => {
    expect(needsLiquidationConfirm(dca(5, null), null)).toBe(true);
    expect(needsLiquidationConfirm(dca(1, null, "short"), null)).toBe(true);
    expect(needsLiquidationConfirm(dca(1, null), null)).toBe(false);
  });

  it("asks for a grid with stop-out off and a liquidation price", () => {
    expect(needsLiquidationConfirm(grid(1, null), gridPreview(null, 190))).toBe(true);
    expect(needsLiquidationConfirm(grid(2, null, "long"), gridPreview(70, null))).toBe(true);
    expect(needsLiquidationConfirm(grid(1, null, "long"), gridPreview(null, null))).toBe(false);
    expect(needsLiquidationConfirm(grid(5, 3), gridPreview(70, 140))).toBe(false);
    expect(needsLiquidationConfirm(grid(1, null, "short"), null)).toBe(true);
    expect(needsLiquidationConfirm(grid(1, null, "long"), null)).toBe(false);
  });
});
