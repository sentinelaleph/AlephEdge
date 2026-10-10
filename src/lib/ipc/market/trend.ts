/**
 * BTC's slow trend (mirrors src-tauri/src/app/market_trend.rs): daily close vs
 * its 50-day average, 2 closes to switch. Information only: no bot reads it.
 */

import { devMock, inTauri, invoke } from "../bridge";

export type TrendState = "up" | "down";

export interface MarketTrend {
  state: TrendState;
  sinceMs: number;
  days: number;
  close: number;
  sma50: number;
  gapPct: number;
  switches365d: number;
  checkedAtMs: number;
}

export function marketTrend(): Promise<MarketTrend> {
  if (inTauri()) return invoke<MarketTrend>("market_trend");
  return devMock(
    () => import("./trend.mock"),
    async (m) => m.trend(),
  );
}
