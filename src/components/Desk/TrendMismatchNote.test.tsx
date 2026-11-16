// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import i18n from "@/i18n";
import type { MarketTrend } from "@/lib/ipc/market/trend";

const DOWN: MarketTrend = {
  state: "down",
  sinceMs: 0,
  days: 22,
  close: 64150,
  sma50: 80225,
  gapPct: -20,
  switches365d: 14,
  checkedAtMs: Date.now(),
};
vi.mock("@/lib/ipc/market/useMarketTrend", () => ({ useMarketTrend: () => ({ trend: DOWN, error: null }) }));

import { TrendMismatchNote } from "./MarketTrend";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;

async function mount(node: React.ReactNode) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root!.render(<>{node}</>));
  return host;
}

beforeAll(async () => {
  await i18n.changeLanguage("en");
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

describe("long-in-downtrend warning offers the grid gate (15 Nov 2026)", () => {
  it("switches the gate on from the warning", async () => {
    const set = vi.fn();
    const el = await mount(<TrendMismatchNote long short={false} gate={{ on: false, set }} />);
    const button = [...el.querySelectorAll("button")].find((b) => b.textContent === "Hold new cycles until it turns up");
    expect(button).toBeTruthy();
    await act(async () => button!.click());
    expect(set).toHaveBeenCalledWith(true);
  });

  it("says the cycles wait once the gate is on", async () => {
    const el = await mount(<TrendMismatchNote long short={false} gate={{ on: true, set: vi.fn() }} />);
    expect(el.textContent).toContain("New cycles wait until it turns up.");
    expect(el.querySelector("button")).toBeNull();
  });

  it("offers nothing without a gate (DCA, signal bots)", async () => {
    const el = await mount(<TrendMismatchNote long short={false} />);
    expect(el.querySelector("button")).toBeNull();
    expect(el.textContent).not.toContain("turns up");
  });
});
