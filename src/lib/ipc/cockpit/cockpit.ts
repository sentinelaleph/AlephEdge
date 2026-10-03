/**
 * Cockpit-probe IPC bridge (mirrors src-tauri/src/cockpit.rs). On-demand
 * connectivity checks the two cockpit buttons trigger. `npm run dev` has no
 * backend to probe, so the dev-only preview reports the probe as unavailable
 * instead of inventing a healthy result.
 */

import { devMock, inTauri, invoke } from "../bridge";

const NO_BACKEND: PingResult = { ok: false, detail: "browser preview: no backend" };
const preview = () => devMock(() => Promise.resolve(null), () => Promise.resolve(NO_BACKEND));

export interface PingResult {
  ok: boolean;
  latencyMs?: number;
  /** A stable code (`cockpitStreamReady`, `cockpitTicketFailed|415`) rendered through localizeError. */
  detail: string;
}

/** Probe the connected exchange's reachability + latency. */
export function cockpitExchange(exchangeId: string): Promise<PingResult> {
  return inTauri()
    ? invoke<PingResult>("cockpit_exchange", { exchangeId })
    : preview();
}

/** Probe Sentinel (ribqa.com): reachable → signed in → stream ticket OK. */
export function cockpitSentinel(): Promise<PingResult> {
  return inTauri()
    ? invoke<PingResult>("cockpit_sentinel")
    : preview();
}
