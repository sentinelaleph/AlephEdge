import { describe, expect, it } from "vitest";
import { ladder, medianRr, movePct } from "./signalLevels";

describe("movePct", () => {
  it("is positive toward a long's target and negative toward its stop", () => {
    const s = { direction: "long" as const, entry: 100 };
    expect(movePct(s, 110)).toBeCloseTo(10);
    expect(movePct(s, 95)).toBeCloseTo(-5);
  });

  it("flips the sign for a short", () => {
    const s = { direction: "short" as const, entry: 100 };
    expect(movePct(s, 90)).toBeCloseTo(10);
    expect(movePct(s, 104)).toBeCloseTo(-4);
  });

  it("is 0 without a usable entry", () => {
    expect(movePct({ direction: "long", entry: 0 }, 10)).toBe(0);
  });
});

describe("ladder", () => {
  it("places the stop at 0, the furthest target at 1 and the entry between", () => {
    const pts = ladder({ direction: "long", entry: 100, sl: 95, tp: [105, 110, 115] })!;
    expect(pts.map((p) => p.kind)).toEqual(["sl", "entry", "tp", "tp", "tp"]);
    expect(pts[0].at).toBe(0);
    expect(pts[1].at).toBeCloseTo(0.25);
    expect(pts[4].at).toBeCloseTo(1);
    expect(pts[2].n).toBe(1);
  });

  it("reads a short's levels from the short side", () => {
    const pts = ladder({ direction: "short", entry: 100, sl: 105, tp: [90] })!;
    expect(pts[0].pct).toBeCloseTo(-5);
    expect(pts[2].pct).toBeCloseTo(10);
    expect(pts[1].at).toBeCloseTo(5 / 15);
  });

  it("is null when the levels cannot form a scale", () => {
    expect(ladder({ direction: "long", entry: 100, sl: 95, tp: [] })).toBeNull();
    expect(ladder({ direction: "long", entry: 100, sl: 0, tp: [110] })).toBeNull();
    // A target on the stop's side: no stop-to-target scale.
    expect(ladder({ direction: "long", entry: 100, sl: 95, tp: [94] })).toBeNull();
  });
});

describe("medianRr", () => {
  it("takes the middle value, or the mean of the middle two", () => {
    expect(medianRr([{ rr: 3 }, { rr: 1 }, { rr: 2 }])).toBe(2);
    expect(medianRr([{ rr: 1 }, { rr: 4 }, { rr: 2 }, { rr: 3 }])).toBe(2.5);
  });

  it("skips non-finite values and is null for none", () => {
    expect(medianRr([{ rr: Number.NaN }, { rr: 1.5 }])).toBe(1.5);
    expect(medianRr([])).toBeNull();
  });
});
