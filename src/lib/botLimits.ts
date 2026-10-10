/**
 * Bot-form bounds that Rust enforces. Kept by hand in lockstep with the Rust
 * constants named below; the server stays the authority and rejects anything
 * outside them.
 */

/** Upper bound on a bot's max concurrent positions (`MAX_BOT_POSITIONS` in bot/model.rs). */
export const MAX_BOT_POSITIONS = 20;

/** A new bot's capital when the risk level allows it (`DEFAULT_CAPITAL` in bot/commands.rs). */
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

/** Mirrors `default_capital` in bot/commands.rs: the default, lowered to the cap. */
export function defaultCapitalWithin(maxCapitalQuote: number | null | undefined): number {
  if (maxCapitalQuote == null || !Number.isFinite(maxCapitalQuote) || maxCapitalQuote <= 0) return DEFAULT_CAPITAL;
  const cap = maxCapitalQuote >= 1 ? Math.floor(maxCapitalQuote) : Math.floor(maxCapitalQuote * 100) / 100;
  return Math.min(DEFAULT_CAPITAL, cap);
}

/** Smallest order Binance accepts on most pairs, USDT; mirrors Rust EXCHANGE_MIN_ORDER_USDT. */
export const EXCHANGE_MIN_ORDER_USDT = 5;
