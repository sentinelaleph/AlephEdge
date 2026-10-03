/**
 * Client-side mirror of src-tauri/src/bot/engine/sizing.rs's `size_position`,
 * used ONLY to render a live worked example while configuring a bot — never
 * to compute a real size (that stays server-side, on the real signal's entry
 * and stop). Keep this in lockstep with the Rust function by hand; there is
 * no shared source of truth across the language boundary.
 */

/** Round-trip taker fees the engine and Sentinel's ledger charge (0.10%). */
export const ROUND_TRIP_FEE_FRAC = 0.001;
/** Funding allowance the engine and Sentinel's ledger charge (0.02%). */
export const FUNDING_COST_FRAC = 0.0002;
/** Round-trip cost added to the stop distance: fees + funding. */
export const ROUND_TRIP_COST_FRAC = ROUND_TRIP_FEE_FRAC + FUNDING_COST_FRAC;
export const MIN_RISK_PCT = 0.1;
export const MAX_RISK_PCT = 5.0;
export const DEFAULT_RISK_PCT = 1.0;

/**
 * The two hypothetical stop distances the bot form's worked example sizes
 * (3% and 7%: the ledger's typical range at the live 2.5xATR stop). Preview
 * only, never a real position's stop.
 */
export const EXAMPLE_STOP_FRACS = [0.03, 0.07] as const;

export interface SizeEstimate {
  /** Position notional in USDT. */
  notional: number;
  /** notional / capital. */
  effectiveLeverage: number;
  /** The leverage cap bound the size — actual risk sits below the declared %. */
  riskCapped: boolean;
  /** What the stop actually costs at this size, in USDT. */
  costUsdt: number;
  /** What the stop actually costs as % of capital (== riskPct unless capped). */
  costPctOfCapital: number;
}

/**
 * Sizes a hypothetical position for a worked example: capital and leverage
 * cap from the form, a declared risk %, and a hypothetical stop distance
 * (e.g. 0.03 for a 3% stop). Mirrors `size_position`'s risk-mode branch
 * exactly, including the risk clamp and the round-trip cost folded into the
 * stop distance.
 */
export function estimateRiskSize(
  capital: number,
  leverageCap: number,
  riskPct: number,
  stopFrac: number,
): SizeEstimate {
  const cap = capital * leverageCap;
  // size_position clamps the declared risk again; mirror it so the example
  // never shows a size the engine would not take.
  const risk = Math.min(MAX_RISK_PCT, Math.max(MIN_RISK_PCT, riskPct));
  const riskUsdt = (capital * risk) / 100;
  const wanted = stopFrac > 0 ? riskUsdt / (stopFrac + ROUND_TRIP_COST_FRAC) : 0;
  const riskCapped = wanted > cap;
  const notional = Math.min(wanted, cap);
  const effectiveLeverage = capital > 0 ? notional / capital : 0;
  const costUsdt = notional * (stopFrac + ROUND_TRIP_COST_FRAC);
  const costPctOfCapital = capital > 0 ? (costUsdt / capital) * 100 : 0;
  return { notional, effectiveLeverage, riskCapped, costUsdt, costPctOfCapital };
}
