import { describe, expect, it } from "vitest";
import { summarizeExchanges } from "./StatusBar";
import { badgeText } from "./Sidebar";

describe("status bar exchange summary", () => {
  it("folds the list into worst level, ok count and max latency", () => {
    const s = summarizeExchanges([
      { id: "binance", name: "Binance", level: "ok", latencyMs: 235 },
      { id: "okx", name: "OKX", level: "ok", latencyMs: 422 },
      { id: "bitso", name: "Bitso", level: "warn", latencyMs: null },
    ]);
    expect(s).toEqual({ total: 3, ok: 2, worst: "warn", maxLatencyMs: 422 });
  });

  it("an empty list is unknown, not ok", () => {
    expect(summarizeExchanges([]).worst).toBe("unknown");
  });

  it("down outranks warn and unknown", () => {
    const s = summarizeExchanges([
      { id: "a", name: "A", level: "unknown", latencyMs: null },
      { id: "b", name: "B", level: "down", latencyMs: null },
      { id: "c", name: "C", level: "warn", latencyMs: 10 },
    ]);
    expect(s.worst).toBe("down");
  });
});

describe("sidebar badge text", () => {
  it("names what a count counts and every dot", () => {
    expect(badgeText({ count: 2, countLabel: "Running 2" })).toBe("Running 2");
    expect(badgeText({ dots: [{ tone: "warn", label: "No exchange key stored" }] })).toBe("No exchange key stored");
    expect(
      badgeText({ count: 1, countLabel: "Open 1", chips: [{ tone: "real", label: "R", title: "Real exchange positions open" }] }),
    ).toBe("Open 1 · Real exchange positions open");
  });

  it("a zero count is no badge", () => {
    expect(badgeText({ count: 0, countLabel: "Running 0" })).toBe("");
    expect(badgeText(undefined)).toBe("");
  });
});
