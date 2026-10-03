/**
 * Health snapshot types (mirror src-tauri/src/health.rs) + a client that reads
 * them from the Rust core. In a plain browser (`npm run dev`) a dev-only
 * preview answers "unknown" for every reading; the released bundle has none.
 */

import { devMock, inTauri, invoke } from "../bridge";

export type HealthLevel = "ok" | "warn" | "down" | "unknown";
export type VaultState = "locked" | "unlocked" | "absent";
export type MembershipState = "active" | "expiring" | "inactive" | "unknown";

export interface ExchangeHealth {
  id: string;
  name: string;
  level: HealthLevel;
  latencyMs: number | null;
}

export interface SignalHealth {
  level: HealthLevel;
  connected: boolean;
  lastSignalSecs: number | null;
  latencyMs: number | null;
}

export interface BtcPulse {
  trend: "bullish" | "bearish" | "neutral";
  strength: number;
  phase: string;
  price: number;
  changePct: number;
  sparkline: number[];
}

export interface HealthSnapshot {
  exchanges: ExchangeHealth[];
  signal: SignalHealth;
  btc: BtcPulse;
  vault: VaultState;
  membership: MembershipState;
  generatedAt: number;
}

export async function fetchHealthSnapshot(): Promise<HealthSnapshot> {
  if (inTauri()) {
    return invoke<HealthSnapshot>("get_health_snapshot");
  }
  return devMock(() => import("./health.mock"), (m) => m.snapshot());
}
