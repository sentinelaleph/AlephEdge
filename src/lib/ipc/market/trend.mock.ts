/**
 * DEV-ONLY browser preview of the market trend. Outside the screenshot
 * harness it answers that no trend is available (the preview reads no
 * market); inside it (?shot=, labelled "Sample data") it pictures one.
 */

import type { MarketTrend } from "./trend";

export function trend(): MarketTrend {
  if (!new URLSearchParams(window.location.search).has("shot")) throw "trendUnavailable";
  const now = Date.now();
  return { state: "down", sinceMs: now - 12 * 86_400_000, days: 12, close: 61_250, sma50: 64_480, gapPct: -5.01, switches365d: 9, checkedAtMs: now };
}
