import type { TFunction } from "i18next";
import { formatNumber } from "@/lib/format";
import type { BudgetFloor } from "@/lib/ipc/strategy/strategy";

/**
 * The minimum budget the DCA / Grid form shows under the Budget field, as
 * Rust serves it (strategy_budget_floor). Nothing here computes a bound:
 * "loading" and "failed" say so instead of showing a number.
 */
export type FloorState = { status: "loading" } | { status: "failed" } | { status: "ready"; floor: BudgetFloor };

const amount = (v: number, locale: string) => formatNumber(v, locale, { maximumFractionDigits: 2 });

/** The save floor (Rust refuses a smaller budget), or null while unknown. */
export function saveMinimum(s: FloorState): number | null {
  return s.status === "ready" ? s.floor.minBudget : null;
}

/**
 * "Minimum 176.98 USDT · orders: 8, smallest 5 USDT (Binance minimum)".
 * `rulesSymbol`: the pair whose Binance rules the pending call reads (a
 * Binance futures bot), or null.
 */
export function minimumLine(t: TFunction, s: FloorState, locale: string, rulesSymbol: string | null): string {
  if (s.status === "loading") return rulesSymbol ? t("strategy.budgetFloor.loadingRules", { symbol: rulesSymbol }) : t("strategy.budgetFloor.loading");
  if (s.status === "failed") return t("strategy.budgetFloor.failed");
  const f = s.floor;
  if (f.minBudget === null) return t("strategy.budgetFloor.fixedSmall", { order: amount(f.minOrder, locale) });
  return t("strategy.budgetFloor.min", { min: amount(f.minBudget, locale), orders: f.orders, order: amount(f.minOrder, locale) });
}

/**
 * The Live minimum of a Binance futures symbol whose exchange minimum is
 * above the save floor, or why it is unknown; null when Live asks for
 * nothing more (or there is no Live path).
 */
export function liveLine(t: TFunction, s: FloorState, locale: string, symbol: string): string | null {
  if (s.status !== "ready") return null;
  const f = s.floor;
  if (f.symbolRules === "notApplicable") return null;
  if (f.symbolRules === "unavailable") return t("strategy.budgetFloor.liveUnknown", { symbol });
  const order = f.symbolMinOrder;
  if (order === null || order <= f.minOrder + 1e-9) return null;
  if (f.liveMinBudget === null) return t("strategy.budgetFloor.liveFixedSmall", { symbol, order: amount(order, locale) });
  if (f.minBudget !== null && f.liveMinBudget <= f.minBudget + 0.005) return null;
  return t("strategy.budgetFloor.live", { min: amount(f.liveMinBudget, locale), symbol, order: amount(order, locale) });
}

/** The three Budget field errors: empty, not a number, below the minimum. */
export function budgetErrors(t: TFunction, s: FloorState, locale: string) {
  const min = saveMinimum(s);
  const minText = min !== null ? amount(min, locale) : null;
  const order = s.status === "ready" ? amount(s.floor.minOrder, locale) : "5";
  const orders = s.status === "ready" ? s.floor.orders : 0;
  return {
    empty: minText !== null ? t("strategy.budgetFloor.empty", { min: minText }) : t("strategy.budgetFloor.emptyNoMin"),
    notANumber: minText !== null ? `${t("strategy.form.notANumber")} ${t("strategy.budgetFloor.minShort", { min: minText })}` : t("strategy.form.notANumber"),
    below: minText !== null ? t("strategy.budgetFloor.below", { min: minText, orders, order }) : null,
  };
}
