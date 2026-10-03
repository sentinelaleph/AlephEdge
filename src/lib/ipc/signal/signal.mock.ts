/**
 * DEV-ONLY signal feed for the browser preview (no Rust in `npm run dev`).
 * Reached through `devMock`, so the released bundle drops this module. Never
 * a real signal source.
 */

import type { Signal } from "./signal";

export const mock = (() => {
  let connected = false;
  const seed: Signal[] = [
    {
      id: "mock-1",
      symbol: "BTCUSDT",
      direction: "long",
      mode: "hybrid",
      entry: 63_050,
      tp: [64_200],
      sl: 62_400,
      confidence: 0.78,
      regime: "Bull",
      confluence: [],
      rr: 1.8,
      expires_at: new Date(Date.now() + 15 * 60_000).toISOString(),
      created_at: new Date().toISOString(),
    },
  ];
  return {
    connect: () => {
      connected = true;
      return Promise.resolve();
    },
    disconnect: () => {
      connected = false;
      return Promise.resolve();
    },
    recent: () => Promise.resolve(connected ? seed : []),
    health: () =>
      Promise.resolve({
        connected,
        phase: connected ? ("live" as const) : ("idle" as const),
        lastSignalSecs: connected ? 12 : undefined,
      }),
  };
})();
