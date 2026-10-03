/**
 * Signal IPC bridge (mirrors src-tauri/src/signal). `Signal` fields stay
 * snake_case on purpose — they mirror the real Sentinel wire format exactly
 * (see sentinel-alephv2/frontend/src/types/signal.ts), unlike our own
 * camelCase-designed types.
 *
 * `npm run dev` gets a dev-only feed through `devMock`; the released bundle
 * has none.
 */

import { devMock, inTauri, invoke } from "../bridge";

type Mock = typeof import("./signal.mock");
const mock = <T>(run: (m: Mock) => Promise<T>) => devMock(() => import("./signal.mock"), run);

export type SignalDirection = "long" | "short";
/** Known values as of this writing — the wire field itself accepts any string the engine sends. */
export type SignalMode = "smc_only" | "ind_only" | "hybrid";

export interface ConfluenceItem {
  source: string;
  score: number;
  weighted: number;
}

export interface Signal {
  id: string;
  symbol: string;
  timeframe?: string;
  direction: SignalDirection;
  mode: string;
  entry: number;
  tp: number[];
  sl: number;
  confidence: number;
  regime?: string;
  confluence: ConfluenceItem[];
  /** Most-specific matched confluence combo ("stophunt_snap", …); what the bots' combo filter matches. */
  combo?: string | null;
  rr: number;
  investment_score?: number;
  expires_at: string;
  created_at: string;
  /** True when this signal replaced an earlier, regenerated one. */
  regenerated?: boolean;
  regenerated_at?: string;
  /** Snake_case: mirrors the Rust engine's management-plan struct, not a shape we designed. */
  management_plan?: {
    breakeven_at_r: number | null;
    partial_at_r: number | null;
    partial_fraction: number | null;
  } | null;
  /** Opaque debugging/telemetry payload; not rendered anywhere in the UI. */
  instrumentation?: Record<string, unknown> | null;
}

/** Mirrors Rust `StreamPhase` (signal/model.rs). */
export type StreamPhase = "idle" | "connecting" | "live" | "retrying" | "down";

export interface SignalHealthInfo {
  connected: boolean;
  /** Where the connection is; progress lives here, never in lastError. */
  phase: StreamPhase;
  /** Seconds out of "live" (since the connect request or the drop); absent while live/idle. */
  notLiveSecs?: number;
  lastSignalSecs?: number;
  latencyMs?: number;
  /** The last real failure (ticket/stream) or a code such as `noAccessToken`; absent while live. */
  lastError?: string;
}

export function signalConnect(): Promise<void> {
  return inTauri() ? invoke<void>("signal_connect") : mock((m) => m.mock.connect());
}

export function signalDisconnect(): Promise<void> {
  return inTauri() ? invoke<void>("signal_disconnect") : mock((m) => m.mock.disconnect());
}

/** Up to 500 recent signals, newest first. */
export function signalRecent(): Promise<Signal[]> {
  return inTauri() ? invoke<Signal[]>("signal_recent") : mock((m) => m.mock.recent());
}

export function signalHealth(): Promise<SignalHealthInfo> {
  return inTauri() ? invoke<SignalHealthInfo>("signal_health") : mock((m) => m.mock.health());
}
