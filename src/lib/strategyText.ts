import type { TFunction } from "i18next";
import i18n from "@/i18n";
import { localizeError } from "@/lib/errorText";
import { formatNumber } from "@/lib/format";
import {
  parseStrategyError,
  type CycleRow,
  type Preset,
  type PresetUniverse,
  type RunState,
  type StrategyBotView,
  type StrategyNote,
} from "@/lib/ipc/strategy/strategy";

/**
 * Renders a Rust strategy error ("code" or "code|field") through
 * `strategy.errors.*`. An unknown code falls back to the raw text, so a new
 * Rust code is still visible instead of a generic failure.
 */
export function strategyErrorText(t: TFunction, raw: string): string {
  const { code, field } = parseStrategyError(raw);
  // "budgetCapReached|<field>|<free>|<limit>": the amounts in USDT.
  const [, , free, limit] = raw.split("|");
  if (code === "budgetCapReached" && free !== undefined && limit !== undefined) {
    return t("strategy.errors.budgetCapDetail", { free, limit });
  }
  // "code|detail": the detail fills {{detail}} (a minimum, the word to type).
  return t(`strategy.errors.${code}`, { defaultValue: raw, detail: field ?? "" });
}

/**
 * A Rust refusal of the config being edited. The min-notional refusal names
 * the budget that passes (`minBudget` from the preview or validation).
 */
export function configErrorText(t: TFunction, raw: string, minBudget: number | null, locale: string): string {
  if (parseStrategyError(raw).code === "budgetBelowMinNotional" && minBudget !== null) {
    return t("strategy.errors.budgetBelowMinNotionalMin", {
      min: formatNumber(minBudget, locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 }),
    });
  }
  return strategyErrorText(t, raw);
}

/** A preset's pair selection as text: "Top 5 …", or "ranked 6 to 15" when it skips the top. */
export function universeText(t: TFunction, u: PresetUniverse): string {
  return t(`strategy.preset.universe.${u.rule}${u.rankFrom > 1 ? "Ranked" : ""}`, {
    n: u.topN,
    from: u.rankFrom,
    days: u.volumeWindowDays,
  });
}

/** The template's name in the user's language; the config name otherwise. */
export function presetName(t: TFunction, p: Preset): string {
  return t(`strategy.preset.names.${p.id}`, { defaultValue: p.config.name });
}

/**
 * A closed cycle's own net P&L, USDT: realized (gross) minus fees and
 * funding, as strategy_stats counts it. Never the stored % times today's
 * budget, which misstates every cycle closed before a budget edit. Null for
 * an open cycle.
 */
export function cycleNetQuote(r: Pick<CycleRow, "closedAt" | "realizedQuote" | "feesQuote" | "fundingQuote">): number | null {
  return r.closedAt === null ? null : r.realizedQuote - r.feesQuote - r.fundingQuote;
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
  // A bot kept Stopped after a restart: the Start check it failed.
  if (n.key === "resumeRefused") {
    return t("strategy.notes.resumeRefused", { reason: n.detail ? strategyErrorText(t, n.detail) : "" });
  }
  const text = i18n.exists(`strategy.notes.${n.key}`)
    ? t(`strategy.notes.${n.key}`, { reason: "" })
    : t(`strategy.errors.${n.key}`, { defaultValue: n.key });
  return n.detail && (n.key === "storeReadFailed" || n.key === "storeWriteFailed")
    ? `${text} (${localizeError(n.detail)})`
    : text;
}

/**
 * A bot that never ran: no cycle closed or open, and not accepting new ones.
 * New DCA / Grid bots are created stopped, so their first action is Start;
 * Resume is for a bot that ran and was paused.
 */
export function neverStarted(v: Pick<StrategyBotView, "acceptingNewCycles" | "cyclesDone" | "openCycle">): boolean {
  return !v.acceptingNewCycles && v.cyclesDone === 0 && v.openCycle === null;
}

/** The run action's label key: Pause while active, Start before the first run, Resume after. */
export function runActionKey(v: Pick<StrategyBotView, "acceptingNewCycles" | "cyclesDone" | "openCycle" | "runState">): string {
  if (v.acceptingNewCycles && v.runState !== "dead") return "strategy.actions.stop";
  return neverStarted(v) ? "strategy.actions.startFirst" : "strategy.actions.start";
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
