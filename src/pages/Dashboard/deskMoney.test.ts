import { describe, expect, it } from "vitest";
import type { PnlStats } from "@/lib/ipc/trades/trades";
import { deskMoney } from "./DashboardPage";

const stats = (over: Partial<PnlStats> = {}): PnlStats => ({
  totalTrades: 0,
  wins: 0,
  losses: 0,
  winRate: null,
  netPnlQuote: 0,
  avgPnlPct: 0,
  profitFactor: null,
  maxDrawdownQuote: 0,
  todayPnlQuote: 0,
  ...over,
});

describe("Dashboard money tiles", () => {
  it("a DCA-only desk shows its cycle losses, not 'No trades today'", () => {
    // Testnet 8 Oct: Grid/DCA cycles lost 8.40 USDT net while the tile read "No trades today".
    const m = deskMoney(stats(), { cycles: 2, netQuote: -8.4034, todayCycles: 2, todayQuote: -8.4034 });
    expect(m.anyResult).toBe(true);
    expect(m.hasCycles).toBe(true);
    expect(m.realizedToday).toBeCloseTo(-8.4034, 6);
    expect(m.netAll).toBeCloseTo(-8.4034, 6);
  });

  it("adds signal trades and cycles", () => {
    const m = deskMoney(stats({ totalTrades: 6, netPnlQuote: -0.84, todayPnlQuote: 0 }), {
      cycles: 3,
      netQuote: -6.67,
      todayCycles: 2,
      todayQuote: -8.4,
    });
    expect(m.netAll).toBeCloseTo(-7.51, 6);
    expect(m.realizedToday).toBeCloseTo(-8.4, 6);
  });

  it("no trade and no cycle ever is a dash; stats still loading is unknown", () => {
    expect(deskMoney(stats(), { cycles: 0, netQuote: 0, todayCycles: 0, todayQuote: 0 }).anyResult).toBe(false);
    expect(deskMoney(null, null).realizedToday).toBeNull();
  });

  it("without cycle totals (real-money scope) the tiles are the signal stats", () => {
    const m = deskMoney(stats({ totalTrades: 2, netPnlQuote: 5, todayPnlQuote: 1 }), null);
    expect([m.netAll, m.realizedToday, m.hasCycles]).toEqual([5, 1, false]);
  });
});
