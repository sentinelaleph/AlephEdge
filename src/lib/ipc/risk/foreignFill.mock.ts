/**
 * DEV-ONLY in-memory foreign-trade watch for the browser preview. Reached
 * through `devMock`, so the released bundle drops this module. Outside the
 * screenshot harness it answers honestly that the preview cannot watch an
 * account; in the harness (?shot=, every screen labelled "Sample data") it
 * pictures one flagged fill for the FAQ.
 */

import type { ForeignFillStatus } from "./foreignFill";

function initial(): ForeignFillStatus {
  const shot = new URLSearchParams(window.location.search).has("shot");
  if (!shot) {
    return { liveBuild: false, enabled: false, watching: false, lastCheckMs: null, lastError: null, alarms: [] };
  }
  const now = Date.now();
  return {
    liveBuild: true,
    enabled: true,
    watching: true,
    lastCheckMs: now - 20_000,
    lastError: null,
    alarms: [
      { symbol: "XYZUSDT", tradeId: 4410023, orderId: 81730021, side: "BUY", qty: 52000, price: 0.0123, timeMs: now - 90_000, detectedMs: now - 40_000 },
    ],
  };
}

export const mock = (() => {
  let s: ForeignFillStatus | null = null;
  const get = () => (s ??= initial());
  const snap = (): Promise<ForeignFillStatus> => Promise.resolve(structuredClone(get()));
  return {
    status: snap,
    ack() {
      s = { ...get(), alarms: [] };
      return snap();
    },
    setEnabled(on: boolean) {
      s = { ...get(), enabled: on };
      return snap();
    },
  };
})();
