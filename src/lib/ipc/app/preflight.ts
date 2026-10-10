/**
 * Live readiness check (mirrors src-tauri/src/app/preflight.rs). Read-only:
 * Rust reads the clock, the vaulted key, the futures account and the desk
 * state and returns one verdict per check. No order is sent.
 */

import { devMock, inTauri, invoke } from "../bridge";

export type PreflightVerdict = "ok" | "warn" | "fail";

export type PreflightCheckId =
  | "build"
  | "endpoint"
  | "venue"
  | "key"
  | "clock"
  | "account"
  | "positionMode"
  | "positions"
  | "killSwitch"
  | "btcRegime"
  | "membership"
  | "stream"
  | "liveBots";

export interface PreflightCheck {
  id: PreflightCheckId;
  verdict: PreflightVerdict;
  /** Reason (`preflight.codes.*`); `binanceRefused<code>` carries Binance's code. */
  code: string | null;
  /** A number or list shown as-is. */
  value: string | null;
}

export interface Preflight {
  checkedAtMs: number;
  /** The exchange whose key and account were read. */
  venue: string;
  checks: PreflightCheck[];
  overall: PreflightVerdict;
}

/** Reads the key and account of `venue` (Binance when omitted). */
export function livePreflight(venue?: string): Promise<Preflight> {
  if (inTauri()) return invoke<Preflight>("live_preflight", { venue: venue ?? null });
  return devMock(
    () => import("./preflight.mock"),
    async (m) => m.preflight(),
  );
}
