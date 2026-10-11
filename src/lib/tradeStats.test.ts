import { describe, expect, it } from "vitest";
import { SAMPLE_HISTORY } from "@/dev/shot/shot.mock";
import type { TradeRecord } from "@/lib/ipc/trades/trades";
import { aggregateTrades, breakdown, cumulative, dailyNet, DAY_MS, streaks, utcDayStart } from "./tradeStats";

const DAY0 = Date.UTC(2026, 9, 1);
function tr(pnl: number, exit: string, at: number, extra: Partial<TradeRecord> = {}): TradeRecord {
  return { ...SAMPLE_HISTORY[0], id: at, pnlQuote: pnl, pnlUsdt: pnl, pnlPct: pnl, exitReason: exit, closedAt: at, ...extra };
}

describe("aggregateTrades", () => {
  // The same case as store/bot_stats.rs and trades.rs: the TS totals follow Rust's rules.
  const rows = [tr(2, "tp", DAY0 + 10), tr(-1, "sl", DAY0 + 20), tr(-3, "sl", DAY0 + 30), tr(5, "horizon", DAY0 + 40)];

  it("counts wins at the target and losses at the stop only", () => {
    const s = aggregateTrades(rows, DAY0 + 25);
    expect([s.totalTrades, s.wins, s.losses]).toEqual([4, 1, 2]);
    expect(s.winRate).toBeCloseTo(1 / 3);
  });

  it("nets, splits today, and measures drawdown on the running total", () => {
    const s = aggregateTrades(rows, DAY0 + 25);
    expect(s.netPnlQuote).toBe(3);
    expect(s.todayPnlQuote).toBe(2);
    expect(s.maxDrawdownQuote).toBe(4); // peak 2, trough -2
    expect(s.profitFactor).toBeCloseTo(7 / 4);
    expect(s.avgPnlQuote).toBeCloseTo(0.75);
    expect(s.best?.pnlQuote).toBe(5);
    expect(s.worst?.pnlQuote).toBe(-3);
  });

  it("is empty without trades", () => {
    const s = aggregateTrades([], DAY0);
    expect(s.totalTrades).toBe(0);
    expect(s.winRate).toBeNull();
    expect(s.best).toBeNull();
  });
});

describe("cumulative and daily", () => {
  it("starts at zero and runs in close order", () => {
    const pts = cumulative([tr(-1, "sl", DAY0 + 20), tr(2, "tp", DAY0 + 10)]);
    expect(pts.map((p) => p.value)).toEqual([0, 2, 1]);
  });

  it("fills days without trades with zero, UTC days", () => {
    const days = dailyNet([tr(2, "tp", DAY0 + 1000), tr(-1, "sl", DAY0 + 2 * DAY_MS + 5), tr(1, "tp", DAY0 + 2 * DAY_MS + 9)]);
    expect(days.map((d) => [d.day - DAY0, d.net, d.trades])).toEqual([
      [0, 2, 1],
      [DAY_MS, 0, 0],
      [2 * DAY_MS, 0, 2],
    ]);
    expect(utcDayStart(DAY0 + 5)).toBe(DAY0);
  });
});

describe("breakdown and streaks", () => {
  it("groups with record and net, largest result first", () => {
    const b = breakdown(
      [tr(2, "tp", 1, { botKind: "spot" }), tr(-5, "sl", 2, { botKind: "futures" }), tr(1, "tp", 3, { botKind: "spot" })],
      (x) => x.botKind,
    );
    expect(b.map((x) => [x.key, x.trades, x.wins, x.losses, x.net])).toEqual([
      ["futures", 1, 0, 1, -5],
      ["spot", 2, 2, 0, 3],
    ]);
  });

  it("tracks runs of gains and losses, a flat trade ending a run", () => {
    const s = streaks([tr(1, "tp", 1), tr(2, "tp", 2), tr(0, "breakeven", 3), tr(-1, "sl", 4), tr(-2, "sl", 5)]);
    expect(s).toEqual({ current: { kind: "loss", count: 2 }, longestGain: 2, longestLoss: 2 });
  });
});
