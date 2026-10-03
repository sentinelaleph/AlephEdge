/**
 * DEV-ONLY browser preview for the health snapshot (no Rust in `npm run dev`).
 * Reached through `devMock`, so the released bundle drops this module. A
 * plain browser has no exchange, stream or BTC feed behind it, so every
 * reading here is "unknown": the preview never invents a latency or a price.
 */

import type { HealthSnapshot } from "./health";

export function snapshot(): Promise<HealthSnapshot> {
  return Promise.resolve({
    exchanges: [{ id: "binance", name: "Binance", level: "unknown", latencyMs: null }],
    signal: { level: "unknown", connected: false, lastSignalSecs: null, latencyMs: null },
    btc: { trend: "neutral", strength: 0, phase: "", price: 0, changePct: 0, sparkline: [] },
    vault: "locked",
    membership: "unknown",
    generatedAt: Date.now(),
  });
}
