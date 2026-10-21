// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import i18n from "@/i18n";
import type { BudgetFloor } from "@/lib/ipc/strategy/strategy";
import { budgetErrors, liveLine, minimumLine, saveMinimum, type FloorState } from "./budgetFloor";
import { NumField } from "./formControls";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const t = i18n.t.bind(i18n);
const floor = (over: Partial<BudgetFloor> = {}): FloorState => ({
  status: "ready",
  floor: { minBudget: 176.98, orders: 8, minOrder: 5, symbolRules: "ok", symbolMinOrder: 5, liveMinBudget: 176.98, ...over },
});

beforeEach(async () => {
  await i18n.changeLanguage("en");
});

describe("minimum budget line", () => {
  it("states the minimum, the order count and the smallest order", () => {
    expect(minimumLine(t, floor(), "en-US", "ALGOUSDT")).toBe("Minimum 176.98 USDT · orders: 8, smallest 5 USDT (Binance minimum)");
  });

  it("reads the same in Turkish", async () => {
    await i18n.changeLanguage("tr");
    expect(minimumLine(t, floor(), "tr-TR", "ALGOUSDT")).toBe("En az 176,98 USDT · 8 emir, en küçük emir 5 USDT (Binance alt sınırı)");
    expect(budgetErrors(t, floor(), "tr-TR").empty).toBe("Bütçe girin (en az 176,98 USDT)");
    expect(budgetErrors(t, floor(), "tr-TR").below).toBe("En az 176,98 USDT gerekli: 8 emre bölününce en küçük emir 5 USDT altına düşer");
  });

  it("shows no number while loading or when the check failed", () => {
    expect(minimumLine(t, { status: "loading" }, "en-US", "BTCUSDT")).toBe("Minimum budget: reading Binance rules for BTCUSDT");
    expect(minimumLine(t, { status: "loading" }, "en-US", null)).toBe("Minimum budget: calculating");
    expect(minimumLine(t, { status: "failed" }, "en-US", null)).not.toMatch(/\d/);
    expect(saveMinimum({ status: "loading" })).toBeNull();
    expect(saveMinimum({ status: "failed" })).toBeNull();
    const e = budgetErrors(t, { status: "failed" }, "en-US");
    expect(e.empty).toBe("Enter a budget");
    expect(e.notANumber).toBe("Not a valid number for this field.");
    expect(e.below).toBeNull();
  });

  it("asks for bigger orders when the budget cannot fix fixed sizes", () => {
    expect(minimumLine(t, floor({ minBudget: null }), "en-US", null)).toBe("Each order must be at least 5 USDT: raise the order sizes first");
  });
});

describe("Live minimum line", () => {
  it("adds the symbol's own Binance minimum when it is above 5 USDT", () => {
    const btc = floor({ symbolMinOrder: 50, liveMinBudget: 1769.8 });
    expect(liveLine(t, btc, "en-US", "BTCUSDT")).toBe("Live: minimum 1,769.8 USDT. Smallest BTCUSDT order on Binance: 50 USDT");
  });

  it("says nothing more for a 5 USDT symbol, spot or another exchange", () => {
    expect(liveLine(t, floor(), "en-US", "ALGOUSDT")).toBeNull();
    expect(liveLine(t, floor({ symbolRules: "notApplicable", symbolMinOrder: null, liveMinBudget: null }), "en-US", "BTCUSDT")).toBeNull();
  });

  it("says the Live minimum is unknown when Binance did not answer", () => {
    const down = floor({ symbolRules: "unavailable", symbolMinOrder: null, liveMinBudget: null });
    expect(liveLine(t, down, "en-US", "BTCUSDT")).toBe("Live minimum unknown: Binance rules for BTCUSDT could not be read");
    expect(liveLine(t, { status: "loading" }, "en-US", "BTCUSDT")).toBeNull();
  });
});

describe("Budget field errors", () => {
  it("carries the minimum in every message", () => {
    const e = budgetErrors(t, floor(), "en-US");
    expect(e.empty).toBe("Enter a budget (minimum 176.98 USDT)");
    expect(e.notANumber).toBe("Not a valid number for this field. Minimum 176.98 USDT.");
    expect(e.below).toBe("Minimum 176.98 USDT needed: split over 8 orders, the smallest falls under 5 USDT");
  });
});

describe("NumField", () => {
  let root: Root | null = null;
  let host: HTMLDivElement | null = null;
  afterEach(async () => {
    await act(async () => root?.unmount());
    host?.remove();
  });

  it("shows the empty text for an empty field, the invalid text for letters, and the placeholder", async () => {
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    await act(async () =>
      root!.render(<NumField label="Budget" value={100} onChange={() => {}} onInvalid={() => {}} invalidText="bad number" emptyText="enter a budget" placeholder="176.98" />),
    );
    const input = host.querySelector("input")!;
    expect(input.placeholder).toBe("176.98");
    const type = async (v: string) =>
      act(async () => {
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, v);
        input.dispatchEvent(new Event("input", { bubbles: true }));
      });
    await type("");
    expect(host.querySelector('[role="alert"]')?.textContent).toBe("enter a budget");
    await type("abc");
    expect(host.querySelector('[role="alert"]')?.textContent).toBe("bad number");
    await type("200");
    expect(host.querySelector('[role="alert"]')).toBeNull();
  });
});
