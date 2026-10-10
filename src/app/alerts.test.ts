import { describe, expect, it } from "vitest";
import type { TFunction } from "i18next";
import type { DeskContextValue } from "./DeskProvider";
import { deriveAlerts } from "./alerts";
import type { ClosedCycleRow, StrategyBotView } from "@/lib/ipc/strategy/strategy";

const NOW = Date.UTC(2026, 9, 8, 12);
const TODAY = Date.UTC(2026, 9, 8);
const t = ((key: string, opts?: Record<string, unknown>) => (opts ? `${key} ${JSON.stringify(opts)}` : key)) as unknown as TFunction;

const cycle = (over: Partial<ClosedCycleRow>): ClosedCycleRow => ({
  botId: "sb_grid",
  botName: "Grid SOLUSDT #1",
  kind: "grid",
  market: "futures",
  symbol: "SOLUSDT",
  side: "neutral",
  seq: 1,
  openedAt: TODAY - 86_400_000,
  closedAt: TODAY + 240_000,
  exitReason: "manual",
  pnlQuote: -7.2,
  pnlPctBudget: -0.72,
  archived: true,
  ...over,
});

function ctx(recent: ClosedCycleRow[], todayCycles: number, bots: Partial<StrategyBotView>[] = []): DeskContextValue {
  return {
    desk: {
      loaded: true,
      error: null,
      status: { killSwitchTripped: false, btcRegime: "normal", recentSkips: [] },
    },
    risk: { state: null },
    feed: { loaded: false },
    pnl: { trades: [] },
    strategy: {
      risk: null,
      bots: bots as StrategyBotView[],
      pnl: { totals: { cycles: recent.length, netQuote: 0, todayCycles, todayQuote: 0 }, recent },
    },
  } as unknown as DeskContextValue;
}

describe("DCA / Grid alerts", () => {
  it("cycles closed today raise a closed alert though no signal trade exists", () => {
    // Testnet 8 Oct: two cycles closed, the alerts said nothing.
    const alerts = deriveAlerts(ctx([cycle({}), cycle({ botId: "sb_dca", seq: 1, kind: "dca" })], 2), t, null, NOW);
    const closed = alerts.find((a) => a.key === "cycleClosed");
    expect(closed?.category).toBe("dealClosed");
    expect(closed?.count).toBe(2);
    expect(alerts.some((a) => a.key === "cycleStop")).toBe(false);
  });

  it("a stop exit warns; a liquidation is danger", () => {
    const sl = deriveAlerts(ctx([cycle({ exitReason: "sl" })], 1), t, null, NOW).find((a) => a.key === "cycleStop");
    expect(sl?.severity).toBe("warn");
    expect(sl?.category).toBe("stopLoss");
    const liq = deriveAlerts(ctx([cycle({ exitReason: "liq" }), cycle({ seq: 2, exitReason: "sl" })], 2), t, null, NOW).find(
      (a) => a.key === "cycleStop",
    );
    expect(liq?.severity).toBe("danger");
    expect(liq?.count).toBe(2);
  });

  it("yesterday's cycles raise nothing", () => {
    const alerts = deriveAlerts(ctx([cycle({ closedAt: TODAY - 1, exitReason: "sl" })], 0), t, null, NOW);
    expect(alerts.filter((a) => a.key.startsWith("cycle"))).toEqual([]);
  });

  it("a bot a stop ended is a bot error", () => {
    const alerts = deriveAlerts(ctx([], 0, [{ id: "sb_x", name: "DCA X", runState: "dead", deadReason: "liq" }]), t, null, NOW);
    const dead = alerts.find((a) => a.key === "strategyDead:sb_x");
    expect(dead?.severity).toBe("danger");
    expect(dead?.title).toContain("DCA X");
  });
});
