import { describe, expect, it } from "vitest";
import type { StrategyBotView } from "@/lib/ipc/strategy/strategy";
import { totalPnlPct, totalPnlQuote } from "./StrategyBotsTable";

describe("DCA / Grid total P&L", () => {
  it("is realized plus the open cycle, in USDT and as % of the budget (owner, 2 Nov 2026)", () => {
    // DCA Long Classic on the testnet run: +12.45 realized, open cycle -13.04 unrealized and -0.04 fees.
    const v = { budget: 1000, equity: 1000 + 12.45 - 13.04 - 0.04 } as StrategyBotView;
    expect(totalPnlQuote(v)).toBeCloseTo(-0.63, 9);
    expect(totalPnlPct(v)).toBeCloseTo(-0.063, 9);
  });
});
