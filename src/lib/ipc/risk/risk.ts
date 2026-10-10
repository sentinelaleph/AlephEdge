/**
 * Risk IPC bridge (mirrors src-tauri/src/risk). The level + simulated balance
 * are global; their limits bound every bot. Browser fallback keeps an
 * in-memory state so the selector is reviewable in `npm run dev`.
 */

import { devMock, inTauri, invoke } from "../bridge";

type Mock = typeof import("./risk.mock");
const mock = <T>(run: (m: Mock) => Promise<T>) => devMock(() => import("./risk.mock"), run);

export type RiskLevel = "cautious" | "calm" | "balanced" | "ambitious" | "greedy";

export const RISK_LEVELS: RiskLevel[] = ["cautious", "calm", "balanced", "ambitious", "greedy"];

export interface RiskLimits {
  level: RiskLevel;
  maxLeverage: number;
  maxConcurrentPositions: number;
  maxCapitalPct: number;
  dailyLossLimitPct: number;
  frThresholdPct: number;
  maxDepthSharePct: number;
}

export interface RiskState {
  limits: RiskLimits;
  /** Simulated account equity (base for the % caps). */
  balance: number;
  /** Close open positions when the daily-loss stop trips (PRD §5.3). */
  closeOnStop: boolean;
  /** balance × maxCapitalPct / 100 — max capital per position. */
  maxCapitalQuote: number;
  /** The tolerance the user typed, echoed back verbatim (null = not set). */
  dailyLossOverridePct: number | null;
  /**
   * The stop actually in force = min(level cap, typed tolerance). Show THIS as
   * "your daily stop" — `limits.dailyLossLimitPct` is only the ceiling.
   */
  effectiveDailyLossPct: number;
  /** True when the typed tolerance was looser than the level allows. */
  dailyLossOverrideCapped: boolean;
}

export function riskGet(): Promise<RiskState> {
  return inTauri() ? invoke<RiskState>("risk_get") : mock((m) => m.mock.state());
}

export function riskSetLevel(level: RiskLevel): Promise<RiskState> {
  return inTauri() ? invoke<RiskState>("risk_set_level", { level }) : mock((m) => m.mock.set({ level }));
}

export function riskSetBalance(balance: number): Promise<RiskState> {
  return inTauri() ? invoke<RiskState>("risk_set_balance", { balance }) : mock((m) => m.mock.set({ balance }));
}

export function riskSetCloseOnStop(close: boolean): Promise<RiskState> {
  return inTauri()
    ? invoke<RiskState>("risk_set_close_on_stop", { close })
    : mock((m) => m.mock.set({ closeOnStop: close }));
}

/**
 * Sets the user's own daily-loss tolerance; `null` clears it.
 *
 * It can only ever TIGHTEN the level's cap — the clamp lives in Rust
 * (`RiskConfig::effective_daily_loss_pct`) so it cannot be bypassed from here.
 */
export function riskSetDailyLoss(pct: number | null): Promise<RiskState> {
  return inTauri()
    ? invoke<RiskState>("risk_set_daily_loss", { pct })
    : mock((m) => m.mock.set({ dailyLossOverridePct: pct }));
}

/**
 * Mirror of the Rust limits table (PRD §5.3), kept by hand. It serves the
 * browser mock and the Greedy confirmation, which must state Greedy's limits
 * BEFORE the level is chosen (the server only reports the current level's).
 */
export const RISK_LIMITS_TABLE: Record<RiskLevel, Omit<RiskLimits, "level">> = {
  cautious: { maxLeverage: 2, maxConcurrentPositions: 3, maxCapitalPct: 2, dailyLossLimitPct: 2, frThresholdPct: 0.05, maxDepthSharePct: 5 },
  calm: { maxLeverage: 3, maxConcurrentPositions: 5, maxCapitalPct: 4, dailyLossLimitPct: 4, frThresholdPct: 0.05, maxDepthSharePct: 5 },
  balanced: { maxLeverage: 5, maxConcurrentPositions: 8, maxCapitalPct: 6, dailyLossLimitPct: 6, frThresholdPct: 0.1, maxDepthSharePct: 10 },
  ambitious: { maxLeverage: 10, maxConcurrentPositions: 12, maxCapitalPct: 10, dailyLossLimitPct: 10, frThresholdPct: 0.1, maxDepthSharePct: 10 },
  greedy: { maxLeverage: 20, maxConcurrentPositions: 20, maxCapitalPct: 15, dailyLossLimitPct: 15, frThresholdPct: 0.15, maxDepthSharePct: 20 },
};
