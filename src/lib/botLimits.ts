/**
 * Bot-form bounds that Rust enforces. sizing.test.ts checks the constants
 * against the Rust ones named below; the server stays the authority and
 * rejects anything outside them.
 */

/** Upper bound on a bot's max concurrent positions (`MAX_BOT_POSITIONS` in bot/model.rs). */
export const MAX_BOT_POSITIONS = 20;

/** A new bot's capital when the risk level allows it (`DEFAULT_CAPITAL` in bot/model.rs). */
export const DEFAULT_CAPITAL = 100;

/**
 * Mirrors `check_capital_cap` in bot/commands.rs: the capital per position is
 * above the risk level's cap (balance × max capital %). Rust refuses to save
 * or start such a bot, because the engine would skip every signal it gets.
 * An unknown cap (risk state not loaded) is never reported as exceeded.
 */
export function capitalAboveCap(capital: number, maxCapitalQuote: number | null | undefined): boolean {
  return maxCapitalQuote != null && maxCapitalQuote > 0 && capital > maxCapitalQuote;
}

/** Mirrors `default_capital` in bot/model.rs: the default, lowered to the cap. */
export function defaultCapitalWithin(maxCapitalQuote: number | null | undefined): number {
  if (maxCapitalQuote == null || !Number.isFinite(maxCapitalQuote) || maxCapitalQuote <= 0) return DEFAULT_CAPITAL;
  const cap = maxCapitalQuote >= 1 ? Math.floor(maxCapitalQuote) : Math.floor(maxCapitalQuote * 100) / 100;
  return Math.min(DEFAULT_CAPITAL, cap);
}

/** Smallest order Binance accepts on most pairs, USDT; mirrors Rust EXCHANGE_MIN_ORDER_USDT. */
export const EXCHANGE_MIN_ORDER_USDT = 5;

/** Widest stop a new desk bot's default risk % is fitted for; mirrors Rust DESK_STOP_CEILING_FRAC. */
export const DESK_STOP_CEILING_FRAC = 0.1;

/**
 * Mirrors `default_risk_pct_for` in bot/model.rs: 1%, raised to the smallest
 * 0.1 step at which a 10% stop still sizes at the 5 USDT minimum, at most 5%.
 */
export function defaultRiskPctFor(capital: number, roundTripCostFrac = 0.0012, minPct = 1, maxPct = 5): number {
  if (!Number.isFinite(capital) || capital <= 0) return minPct;
  const needed = ((EXCHANGE_MIN_ORDER_USDT * (DESK_STOP_CEILING_FRAC + roundTripCostFrac)) / capital) * 100;
  const stepped = Math.ceil(needed * 10 - 1e-9) / 10;
  return Math.min(maxPct, Math.max(minPct, stepped));
}
