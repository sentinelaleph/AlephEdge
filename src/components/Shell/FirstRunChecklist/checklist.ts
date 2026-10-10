import type { BotDeskStatus } from "@/lib/ipc/bot/bot";
import { isActive, type StrategyBotView } from "@/lib/ipc/strategy/strategy";

/** The template a new user starts from: it passed its checks (strategy_presets). */
export const FIRST_PRESET_ID = "dca_long_classic";
export const FIRST_PRESET_PATH = `/bots/dca/new?preset=${FIRST_PRESET_ID}`;

export type ChecklistStepKey = "signedIn" | "streamConnected" | "botConfigured" | "botRunning";

export interface ChecklistStep {
  key: ChecklistStepKey;
  done: boolean;
  /** The page that completes this step. */
  to?: string;
  /** Which hint the step shows while open. */
  hint?: "streamWaiting" | "firstPreset" | "botRunning";
}

export interface ChecklistState {
  steps: ChecklistStep[];
  doneCount: number;
  allDone: boolean;
  /** Any bot exists: a signal bot config or a DCA / Grid bot. */
  botConfigured: boolean;
}

type SignalStatus = Pick<BotDeskStatus, "futures" | "spot" | "pump" | "futuresRunning" | "spotRunning" | "pumpRunning">;

/**
 * The checklist from real state, shared by the checklist and the Dashboard's
 * hide rule so the two cannot disagree. Every bot type counts: a user who
 * runs only DCA Long Classic is done (audit 2026-10-08, ux_global 2). The
 * Sentinel stream feeds signal bots only, so its step appears once a signal
 * bot is configured.
 */
export function checklistState(
  status: SignalStatus,
  strategyBots: StrategyBotView[] | null,
  streamConnected: boolean,
): ChecklistState {
  const signalConfigured = status.futures !== null || status.spot !== null || status.pump !== null;
  const signalRunning = status.futuresRunning || status.spotRunning || status.pumpRunning;
  const bots = strategyBots ?? [];
  const strategyRunning = bots.some((b) => isActive(b) || b.openCycle !== null);
  const botConfigured = signalConfigured || bots.length > 0;
  const botRunning = signalRunning || strategyRunning;

  const kinds = new Set(bots.map((b) => b.kind));
  const runPage =
    signalConfigured && bots.length > 0
      ? "/bots"
      : signalConfigured
        ? "/bots/signal"
        : kinds.size === 1
          ? `/bots/${[...kinds][0]}`
          : bots.length > 0
            ? "/bots"
            : FIRST_PRESET_PATH;

  const steps: ChecklistStep[] = [{ key: "signedIn", done: true }];
  if (signalConfigured) {
    steps.push({ key: "streamConnected", done: streamConnected, to: "/signals", hint: "streamWaiting" });
  }
  steps.push(
    { key: "botConfigured", done: botConfigured, to: FIRST_PRESET_PATH, hint: "firstPreset" },
    { key: "botRunning", done: botRunning, to: runPage, hint: botConfigured ? "botRunning" : undefined },
  );
  const doneCount = steps.filter((s) => s.done).length;
  return { steps, doneCount, allDone: doneCount === steps.length, botConfigured };
}
