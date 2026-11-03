/**
 * Pine bot IPC bridge (mirrors src-tauri/src/bot/pinebot and the runner
 * contract in src-tauri/src/pine). The bot is `BotKind` `pine`: one script,
 * one timeframe, 1 to 10 USDT-M symbols, closed Binance bars only. Its Live
 * arm is the desk's `botSetLive("pine", …)`; a bot-level stop is required
 * for Live (`pineLiveStopRequired`).
 *
 * Every refusal is a code (`key` or `key|detail`), never English text; the
 * list is `PINE_CODES`. Outside Tauri, `npm run dev` gets an idle preview
 * through `devMock` (no script runtime: compiles answer `pineRuntimeMissing`).
 */

import { devMock, inTauri, invoke, listenEvent, type Unlisten } from "../bridge";
import type { BotConfig } from "../bot/bot";

type Mock = typeof import("./pine.mock");
const mock = <T>(run: (m: Mock) => Promise<T>) => devMock(() => import("./pine.mock"), run);

/** Bar timeframe (decision D3). */
export type PineTimeframe = "15m" | "1h" | "4h" | "1d";
export const PINE_TIMEFRAMES: readonly PineTimeframe[] = ["15m", "1h", "4h", "1d"];

/** What a fired trigger does; an unmapped trigger is `ignore`. */
export type PineAction = "buy" | "sell" | "close" | "ignore";

/** Mirrors `PineSettings` (bot/pine_settings.rs). Rust recomputes `scriptHash` on every save. */
export interface PineSettings {
  name: string;
  script: string;
  scriptHash: string;
  timeframe: PineTimeframe;
  /** 1 to 10 USDT-M futures symbols. */
  symbols: string[];
  /** Trigger id ("alert#0", "shape#1") to action; absent = ignore. */
  triggers: Record<string, PineAction>;
  /** Input overrides keyed by the engine's input id. */
  inputs: Record<string, unknown>;
  warmupBars: number;
  budgetUsdt: number;
  /** Position notional of one entry. */
  maxSizeUsdt: number;
  maxOpensPerDay: number;
  /** Stop, % from the fill. Required for Live. */
  stopPct: number | null;
  takeProfitPct: number | null;
  /** Title of an `input.float` read as the stop %; the bot's stop stays the floor (the tighter one is used). */
  stopInput: string | null;
  /** Title of an `input.float` read as the target %. */
  targetInput: string | null;
  staleS: number;
  priceGuardPct: number;
}

/** The ranges Rust checks (`PineSettings::validate`); pine.test.ts reads them from the Rust source. */
export const PINE_LIMITS = {
  maxSymbols: 10,
  maxScriptKb: 500,
  warmupMin: 300,
  warmupMax: 5000,
  warmupDefault: 1000,
  opensMax: 200,
  opensDefault: 20,
  staleMin: 15,
  staleMax: 300,
  staleDefault: 60,
  guardMax: 10,
  guardDefault: 0.5,
  stopMin: 0.1,
  stopMax: 50,
  targetMin: 0.1,
  targetMax: 1000,
  sizeMin: 5,
} as const;

/** A named Pine-lite error (`kind`: "syntax", "type", "runtime", "limit", "input", "unsupported", …). */
export interface PineErrorInfo {
  kind: string;
  message: string;
  line: number;
  col: number;
}

export interface PineTriggerInfo {
  /** "alert#n" or "shape#n", stable for one script text. */
  id: string;
  kind: "alert" | "shape";
  /** Empty when the script gave none (show "plotshape #n"). */
  title: string;
  line: number;
}

/** One `input.*` declaration as the engine reports it. */
export type PineInput = Record<string, unknown>;

/** What a compile found (`pine::CompileInfo`). */
export interface PineCompileInfo {
  ok: boolean;
  error: PineErrorInfo | null;
  title: string;
  overlay: boolean;
  inputs: PineInput[];
  triggers: PineTriggerInfo[];
  hash: string;
}

export interface PineFiredTrigger {
  id: string;
  title: string;
  action: PineAction;
}

export interface PineRunError extends PineErrorInfo {
  /** Open time of the bar the run was for (ms). `kind` "timeout" = over the wall time. */
  barTime: number;
  atMs: number;
}

export interface PineSymbolStatus {
  symbol: string;
  lastRunAt: number | null;
  /** Open time of the last bar run (ms). */
  lastBarTime: number | null;
  barsUsed: number | null;
  runMs: number | null;
  lastFired: PineFiredTrigger[];
  lastError: PineRunError | null;
  /** `pineBarMissing`, `pineStale`, `pineBarsUnavailable`, `pineBarsFailed|…`, `storeWriteFailed`… */
  lastNote: string | null;
}

/** The bot card. */
export interface PineStatus {
  configured: boolean;
  running: boolean;
  mode: "paper" | "live" | null;
  /** Armed for real orders this session. */
  armed: boolean;
  scriptHash: string | null;
  timeframe: PineTimeframe | null;
  /** `pineHalted|3` while new entries are stopped after failed runs (a start ends it). */
  halted: string | null;
  failedInARow: number;
  opensToday: number;
  maxOpensPerDay: number;
  /** Real entries whose outcome is still being looked up. */
  pending: number;
  pilotLeft: number;
  nextBarClose: number | null;
  symbols: PineSymbolStatus[];
}

