import { SIGNAL_BOT_IDS } from "@/app/router/routes";
import type { StatusKind } from "@/components/ui/StatusChip/StatusChip";
import { capitalAboveCap } from "@/lib/botLimits";
import type { BotConfig, BotDeskStatus, BotKind } from "@/lib/ipc/bot/bot";

export type BotType = "signal" | "dca" | "grid";
export type BotRowState = "running" | "stopped" | "stoppedByKillSwitch" | "notConfigured";

/**
 * One row of the bots table. Today only the three legacy signal kinds exist
 * (bot_status); DCA and Grid instances arrive with Rust's bot_list.
 */
export interface BotRow {
  id: string;
  kind: BotKind;
  type: BotType;
  market: "futures" | "spot";
  config: BotConfig | null;
  running: boolean;
  state: BotRowState;
  live: boolean;
  exchangeId: string | null;
  symbols: string[];
  openPositions: number;
  capital: number | null;
  maxPositions: number | null;
  /** i18n key of the reason Start is refused, or null when it is allowed. */
  startBlockKey: string | null;
  /** No replay or forward test exists for this bot's rule (Pump). */
  untested: boolean;
  /**
   * Saved capital per position is above the risk level's cap: Rust refuses
   * to start it, and a running one skips every signal.
   */
  capitalAboveCap: boolean;
}

const KINDS: BotKind[] = ["futures", "spot", "pump"];

/**
 * The signal bot rows. Pump is not unlocked by the risk level any more:
 * raising that GLOBAL level to try it also loosened every other bot's
 * limits (Rust risk/model.rs). It runs paper only and is marked untested.
 * `maxCapitalQuote`: the risk level's cap per position (null = not loaded).
 */
export function signalBotRows(s: BotDeskStatus, maxCapitalQuote: number | null = null): BotRow[] {
  return KINDS.map((kind) => {
    const config = s[kind];
    const running = kind === "futures" ? s.futuresRunning : kind === "spot" ? s.spotRunning : s.pumpRunning;
    const state: BotRowState = running
      ? "running"
      : !config
        ? "notConfigured"
        : s.killSwitchTripped
          ? "stoppedByKillSwitch"
          : "stopped";
    const overCap = config !== null && capitalAboveCap(config.capital, maxCapitalQuote);
    const startBlockKey = s.killSwitchTripped
      ? "botsList.blocked.killSwitch"
      : !config
        ? "botsList.blocked.notConfigured"
        : overCap
          ? "botsList.blocked.capitalAboveCap"
          : null;
    return {
      id: SIGNAL_BOT_IDS[kind],
      kind,
      type: "signal",
      market: kind === "spot" ? "spot" : "futures",
      config,
      running,
      state,
      live: config?.live === true,
      exchangeId: config?.exchangeId ?? null,
      symbols: config?.symbols ?? [],
      openPositions: s.openPositions.filter((p) => p.botKind === kind).length,
      capital: config?.capital ?? null,
      maxPositions: config?.maxPositions ?? null,
      startBlockKey,
      untested: kind === "pump",
      capitalAboveCap: overCap,
    };
  });
}

/** The legacy kind behind a signal bot id, or null. */
export function kindForBotId(id: string): BotKind | null {
  const hit = (Object.entries(SIGNAL_BOT_IDS) as [BotKind, string][]).find(([, v]) => v === id);
  return hit ? hit[0] : null;
}

/**
 * The StatusChip of a signal bot row: state, its label key, and the reason
 * shown on hover when the state blocks a start (the daily stop).
 */
export function rowStatus(row: BotRow): { status: StatusKind; labelKey: string; reasonKey: string | null } {
  if (row.running) return { status: "running", labelKey: "botState.running", reasonKey: null };
  if (row.state === "stoppedByKillSwitch")
    return { status: "killSwitch", labelKey: "botState.stoppedByKillSwitch", reasonKey: "botsList.blocked.killSwitch" };
  if (row.state === "notConfigured") return { status: "notConfigured", labelKey: "botState.notConfigured", reasonKey: null };
  return { status: "stopped", labelKey: "botState.stopped", reasonKey: null };
}
