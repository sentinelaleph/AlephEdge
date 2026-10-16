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
    // Regression: closed cycles once raised no alert at all.
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

describe("foreign-trade alert", () => {
  const fill = (symbol: string, tradeId: number, detectedMs: number) => ({
    symbol,
    tradeId,
    orderId: 1,
    side: "BUY",
    qty: 1,
    price: 1,
    timeMs: detectedMs - 1_000,
    detectedMs,
  });
  const withWatch = (liveBuild: boolean, alarms: ReturnType<typeof fill>[]) =>
    ({ ...ctx([], 0), foreignFill: { liveBuild, enabled: true, watching: true, lastCheckMs: NOW, lastError: null, alarms } }) as DeskContextValue;

  it("an unacknowledged foreign fill is a danger alert in the kill-switch category", () => {
    const alerts = deriveAlerts(withWatch(true, [fill("XUSDT", 1, NOW - 5_000), fill("XUSDT", 2, NOW - 1_000), fill("YUSDT", 3, NOW - 2_000)]), t, null, NOW);
    const a = alerts.find((x) => x.key === "foreignTrades");
    expect(a?.severity).toBe("danger");
    expect(a?.category).toBe("killSwitch");
    expect(a?.count).toBe(3);
    expect(a?.detail).toBe("XUSDT, YUSDT");
    expect(alerts[0].key).toBe("foreignTrades");
  });

  it("no alarm, or a paper build, raises nothing", () => {
    expect(deriveAlerts(withWatch(true, []), t, null, NOW).some((a) => a.key === "foreignTrades")).toBe(false);
    expect(deriveAlerts(withWatch(false, [fill("XUSDT", 1, NOW)]), t, null, NOW).some((a) => a.key === "foreignTrades")).toBe(false);
    expect(deriveAlerts(ctx([], 0), t, null, NOW).some((a) => a.key === "foreignTrades")).toBe(false);
  });
});

describe("portfolio risk alerts", () => {
  it("a crossed threshold alerts while it holds, with its start time", async () => {
    const { SAMPLE_SNAPSHOT } = await import("@/lib/ipc/riskview/riskview.mock");
    const base = ctx([], 0);
    const withView = { ...base, riskView: { snapshot: SAMPLE_SNAPSHOT } } as unknown as DeskContextValue;
    const alerts = deriveAlerts(withView, t, null, NOW).filter((a) => a.category === "portfolioRisk");
    // The sample paper book crosses same-direction and single-coin; stress is only a warning.
    expect(alerts.map((a) => a.key)).toEqual(["portfolioRisk:paper:sameDirection", "portfolioRisk:paper:singleCoin"]);
    expect(alerts[0].severity).toBe("danger");
    expect(alerts[0].at).toBe(SAMPLE_SNAPSHOT.paper.checks[0].sinceMs);
    expect(alerts[0].title).toContain('"value":88');
    // No snapshot (not loaded yet, or the old test context): nothing.
    expect(deriveAlerts(base, t, null, NOW).some((a) => a.category === "portfolioRisk")).toBe(false);
  });
});
