import type { TFunction } from "i18next";
import i18n from "@/i18n";
import { localizeError } from "@/lib/errorText";
import { parseStrategyError, type RunState, type StrategyBotView, type StrategyNote } from "@/lib/ipc/strategy/strategy";

/**
 * Renders a Rust strategy error ("code" or "code|field") through
 * `strategy.errors.*`. An unknown code falls back to the raw text, so a new
 * Rust code is still visible instead of a generic failure.
 */
export function strategyErrorText(t: TFunction, raw: string): string {
  const { code } = parseStrategyError(raw);
  return t(`strategy.errors.${code}`, { defaultValue: raw });
}

/**
 * A note line: `strategy.notes.<key>`, with the exit reason for cycleClosed.
 * A cycle that fails to open notes the Rust refusal code itself
 * (driver.rs `Err(code) => notes.push((code, None))`: ladderExceedsBudget,
 * gridBadRange, …), so a key missing under notes is read under
 * `strategy.errors.*` before falling back to the raw code.
 */
export function strategyNoteText(t: TFunction, n: Pick<StrategyNote, "key" | "detail">): string {
  if (n.key === "cycleClosed" && n.detail) {
    return t("strategy.notes.cycleClosed", { reason: t(`strategy.exit.${n.detail}`, { defaultValue: n.detail }) });
  }
  const text = i18n.exists(`strategy.notes.${n.key}`)
    ? t(`strategy.notes.${n.key}`, { reason: "" })
    : t(`strategy.errors.${n.key}`, { defaultValue: n.key });
  return n.detail && (n.key === "storeReadFailed" || n.key === "storeWriteFailed")
    ? `${text} (${localizeError(n.detail)})`
    : text;
}

export const RUN_TONE: Record<RunState, "running" | "waiting" | "paused" | "stopped" | "error"> = {
  inCycle: "running",
  armed: "waiting",
  paused: "paused",
  stopped: "stopped",
  dead: "error",
};

/**
 * The state label a person reads. A stopped bot still managing its open
 * cycle is "Paused: managing open cycle", not "In cycle", because Pause was
 * pressed and no new cycle will open.
 */
export function runStateText(t: TFunction, v: StrategyBotView): string {
  if (v.runState === "dead") {
    return v.deadReason
      ? t("strategy.state.deadReason", { reason: t(`strategy.exit.${v.deadReason}`, { defaultValue: v.deadReason }) })
      : t("strategy.state.dead");
  }
  if (v.runState === "paused" && v.pauseReason) {
    return t("strategy.state.pausedReason", { reason: t(`strategy.notes.${v.pauseReason}`, { defaultValue: v.pauseReason }) });
  }
  if (v.openCycle && !v.acceptingNewCycles) return t("strategy.state.closingOnly");
  return t(`strategy.state.${v.runState}`);
}

export function sideText(t: TFunction, side: string): string {
  return t(`strategy.side.${side}`, { defaultValue: side });
}
