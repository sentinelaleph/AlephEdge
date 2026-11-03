/**
 * The live line under the DCA stop loss field: where the stop sits and what
 * it costs on the fully filled ladder. Every number comes from Rust
 * (`validate::sl_estimate`, the same math as the preview's worst case); this
 * file only words it. Without a price the stop is shown relative to the
 * average entry and no price is made up.
 */
import type { TFunction } from "i18next";
import { formatNumber } from "@/lib/format";
import type { SlEstimate } from "@/lib/ipc/strategy/strategy";

export function stopLossLine(t: TFunction, e: SlEstimate, slPct: number, short: boolean, locale: string): string {
  const loss = formatNumber(e.lossQuote, locale, { maximumFractionDigits: 2 });
  const pct = formatNumber(e.lossPctOfBudget, locale, { maximumFractionDigits: 1 });
  if (e.stopPrice !== null) {
    return t("strategy.hint.slLine", { price: formatNumber(e.stopPrice, locale, { maximumSignificantDigits: 6 }), loss, pct });
  }
  const rel = `${short ? "+" : "−"}${formatNumber(slPct, locale, { maximumFractionDigits: 2 })}%`;
  return t("strategy.hint.slLineRelative", { rel, loss, pct });
}
