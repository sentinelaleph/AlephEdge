import { describe, expect, it } from "vitest";
import { newBacktestRunToken, progressOf, type BacktestProgress } from "./backtest";

describe("backtest progress run token", () => {
  it("a listener only sees the progress of its own run", () => {
    const a = newBacktestRunToken();
    const b = newBacktestRunToken();
    expect(a).not.toBe(b);
    expect(a).toMatch(/^[A-Za-z0-9_-]{1,64}$/);
    const seen: BacktestProgress[] = [];
    const handler = progressOf(a, (p) => seen.push(p));
    handler({ runToken: b, done: 3, total: 9 });
    handler({ runToken: a, done: 1, total: 4 });
    handler({ runToken: "", done: 2, total: 2 });
    expect(seen).toEqual([{ runToken: a, done: 1, total: 4 }]);
  });
});