/** One action row: what a bar's mapped triggers did. */
export interface PineResult {
  /** `pine:<hash>:<symbol>:<bar close ms>`, acted on at most once. */
  key: string;
  symbol: string;
  scriptHash: string;
  timeframe: PineTimeframe;
  barTime: number;
  barCloseMs: number;
  barClose: number;
  handledAt: number;
  triggers: PineFiredTrigger[];
  /** `opened`, `closed`, `flipped`, `livePending|code`, `pineRunning`, or a refusal code. */
  outcome: string;
  /** What else held something back (`pineConflict` after a close). */
  note: string | null;
  side: "long" | "short" | null;
  botName: string;
  mode: "paper" | "live";
  positionId: string | null;
  requestedQty: number | null;
  filledQty: number | null;
  fillPrice: number | null;
  /** (fill − bar close) / bar close in %, positive = worse for the trade. */
  slippagePct: number | null;
  stopPct: number | null;
  targetPct: number | null;
}

/**
 * Every code the Pine bot's Rust side returns or records (outcome, note,
 * refusal, status note), for the screens' i18n. Codes shared with the other
 * bots (`liveArmRequired`, `liveTermsChanged|…`, `liveOrderFailed`,
 * `botDailyLossTripped`, `priceUnavailable`, `belowExchangeMinimum|…`,
 * `tradeSymbolNotListed`, …) are not repeated here.
 */
export const PINE_CODES = [
  // settings
  "pineSettingsMissing", "pineBotNameInvalid", "pineScriptEmpty", "pineScriptTooLarge", "pineSymbolsMin", "pineSymbolsMax",
  "pineSymbolInvalid", "pineTriggerMapInvalid", "pineWarmupRange", "pineBudgetInvalid", "pineMaxSizeInvalid", "pineOpensPerDayRange",
  "pineStopRange", "pineTargetRange", "pineStopRequired", "pineTargetRequired", "pineStaleRange", "pinePriceGuardRange",
  "pineLiveStopRequired", "pineBotKind", "pineCompileFailed",
  // runs and bars
  "pineBarMissing", "pineStale", "pineBarsUnavailable", "pineBarsFailed", "pineRunTimeout", "pineRunAborted", "pineHalted",
  "pineRuntimeMissing",
  // actions
  "pineRunning", "pineConflict", "pineAlreadyOpen", "pineNothingToClose", "pinePriceMoved", "pineDailyOpensFull", "pinePositionsFull",
  "pineBudgetFull", "pineSymbolHeldBy", "pineBelowSymbolMinimum", "pineInputNotFound", "pineInputLevelInvalid", "pineFlipCloseFailed",
  "pineFlipOpenRefused", "pineBotRequired", "pineOpened", "pineLateFillAdopted",
] as const;

/** Compiles a script (nothing saved): inputs, triggers, hash, or the engine's named error. */
export function pineCompile(source: string): Promise<PineCompileInfo> {
  return inTauri() ? invoke<PineCompileInfo>("pine_compile", { source }) : mock((m) => m.mock.compile(source));
}

/** A new Pine bot's defaults (Binance, Paper, capital fitted under the cap). */
export function pineDefaultConfig(): Promise<BotConfig> {
  return inTauri() ? invoke<BotConfig>("pine_default_config") : mock((m) => m.mock.defaultConfig());
}

export function pineGetConfig(): Promise<BotConfig | null> {
  return inTauri() ? invoke<BotConfig | null>("pine_get_config") : mock((m) => m.mock.getConfig());
}

/** Saves the Pine bot; an edit of the script, triggers, inputs, bars, exits or money disarms a Live bot. */
export function pineSaveConfig(config: BotConfig): Promise<BotConfig> {
  return inTauri() ? invoke<BotConfig>("pine_save_config", { config }) : mock((m) => m.mock.saveConfig(config));
}

/** Starts the bot (a Live one needs its typed LIVE first); a start ends a halt. */
export function pineStart(): Promise<PineStatus> {
  return inTauri() ? invoke<PineStatus>("pine_start") : mock((m) => m.mock.start());
}

export function pineStop(): Promise<PineStatus> {
  return inTauri() ? invoke<PineStatus>("pine_stop") : mock((m) => m.mock.stop());
}

export function pineStatus(): Promise<PineStatus> {
  return inTauri() ? invoke<PineStatus>("pine_status") : mock((m) => m.mock.status());
}

/** Action rows, newest first (default 100, max 500). */
export function pineResults(limit?: number): Promise<PineResult[]> {
  return inTauri() ? invoke<PineResult[]>("pine_results", { limit }) : mock((m) => m.mock.results());
}

/** Rust's `RESULT_EVENT`: a result row was written or updated (payload: its key). */
export const PINE_RESULT_EVENT = "pine:result";

export function onPineResult(handler: (key: string) => void): Promise<Unlisten> {
  return listenEvent<string>(PINE_RESULT_EVENT, handler);
}
