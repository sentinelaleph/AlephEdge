import type { TFunction } from "i18next";
import { SIGNAL_BOT_IDS } from "@/app/router/routes";
import type { StatusKind } from "@/components/ui/StatusChip/StatusChip";
import { capitalAboveCap } from "@/lib/botLimits";
import { botMode, kindRunning, needsArm, SIGNAL_KINDS, type BotConfig, type BotDeskStatus, type BotKind } from "@/lib/ipc/bot/bot";

export type BotType = "signal" | "tradingview" | "dca" | "grid";
export type BotRowState = "running" | "stopped" | "stoppedByKillSwitch" | "notConfigured";

/**
 * One row of the bots table. Today only the three legacy signal kinds exist
 * (bot_status); DCA and Grid instances arrive with Rust's bot_list.
 */
export interface BotRow {
  id: string;
  kind: BotKind;
  /** The bot's own name when it has one (the TradingView bot), else null. */
  name: string | null;
  type: BotType;
  market: "futures" | "spot";
  config: BotConfig | null;
  running: boolean;
  state: BotRowState;
  /** A Live bot (stored mode), armed or not. */
  live: boolean;
  /** A Live bot armed for real orders this session. */
  armed: boolean;
  exchangeId: string | null;
  symbols: string[];
  openPositions: number;
  /** Signal bots: capital per position. TradingView bot: its budget. */
  capital: number | null;
  /** TradingView bot only: the max size per alert (position notional). */
  maxSizeUsdt: number | null;
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

const KINDS = SIGNAL_KINDS;

/**
 * The signal bot rows. Pump is not gated by the risk level: raising that
 * GLOBAL level for one bot would loosen every other bot's limits (Rust
 * risk/model.rs). It runs paper only and is marked untested.
 * `maxCapitalQuote`: the risk level's cap per position (null = not loaded).
 */
export function signalBotRows(s: BotDeskStatus, maxCapitalQuote: number | null = null): BotRow[] {
  return KINDS.map((kind) => bookRow(s, kind, maxCapitalQuote));
}

/** The TradingView bot's row (#/bots/tradingview): its own type, same state and Start / Stop rules. */
export function tvBotRow(s: BotDeskStatus, maxCapitalQuote: number | null = null): BotRow {
  return bookRow(s, "tradingview", maxCapitalQuote);
}

/**
 * Every bot of the book (the signal bots and the TradingView bot), as All
 * bots, the Dashboard and their counters list them. The TradingView bot is a
 * row only once it was made.
 */
export function allBotRows(s: BotDeskStatus, maxCapitalQuote: number | null = null): BotRow[] {
  const tv = tvBotRow(s, maxCapitalQuote);
  return [...signalBotRows(s, maxCapitalQuote), ...(tv.config ? [tv] : [])];
}

function bookRow(s: BotDeskStatus, kind: BotKind, maxCapitalQuote: number | null): BotRow {
  const config = s[kind] ?? null;
  const running = kindRunning(s, kind);
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
      : s.tradingBlocked
        ? "botsList.blocked.tradingBlocked"
        : needsArm(config)
          ? "botsList.blocked.liveArm"
          : overCap
            ? "botsList.blocked.capitalAboveCap"
            : null;
  const tv = kind === "tradingview" ? (config?.tv ?? null) : null;
  return {
    id: kind === "tradingview" ? TV_BOT_ID : SIGNAL_BOT_IDS[kind as keyof typeof SIGNAL_BOT_IDS],
    kind,
    name: tv?.name.trim() || null,
    type: kind === "tradingview" ? "tradingview" : "signal",
    market: kind === "spot" ? "spot" : "futures",
    config,
    running,
    state,
    live: botMode(config) === "live",
    armed: config?.live === true,
    exchangeId: config?.exchangeId ?? null,
    symbols: config?.symbols ?? [],
    openPositions: s.openPositions.filter((p) => p.botKind === kind).length,
    capital: tv ? tv.budgetUsdt : (config?.capital ?? null),
    maxSizeUsdt: tv ? tv.maxSizeUsdt : null,
    maxPositions: config?.maxPositions ?? null,
    startBlockKey,
    untested: kind === "pump",
    capitalAboveCap: overCap,
  };
}

/** A row's name: the bot's own name (the TradingView bot), else its kind's name. */
export function botRowName(t: TFunction, row: Pick<BotRow, "kind" | "name">): string {
  return row.name ?? t(`bots.kind.${row.kind}`);
}

/** The TradingView bot's id in bot links (#/bots/tradingview is its page). */
export const TV_BOT_ID = "tradingview";

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
