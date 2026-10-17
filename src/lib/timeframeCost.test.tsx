// @vitest-environment jsdom
import { act } from "react";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import i18n from "@/i18n";
import { BREAKEVEN_HIT_RATE_PCT, shortTimeframeHitRate, TimeframeCostNote } from "./timeframeCost";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;

async function render(tf: string) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root!.render(<TimeframeCostNote timeframe={tf} />));
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

describe("short-timeframe cost warning", () => {
  it("carries the lab's break-even hit rates at 0.12% round trip", () => {
    expect(BREAKEVEN_HIT_RATE_PCT).toEqual({ "15m": 68.6, "30m": 63.3, "1h": 59.4, "4h": 54.6 });
    expect(shortTimeframeHitRate("15m")).toBe(68.6);
    expect(shortTimeframeHitRate("30m")).toBe(63.3);
    for (const tf of ["1m", "5m", "1h", "4h", "1d", ""]) expect(shortTimeframeHitRate(tf), tf).toBeNull();
  });

  it("shows one line with the required hit rate on 15m and 30m", async () => {
    const a = await render("15m");
    expect(a.querySelector("[role=note]")?.textContent).toBe(
      "15m: trading every candle needs a 68.6% hit rate to break even at 0.12% round-trip cost. No tested setup reached it.",
    );
    await act(async () => root?.unmount());
    host?.remove();
    const b = await render("30m");
    expect(b.textContent).toContain("30m: trading every candle needs a 63.3% hit rate");
  });

  it("shows nothing on 1h, 4h and 1d", async () => {
    for (const tf of ["1h", "4h", "1d"]) {
      const el = await render(tf);
      expect(el.textContent, tf).toBe("");
      await act(async () => root?.unmount());
      host?.remove();
    }
    root = null;
    host = null;
  });

  it("the Backtest interval choice carries the warning", () => {
    const src = readFileSync(join(__dirname, "../pages/Backtest/BacktestPage.tsx"), "utf8");
    expect(src).toMatch(/<TimeframeCostNote timeframe=\{interval\} \/>/);
  });
});
