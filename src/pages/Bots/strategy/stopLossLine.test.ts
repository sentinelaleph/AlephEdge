import { beforeEach, describe, expect, it } from "vitest";
import i18n from "@/i18n";
import { stopLossLine } from "./stopLossLine";

const t = i18n.t.bind(i18n);

describe("stop loss line", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("shows the stop price and the loss when a price is known", () => {
    const line = stopLossLine(t, { stopPrice: 33.95, avgEntry: 97, lossQuote: 196.3, lossPctOfBudget: 19.63 }, 65, false, "en-US");
    expect(line).toBe("Stop price ≈ 33.95 · Max loss ≈ 196.3 USDT (19.6% of the budget)");
  });

  it("never invents a price: relative to the average entry without one", () => {
    const e = { stopPrice: null, avgEntry: null, lossQuote: 196.3, lossPctOfBudget: 19.63 };
    expect(stopLossLine(t, e, 65, false, "en-US")).toBe("Stop ≈ −65% from average entry · Max loss ≈ 196.3 USDT (19.6% of the budget)");
    expect(stopLossLine(t, e, 65, true, "en-US")).toContain("+65% from average entry");
  });

  it("reads in Turkish", async () => {
    await i18n.changeLanguage("tr");
    const line = stopLossLine(t, { stopPrice: 35, avgEntry: 100, lossQuote: 65.5, lossPctOfBudget: 6.55 }, 65, false, "tr-TR");
    expect(line).toBe("Stop fiyatı ≈ 35 · En fazla zarar ≈ 65,5 USDT (bütçenin %6,6 kadarı)");
  });
});
