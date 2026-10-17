import { useTranslation } from "react-i18next";
import { localeForLanguage } from "@/i18n";
import { formatNumber } from "@/lib/format";

/**
 * Round-trip cost the break-even figures assume, % of the position (lab
 * next-candle pre-registration, 17 Oct 2026: 0 of 2,022 configurations
 * reached the hit rate on any of these timeframes).
 */
export const ROUND_TRIP_COST_PCT = 0.12;

/**
 * Hit rate a strategy that trades every candle needs to break even at
 * `ROUND_TRIP_COST_PCT`, %. Only the short timeframes carry the warning; the
 * longer ones are listed for the record.
 */
export const BREAKEVEN_HIT_RATE_PCT: Readonly<Record<string, number>> = {
  "15m": 68.6,
  "30m": 63.3,
  "1h": 59.4,
  "4h": 54.6,
};

/** The timeframes that show the cost warning. */
export const WARN_TIMEFRAMES: readonly string[] = ["15m", "30m"];

/** Break-even hit rate for a timeframe that carries the warning, else null. */
export function shortTimeframeHitRate(timeframe: string): number | null {
  return WARN_TIMEFRAMES.includes(timeframe) ? (BREAKEVEN_HIT_RATE_PCT[timeframe] ?? null) : null;
}

/** One line under a timeframe choice: the hit rate costs demand on 15m and 30m. Nothing on other timeframes. */
export function TimeframeCostNote({ timeframe }: { timeframe: string }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const rate = shortTimeframeHitRate(timeframe);
  if (rate === null) return null;
  const pct = (v: number) => `${formatNumber(v, locale, { minimumFractionDigits: 1, maximumFractionDigits: 2 })}%`;
  return (
    <span className="ae-tfcost" role="note" data-timeframe={timeframe}>
      {t("timeframeCost.warning", { tf: timeframe, rate: pct(rate), cost: pct(ROUND_TRIP_COST_PCT) })}
    </span>
  );
}
