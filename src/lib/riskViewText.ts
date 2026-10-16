import type { TFunction } from "i18next";
import i18n from "@/i18n";
import { backtestErrorText } from "@/lib/strategyText";

/**
 * A risk view refusal or unavailable reason: `riskview.errors.<code>` when it
 * is the view's own, else the history / strategy text (a DataHub read, a
 * candidate config Rust refused).
 */
export function riskviewErrorText(t: TFunction, raw: string): string {
  const code = raw.split("|")[0].trim();
  if (i18n.exists(`riskview.errors.${code}`)) return t(`riskview.errors.${code}`);
  return backtestErrorText(t, raw);
}
