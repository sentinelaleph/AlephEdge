import { describe, expect, it } from "vitest";
import type { BotConfig, BotDeskStatus } from "@/lib/ipc/bot/bot";
import { rowStatus, signalBotRows } from "./botRows";

const status = (patch: Partial<BotDeskStatus> = {}): BotDeskStatus => ({
  liveTradingEnabled: false,
  binanceIsProduction: true,
  futures: null,
  spot: null,
  pump: null,
  futuresRunning: false,
  spotRunning: false,
  pumpRunning: false,
  openPositions: [],
  recentSkips: [],
  killSwitchTripped: false,
  btcRegime: "normal",
  ...patch,
});

describe("signal bot rows", () => {
  it("Pump is marked untested and is not locked behind the global risk level", () => {
    const pumpConfig = { kind: "pump", exchangeId: "binance", capital: 100, leverage: 2, maxPositions: 1 } as BotConfig;
    const rows = signalBotRows(status({ pump: pumpConfig }));
    const pump = rows.find((r) => r.kind === "pump")!;
    expect(pump.untested).toBe(true);
    expect(pump.startBlockKey).toBeNull();
    expect(rowStatus(pump).labelKey).toBe("botState.stopped");
    expect(rows.filter((r) => r.untested).map((r) => r.kind)).toEqual(["pump"]);
  });

  it("the daily stop still blocks every start", () => {
    const rows = signalBotRows(status({ killSwitchTripped: true }));
    expect(rows.every((r) => r.startBlockKey === "botsList.blocked.killSwitch")).toBe(true);
  });
});

const cfg = (capital: number): BotConfig => ({ kind: "futures", exchangeId: "binance", maxPositions: 3, capital, leverage: 3 });
const capStatus = (capital: number, running: boolean): BotDeskStatus =>
  status({ futures: cfg(capital), futuresRunning: running });

// Audit 2026-10-08, signal_bots 1: a bot above the per-position cap skipped
// every signal while its row said "Start" and "Running". Rust now refuses
// the start; the row says why before anyone presses it.
describe("signal bot rows and the per-position cap", () => {
  it("a stopped bot above the cap cannot be started from its row", () => {
    const row = signalBotRows(capStatus(100, false), 20).find((r) => r.kind === "futures")!;
    expect(row.capitalAboveCap).toBe(true);
    expect(row.startBlockKey).toBe("botsList.blocked.capitalAboveCap");
  });

  it("at or under the cap, or with the cap unknown, nothing is blocked", () => {
    for (const [capital, cap] of [
      [20, 20],
      [100, null],
    ] as const) {
      const row = signalBotRows(capStatus(capital, false), cap).find((r) => r.kind === "futures")!;
      expect(row.capitalAboveCap).toBe(false);
      expect(row.startBlockKey).toBeNull();
    }
  });

  it("a running bot above a lowered cap is flagged for the page banner", () => {
    const row = signalBotRows(capStatus(100, true), 20).find((r) => r.kind === "futures")!;
    expect(row.running && row.capitalAboveCap).toBe(true);
  });
});
