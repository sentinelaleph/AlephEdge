import type { TFunction } from "i18next";
import type { PineErrorInfo, PineTriggerInfo } from "@/lib/ipc/pine/pine";
import { tvOutcomeText } from "@/pages/Settings/TradingViewTab";
import { triggerNumber } from "./pineBotModel";

type Exists = (key: string) => boolean;

/** Codes that wrap another code after the pipe: the text names both. */
const WRAPPERS = ["livePending", "pineFlipCloseFailed", "pineFlipOpenRefused"];

/**
 * A Pine bot code (outcome, note, refusal, status note) in the user's
 * language: `errors.pine*` with its `|detail`, the symbol minimum with its
 * four numbers, a wrapper with the code it wraps, and every code shared with
 * the TradingView bot through that bot's chain. Unknown text is shown as it came.
 */
export function pineCodeText(t: TFunction, exists: Exists, raw: string): string {
  const [head0, ...rest] = raw.split("|");
  const head = head0.trim();
  const detail = rest.join("|").trim();
  if (head === "pineBelowSymbolMinimum") {
    const [symbol = "", min = "", size = "", max = ""] = rest.map((x) => x.trim());
    return t("botsPine.belowMinimum", { symbol, min, size, max });
  }
  if (WRAPPERS.includes(head) && detail) {
    const own = head === "livePending" ? t("settings.tv.outcome.livePending") : t(`errors.${head}`, { detail: "" });
    return `${own} · ${pineCodeText(t, exists, detail)}`;
  }
  if (head.startsWith("pine") && exists(`errors.${head}`)) {
    const holder =
      head === "pineSymbolHeldBy" ? (detail === "strategy" ? t("settings.tv.holderStrategy") : t(`bots.kind.${detail}`, { defaultValue: detail })) : detail;
    return t(`errors.${head}`, { detail: holder });
  }
  return tvOutcomeText(t, exists, raw);
}

/** A trigger's name: its title, else "plotshape #n" / "alert #n". */
export function triggerLabel(t: TFunction, tr: Pick<PineTriggerInfo, "id" | "title">): string {
  if (tr.title.trim()) return tr.title;
  const n = triggerNumber(tr.id);
  return tr.id.startsWith("alert") ? t("botsPine.trigger.alertN", { n }) : t("botsPine.trigger.shapeN", { n });
}

/** An engine error's kind in words ("syntax", "type", "runtime", …); an unknown kind as it came. */
export function errorKindText(t: TFunction, exists: Exists, kind: string): string {
  const key = `botsPine.errKind.${kind}`;
  return exists(key) ? t(key) : kind;
}

/** The engine's message: a known code translated, else the engine's own words (data, not UI copy). */
export function engineMessage(t: TFunction, exists: Exists, e: Pick<PineErrorInfo, "message">): string {
  return /^pine[A-Z]\w*(\|.*)?$/.test(e.message) ? pineCodeText(t, exists, e.message) : e.message;
}
