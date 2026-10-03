/**
 * DEV-ONLY in-memory risk settings for the browser preview (no Rust in
 * `npm run dev`). Reached through `devMock`, so the released bundle drops
 * this module. It mirrors the Rust clamp; it is not the authority.
 */

import { RISK_LIMITS_TABLE, type RiskLevel, type RiskState } from "./risk";

export const mock = (() => {
  let level: RiskLevel = "cautious";
  let balance = 1000;
  let closeOnStop = true;
  let dailyLossOverridePct: number | null = null;
  const state = (): Promise<RiskState> => {
    const limits = { level, ...RISK_LIMITS_TABLE[level] };
    // Mirrors the Rust clamp exactly: a looser number never widens the stop,
    // and a nonsense number falls back to the table rather than halting.
    const valid =
      dailyLossOverridePct !== null && Number.isFinite(dailyLossOverridePct) && dailyLossOverridePct > 0;
    return Promise.resolve({
      limits,
      balance,
      closeOnStop,
      maxCapitalQuote: (balance * limits.maxCapitalPct) / 100,
      allowsPump: level === "ambitious" || level === "greedy",
      dailyLossOverridePct,
      effectiveDailyLossPct: valid
        ? Math.min(dailyLossOverridePct as number, limits.dailyLossLimitPct)
        : limits.dailyLossLimitPct,
      dailyLossOverrideCapped: valid && (dailyLossOverridePct as number) > limits.dailyLossLimitPct,
    });
  };
  return {
    state,
    set(patch: {
      level?: RiskLevel;
      balance?: number;
      closeOnStop?: boolean;
      dailyLossOverridePct?: number | null;
    }) {
      if (patch.level) level = patch.level;
      if (patch.balance !== undefined) balance = Math.max(1, patch.balance);
      if (patch.closeOnStop !== undefined) closeOnStop = patch.closeOnStop;
      if (patch.dailyLossOverridePct !== undefined) dailyLossOverridePct = patch.dailyLossOverridePct;
      return state();
    },
  };
})();
