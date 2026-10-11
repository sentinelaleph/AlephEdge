import { describe, expect, it } from "vitest";
import { sumMoney, totalOf } from "./botMoney";

describe("sumMoney", () => {
  it("adds open counts, open P&L and realized across bots", () => {
    const m = sumMoney([
      { open: 2, openPnl: 5, realized: 10 },
      { open: 1, openPnl: -2, realized: -4 },
    ]);
    expect(m).toEqual({ open: 3, openPnl: 3, realized: 6 });
    expect(totalOf(m)).toBe(9);
  });

  it("ignores the open P&L of a bot with nothing open", () => {
    expect(sumMoney([{ open: 0, openPnl: null, realized: 1 }, { open: 1, openPnl: 2, realized: 0 }]).openPnl).toBe(2);
  });

  it("keeps a part unknown when any bot's part is unknown", () => {
    const m = sumMoney([
      { open: 1, openPnl: null, realized: 1 },
      { open: 0, openPnl: 0, realized: null },
    ]);
    expect(m.openPnl).toBeNull();
    expect(m.realized).toBeNull();
    expect(totalOf(m)).toBeNull();
  });
});
