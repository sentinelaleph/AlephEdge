import { describe, expect, it } from "vitest";
import { SAMPLE_HISTORY } from "@/dev/shot/shot.mock";
import { aggregateTrades } from "@/lib/tradeStats";
import { shownTrade, type TradeRecord } from "./trades";

// RVNUSDT on the 26 Oct History screen: booked as a stop at the restart
// minute, recalculated on Sentinel's earlier invalidation.
const booked: TradeRecord = {
  ...SAMPLE_HISTORY[0],
  exit: 91.6,
  exitReason: "sl",
  closedAt: 10_000,
  pnlPct: -8.39,
  pnlQuote: -8.39,
  pnlUsdt: -8.39,
  pnlPctOfCapital: -8.39,
  unleveredNetPct: -8.5,
  recalc: { exit: 96.67, exitReason: "invalidated", closedAt: 2_000, pnlPct: -3.33, pnlQuote: -3.33, unleveredNetPct: -3.45 },
};

describe("shownTrade", () => {
  it("shows Sentinel's close and keeps the first booking beside it", () => {
    const s = shownTrade(booked);
    expect([s.exit, s.exitReason, s.closedAt, s.pnlPct, s.pnlUsdt, s.unleveredNetPct]).toEqual([96.67, "invalidated", 2_000, -3.33, -3.33, -3.45]);
    expect(s.booked).toEqual({ exit: 91.6, exitReason: "sl", closedAt: 10_000, pnlPct: -8.39 });
  });

  it("leaves a trade without a recalculation as booked", () => {
    const plain = { ...booked, recalc: undefined };
    expect(shownTrade(plain)).toBe(plain);
  });

  it("feeds the desk's figures with the recalculated close", () => {
    expect(aggregateTrades([shownTrade(booked)], 0).netPnlQuote).toBeCloseTo(-3.33);
  });
});
