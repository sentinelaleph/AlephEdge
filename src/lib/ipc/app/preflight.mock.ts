/**
 * DEV-ONLY browser preview for the live readiness check. Reached through
 * `devMock`, so the released bundle drops this module. Outside the
 * screenshot harness it answers honestly that the preview cannot trade; in
 * the harness (?shot=, every screen labelled "Sample data") it pictures a
 * completed check for the FAQ.
 */

import type { Preflight } from "./preflight";

export function preflight(): Preflight {
  if (!new URLSearchParams(window.location.search).has("shot")) {
    return { checkedAtMs: Date.now(), venue: "binance", overall: "fail", checks: [{ id: "build", verdict: "fail", code: "buildOff", value: null }] };
  }
  return {
    checkedAtMs: Date.now(),
    venue: "binance",
    overall: "warn",
    checks: [
      { id: "build", verdict: "ok", code: null, value: null },
      { id: "endpoint", verdict: "ok", code: "production", value: null },
      { id: "venue", verdict: "ok", code: "venueDryRun", value: null },
      { id: "key", verdict: "ok", code: null, value: null },
      { id: "clock", verdict: "ok", code: null, value: "-42 ms" },
      { id: "account", verdict: "ok", code: null, value: "912.40 / 1000.00 USDT" },
      { id: "positionMode", verdict: "ok", code: "oneWay", value: null },
      { id: "positions", verdict: "warn", code: "foreignPositions", value: "ETHUSDT" },
      { id: "killSwitch", verdict: "ok", code: null, value: null },
      { id: "btcRegime", verdict: "ok", code: "normal", value: null },
      { id: "membership", verdict: "ok", code: null, value: null },
      { id: "stream", verdict: "ok", code: null, value: null },
      { id: "liveBots", verdict: "ok", code: null, value: "1 + 0" },
    ],
  };
}
