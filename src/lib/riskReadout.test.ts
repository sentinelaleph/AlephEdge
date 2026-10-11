import { describe, expect, it } from "vitest";
import { SAMPLE_AT_STOP, SAMPLE_POSITIONS } from "@/dev/shot/shot.mock";
import type { StrategyRiskView } from "@/lib/ipc/strategy/strategy";
import { stopDistancePct } from "@/pages/Risk/OpenRiskTable";
import { dailyRoom, stopRisk, stopsWouldTrip, strategyRoom } from "./riskReadout";

describe("daily stop room", () => {
  it("is the limit plus today's net: the engine trips at today <= -limit", () => {
    expect(dailyRoom(2000, 3, 0)).toEqual({ limit: 60, used: 0, left: 60 });
    expect(dailyRoom(2000, 3, 8.45).left).toBeCloseTo(68.45);
    expect(dailyRoom(2000, 3, -20)).toEqual({ limit: 60, used: 20, left: 40 });
    expect(dailyRoom(2000, 3, -75).left).toBe(0);
  });

  it("says whether every stop hitting would reach it", () => {
    const room = dailyRoom(2000, 3, 0);
    expect(stopsWouldTrip(-59.99, room)).toBe(false);
    expect(stopsWouldTrip(-60, room)).toBe(true);
    expect(stopsWouldTrip(12, room)).toBe(false);
  });
});

describe("stop risk", () => {
  it("sums each position at its current stop, with the capital in use", () => {
    const r = stopRisk(SAMPLE_POSITIONS, SAMPLE_AT_STOP);
    expect(r.positions).toBe(3);
    expect(r.atStops).toBeCloseTo(SAMPLE_AT_STOP.reduce((n, a) => n + a.pnlUsdt, 0));
    expect(r.capital).toBe(SAMPLE_POSITIONS.reduce((n, p) => n + p.capital, 0));
  });

  it("measures the distance to the stop on its own side", () => {
    expect(stopDistancePct({ direction: "long" }, 100, 95)).toBeCloseTo(5);
    expect(stopDistancePct({ direction: "short" }, 100, 104)).toBeCloseTo(4);
  });
});

describe("DCA/Grid room", () => {
  it("reads the budget cap and the breaker from the strategy risk view", () => {
    const r = strategyRoom({
      balance: 10_000,
      budgetCapPct: 40,
      reservedBudget: 1_000,
      peakPnlQuote: 30,
      pnlQuote: 20,
      breakerBudget: 1_800,
      portfolioDdPct: 15,
    } as StrategyRiskView);
    expect(r).toEqual({ cap: 4_000, reserved: 1_000, free: 3_000, ddUsed: 10, ddLimit: 270, ddLeft: 260 });
  });
});
