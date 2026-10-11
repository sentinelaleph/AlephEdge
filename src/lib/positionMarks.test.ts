import { describe, expect, it } from "vitest";
import { SAMPLE_MARKS, SAMPLE_POSITIONS } from "@/dev/shot/shot.mock";
import type { OpenPosition, PositionMark } from "@/lib/ipc/bot/bot";
import { peekOrder } from "@/pages/Dashboard/OpenPositionsPeek";
import { totalOf } from "@/pages/Bots/botMoney";
import { markKey, marksByPosition, openPnl, rangePosition } from "./positionMarks";

const long = { direction: "long" as const, entry: 100, sl: 90, tp: 120 };
const short = { direction: "short" as const, entry: 100, sl: 110, tp: 80 };

describe("rangePosition", () => {
  it("places a long's price and entry between the stop (0) and the target (1)", () => {
    expect(rangePosition(long, 90)).toEqual({ price: 0, entry: 1 / 3 });
    expect(rangePosition(long, 110)?.price).toBeCloseTo(2 / 3);
    expect(rangePosition(long, 130)?.price).toBe(1);
  });

  it("measures a short from its stop above to its target below", () => {
    expect(rangePosition(short, 110)?.price).toBe(0);
    expect(rangePosition(short, 90)?.price).toBeCloseTo(2 / 3);
    expect(rangePosition(short, 100)?.entry).toBeCloseTo(1 / 3);
  });

  it("has no range when the levels do not form one", () => {
    expect(rangePosition({ direction: "long", entry: 100, sl: 120, tp: 110 }, 100)).toBeNull();
  });
});

describe("openPnl", () => {
  const marks = marksByPosition({ marks: SAMPLE_MARKS });

  it("sums the priced positions and says how many are priced", () => {
    const o = openPnl(SAMPLE_POSITIONS, marks);
    expect(o).toMatchObject({ open: 3, priced: 3 });
    expect(o.pnlUsdt).toBeCloseTo(SAMPLE_MARKS.reduce((n, m) => n + m.pnlUsdt, 0));
  });

  it("never estimates an unpriced position", () => {
    const partial = new Map([...marks].slice(0, 1));
    const o = openPnl(SAMPLE_POSITIONS, partial);
    expect(o.priced).toBe(1);
    expect(o.pnlUsdt).toBeCloseTo(SAMPLE_MARKS[0].pnlUsdt);
  });
});

describe("Dashboard open positions", () => {
  it("lists the worst P&L first and unpriced positions last", () => {
    const marks = new Map<string, PositionMark>([
      [markKey("futures", "a"), { signalId: "a", botKind: "futures", price: 1, atMs: 0, pnlPct: 1, pnlUsdt: 5 }],
      [markKey("futures", "b"), { signalId: "b", botKind: "futures", price: 1, atMs: 0, pnlPct: -1, pnlUsdt: -7 }],
    ]);
    const pos = (id: string) => ({ ...SAMPLE_POSITIONS[0], signalId: id }) as OpenPosition;
    expect(peekOrder([pos("c"), pos("a"), pos("b")], marks).map((p) => p.signalId)).toEqual(["b", "a", "c"]);
  });
});

describe("bot total", () => {
  it("is realized plus open, and unknown while either part is", () => {
    expect(totalOf({ open: 2, openPnl: -3, realized: 10 })).toBe(7);
    expect(totalOf({ open: 0, openPnl: null, realized: 4 })).toBe(4);
    expect(totalOf({ open: 1, openPnl: null, realized: 4 })).toBeNull();
    expect(totalOf({ open: 0, openPnl: 0, realized: null })).toBeNull();
  });
});
