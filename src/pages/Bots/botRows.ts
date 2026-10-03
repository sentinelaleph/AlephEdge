import { SIGNAL_BOT_IDS } from "@/app/router/routes";
import type { StatusKind } from "@/components/ui/StatusChip/StatusChip";
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
  /** Pump below the Ambitious risk level: cannot be started at all. */
  locked: boolean;
}

const KINDS: BotKind[] = ["futures", "spot", "pump"];

export function signalBotRows(s: BotDeskStatus, allowsPump: boolean): BotRow[] {
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
    const locked = kind === "pump" && !allowsPump;
    const startBlockKey = s.killSwitchTripped
      ? "botsList.blocked.killSwitch"
      : kind === "pump" && !allowsPump
        ? "bots.pumpLocked"
        : !config
          ? "botsList.blocked.notConfigured"
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
      locked,
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
 * shown on hover when the state blocks a start (Pump locked by the risk level).
 */
export function rowStatus(row: BotRow): { status: StatusKind; labelKey: string; reasonKey: string | null } {
  if (row.running) return { status: "running", labelKey: "botState.running", reasonKey: null };
  if (row.state === "stoppedByKillSwitch")
    return { status: "killSwitch", labelKey: "botState.stoppedByKillSwitch", reasonKey: "botsList.blocked.killSwitch" };
  if (row.locked) return { status: "needsSetup", labelKey: "status.locked", reasonKey: "bots.pumpLocked" };
  if (row.state === "notConfigured") return { status: "notConfigured", labelKey: "botState.notConfigured", reasonKey: null };
  return { status: "stopped", labelKey: "botState.stopped", reasonKey: null };
}
