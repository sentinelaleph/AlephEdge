import { TV_LIMITS, type BotConfig, type TvSettings } from "@/lib/ipc/bot/bot";

/**
 * The TradingView bot's form as numbers and text, and the BotConfig it
 * saves. Rust re-checks every value (`TvSettings::validate`,
 * `tv_trade::normalize_config`); these checks only say why Save is off.
 */
export interface TvBotForm {
  name: string;
  exchangeId: string;
  budgetUsdt: number;
  maxSizeUsdt: number;
  maxPositions: number;
  leverage: number;
  maxOpensPerDay: number;
  /** Comma or space separated; empty = every pair. */
  allowList: string;
  staleS: number;
  priceGuardPct: number;
  requirePrice: boolean;
  /** "keep" the stored one, "set" `passphrase`, or "clear" it. */
  passphraseAction: "keep" | "set" | "clear";
  passphrase: string;
}

/** Why Save is off: an i18n key under botsTv.error and its params. */
export interface TvFormError {
  key: string;
  params?: Record<string, string | number>;
}

export function formFromConfig(cfg: BotConfig | null, fallbackExchange: string): TvBotForm {
  const tv: Partial<TvSettings> = cfg?.tv ?? {};
  return {
    name: tv.name ?? "TradingView",
    exchangeId: cfg?.exchangeId ?? fallbackExchange,
    budgetUsdt: tv.budgetUsdt ?? 100,
    maxSizeUsdt: tv.maxSizeUsdt ?? 50,
    maxPositions: cfg?.maxPositions ?? 3,
    leverage: cfg?.leverage ?? 3,
    maxOpensPerDay: tv.maxOpensPerDay ?? 10,
    allowList: (cfg?.symbols ?? []).join(", "),
    staleS: tv.staleS ?? TV_LIMITS.staleDefault,
    priceGuardPct: tv.priceGuardPct ?? TV_LIMITS.guardDefault,
    requirePrice: tv.requirePrice ?? true,
    passphraseAction: "keep",
    passphrase: "",
  };
}

/** `btcusdt, ETHUSDT ethusdt` -> ["BTCUSDT", "ETHUSDT"]: comma / space separated, upper case, once each. */
export function parseAllowList(text: string): string[] {
  const out: string[] = [];
  for (const w of text.split(/[\s,;]+/)) {
    const s = w.trim().toUpperCase();
    if (s && !out.includes(s)) out.push(s);
  }
  return out;
}

/** The first reason the form cannot be saved, or null. */
export function formError(f: TvBotForm, maxLeverage: number | null): TvFormError | null {
  const pos = (v: number) => Number.isFinite(v) && v > 0;
  if (!f.name.trim() || f.name.trim().length > 40) return { key: "name" };
  if (!f.exchangeId) return { key: "exchange" };
  if (!pos(f.budgetUsdt)) return { key: "budget" };
  if (!pos(f.maxSizeUsdt) || f.maxSizeUsdt < TV_LIMITS.sizeMin) return { key: "size", params: { min: TV_LIMITS.sizeMin } };
  if (!Number.isInteger(f.maxPositions) || f.maxPositions < 1 || f.maxPositions > 20) return { key: "positions" };
  if (!Number.isInteger(f.leverage) || f.leverage < 1 || (maxLeverage !== null && f.leverage > maxLeverage))
    return { key: "leverage", params: { max: maxLeverage ?? 1 } };
  if (!Number.isInteger(f.maxOpensPerDay) || f.maxOpensPerDay < 1 || f.maxOpensPerDay > TV_LIMITS.opensMax)
    return { key: "opens", params: { max: TV_LIMITS.opensMax } };
  if (!Number.isInteger(f.staleS) || f.staleS < TV_LIMITS.staleMin || f.staleS > TV_LIMITS.staleMax)
    return { key: "stale", params: { min: TV_LIMITS.staleMin, max: TV_LIMITS.staleMax } };
  if (!(f.priceGuardPct > 0) || f.priceGuardPct > TV_LIMITS.guardMax) return { key: "guard", params: { max: TV_LIMITS.guardMax } };
  if (parseAllowList(f.allowList).some((s) => !/^[A-Z0-9]+USDT$/.test(s) || s === "USDT")) return { key: "allowList" };
  if (f.passphraseAction === "set" && (!f.passphrase.trim() || f.passphrase.length > 64)) return { key: "passphrase" };
  return null;
}

/** The config Save sends. `base` is the stored config (or the server's defaults). */
export function configFromForm(f: TvBotForm, base: BotConfig): BotConfig {
  const tv: TvSettings = {
    name: f.name.trim(),
    budgetUsdt: f.budgetUsdt,
    maxSizeUsdt: f.maxSizeUsdt,
    maxOpensPerDay: f.maxOpensPerDay,
    staleS: f.staleS,
    priceGuardPct: f.priceGuardPct,
    requirePrice: f.requirePrice,
  };
  if (f.passphraseAction === "set") tv.passphrase = f.passphrase.trim();
  if (f.passphraseAction === "clear") tv.passphrase = "";
  return {
    ...base,
    kind: "tradingview",
    exchangeId: f.exchangeId,
    maxPositions: f.maxPositions,
    leverage: f.leverage,
    symbols: parseAllowList(f.allowList),
    sizing: "fixed",
    // Rust sets capital = budget / positions; sent the same so the cap check reads it.
    capital: Math.floor((f.budgetUsdt / f.maxPositions) * 100) / 100,
    tv,
  };
}

/** Margin per position the budget gives (what each alert may tie up at most on average). */
export function marginPerPosition(f: TvBotForm): number {
  return f.maxPositions > 0 ? Math.floor((f.budgetUsdt / f.maxPositions) * 100) / 100 : 0;
}

/**
 * The budget one alert at the max size needs, and the budget that holds
 * every position at the max size. Same rule as the alert guard
 * (tvlink/guard.rs `limit_refusal`: margin = size / leverage, at the
 * leverage `precheck::effective_leverage` uses: the bot's, capped by the
 * risk level). Rounded up to the cent; null while size or leverage is not
 * a usable number.
 */
export function budgetFloor(f: TvBotForm, maxLeverage: number | null): { min: number; full: number; leverage: number } | null {
  const lev = maxLeverage !== null ? Math.min(f.leverage, maxLeverage) : f.leverage;
  if (!(Number.isInteger(lev) && lev >= 1) || !(f.maxSizeUsdt >= TV_LIMITS.sizeMin)) return null;
  const cents = (v: number) => Math.ceil(v * 100 - 1e-9) / 100;
  const min = cents(f.maxSizeUsdt / lev);
  const positions = Number.isInteger(f.maxPositions) && f.maxPositions >= 1 ? f.maxPositions : 1;
  return { min, full: cents((f.maxSizeUsdt * positions) / lev), leverage: lev };
}
