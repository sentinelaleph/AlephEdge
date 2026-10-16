import { describe, expect, it } from "vitest";
import type { TradePreview } from "@/lib/ipc/bot/bot";
import type { PreviewDto, StrategyConfig } from "@/lib/ipc/strategy/strategy";
import { riskSetViewThresholds } from "@/lib/ipc/risk/risk";
import { strategyCandidate, tradeCandidate } from "./PortfolioWhatIf";

describe("what-if candidates", () => {
  it("a strategy candidate needs a priced, accepted preview", () => {
    const cfg = { symbol: "SOLUSDT", budget: 300 } as StrategyConfig;
    const preview = { referencePrice: 152.4, error: null } as PreviewDto;
    expect(strategyCandidate(cfg, preview)).toEqual({ type: "strategy", config: cfg, price: 152.4 });
    expect(strategyCandidate(cfg, null)).toBeNull();
    expect(strategyCandidate(cfg, { ...preview, error: { code: "tpBelowFees", field: "tpPct" } } as PreviewDto)).toBeNull();
    expect(strategyCandidate(cfg, { ...preview, referencePrice: 0 })).toBeNull();
  });

  it("a trade candidate carries the preview's own figures", () => {
    const p = {
      botKind: "futures",
      symbol: "ETHUSDT",
      direction: "short",
      entry: 3100,
      takeProfit: 2900,
      stopLoss: 3200,
      capital: 60,
      notionalUsdt: 300,
      effectiveLeverage: 5,
      leverage: 5,
      qty: 0.0967,
    } as TradePreview;
    const c = tradeCandidate(p);
    expect(c.type).toBe("trade");
    if (c.type !== "trade") return;
    expect(c.trade).toEqual({
      botKind: "futures",
      symbol: "ETHUSDT",
      direction: "short",
      entry: 3100,
      takeProfit: 2900,
      stopLoss: 3200,
      capital: 60,
      notionalUsdt: 300,
      effectiveLeverage: 5,
      leverage: 5,
    });
  });
});

describe("view thresholds (browser mock mirrors the Rust range check)", () => {
  it("stores a valid set and refuses an out-of-range one with the Rust code", async () => {
    const s = await riskSetViewThresholds({ sameDirectionPct: 75, singleCoinPct: 35, stress20Pct: 20 });
    expect(s.viewThresholds).toEqual({ sameDirectionPct: 75, singleCoinPct: 35, stress20Pct: 20 });
    await expect(riskSetViewThresholds({ sameDirectionPct: 40, singleCoinPct: 35, stress20Pct: 20 })).rejects.toBe("viewThresholdSameDirection");
    await expect(riskSetViewThresholds({ sameDirectionPct: 80, singleCoinPct: 35, stress20Pct: 0 })).rejects.toBe("viewThresholdStress");
  });
});
