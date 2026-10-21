import { describe, expect, it } from "vitest";
import type { BotConfig, BotDeskStatus } from "@/lib/ipc/bot/bot";
import { allBotRows, botRowName, rowStatus, signalBotRows, tvBotRow } from "./botRows";

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

// Rust refuses to start a bot whose capital is above the per-position cap;
// the row says why before anyone presses Start.
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

// Testnet run, 21 Oct: "Running 1 of 5" and the bot tables left the
// TradingView bot out while the header said "Live running: 1".
describe("the TradingView bot in every bot list and count", () => {
  const tv: BotConfig = {
    kind: "tradingview",
    exchangeId: "binance",
    maxPositions: 3,
    capital: 33.33,
    leverage: 3,
    mode: "live",
    live: true,
    tv: { name: "Trend", budgetUsdt: 100, maxSizeUsdt: 50, maxOpensPerDay: 10, staleS: 30, priceGuardPct: 0.5, requirePrice: true },
  };
  const pos = { botKind: "tradingview", live: true } as BotDeskStatus["openPositions"][number];

  it("is its own row: type, mode, state, open positions and Start / Stop", () => {
    const s = status({ tradingview: tv, tradingviewRunning: true, openPositions: [pos] });
    const row = tvBotRow(s);
    expect(row.id).toBe("tradingview");
    expect(row.type).toBe("tradingview");
    expect(row.running && row.live && row.armed).toBe(true);
    expect(row.openPositions).toBe(1);
    expect(rowStatus(row).labelKey).toBe("botState.running");
    // A Live one after a restart: Start says it needs LIVE.
    const idle = tvBotRow(status({ tradingview: { ...tv, live: false } }));
    expect(idle.startBlockKey).toBe("botsList.blocked.liveArm");
  });

  // Owner check, 21 Oct: the All bots row read "TradingView" and "Capital 200.00 USDT".
  it("carries its own name, its budget as capital and its max size per alert", () => {
    const t = ((k: string) => `t:${k}`) as unknown as Parameters<typeof botRowName>[0];
    const named = { ...tv, capital: 200, tv: { ...tv.tv!, name: "deneme", budgetUsdt: 1000, maxSizeUsdt: 200 } };
    const row = tvBotRow(status({ tradingview: named }));
    expect(botRowName(t, row)).toBe("deneme");
    expect(row.capital).toBe(1000);
    expect(row.maxSizeUsdt).toBe(200);
    // A blank name falls back to the kind's name; a signal bot keeps its kind's name and capital.
    const blank = tvBotRow(status({ tradingview: { ...named, tv: { ...named.tv, name: "  " } } }));
    expect(botRowName(t, blank)).toBe("t:bots.kind.tradingview");
    const futures = { kind: "futures", exchangeId: "binance", capital: 20, leverage: 3, maxPositions: 3 } as BotConfig;
    const sig = signalBotRows(status({ futures })).find((r) => r.kind === "futures")!;
    expect(botRowName(t, sig)).toBe("t:bots.kind.futures");
    expect(sig.capital).toBe(20);
    expect(sig.maxSizeUsdt).toBeNull();
  });

  it("counts with the signal bots once it exists", () => {
    const futures = { kind: "futures", exchangeId: "binance", capital: 20, leverage: 3, maxPositions: 3 } as BotConfig;
    const s = status({ futures, futuresRunning: false, tradingview: tv, tradingviewRunning: true });
    const rows = allBotRows(s);
    const configured = rows.filter((r) => r.config);
    expect(configured.map((r) => r.kind)).toEqual(["futures", "tradingview"]);
    expect(configured.filter((r) => r.running).length).toBe(1);
    expect(rows.filter((r) => r.live && r.running).map((r) => r.type)).toEqual(["tradingview"]);
    // Not made yet: no row, nothing counted.
    expect(allBotRows(status({ futures })).some((r) => r.kind === "tradingview")).toBe(false);
    expect(signalBotRows(s).some((r) => r.kind === "tradingview")).toBe(false);
  });
});
