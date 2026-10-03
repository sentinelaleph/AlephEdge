/**
 * Bot-desk IPC bridge (mirrors src-tauri/src/bot). Live trading stays behind
 * a Rust-side feature flag (OFF through F3) — `liveTradingEnabled` in the
 * status is the honest indicator the UI shows. `npm run dev` gets an idle,
 * dev-only desk through `devMock`; the released bundle has none.
 */

import { devMock, inTauri, invoke } from "../bridge";

type Mock = typeof import("./bot.mock");
const mock = <T>(run: (m: Mock) => Promise<T>) => devMock(() => import("./bot.mock"), run);

export type BotKind = "futures" | "spot" | "pump";

/** How a position is sized — mirrors `SizingMode` in bot/model.rs. */
export type SizingMode = "fixed" | "risk";

/**
 * Which take-profit a bot exits at — mirrors `TakeProfitTarget` in
 * bot/model.rs. `custom.pct` is the percent distance from the actual fill in
 * the trade's direction (40 = +40% long, −40% short). Absent ⇒ TP1.
 */
export type TakeProfitTarget =
  | { kind: "tp1" }
  | { kind: "tp2" }
  | { kind: "tp3" }
  | { kind: "custom"; pct: number };

export interface BotConfig {
  kind: BotKind;
  exchangeId: string;
  maxPositions: number;
  capital: number;
  leverage: number;
  minConfidence?: number;
  direction?: string;
  symbols?: string[];
  /**
   * Trade ONLY these confluence combos (empty/absent = every combo). Sentinel's
   * published per-combo records differ by tens of percentage points, so this is
   * the desk's main lever for acting on its own evidence.
   */
  combos?: string[];
  /**
   * Trade ONLY signals whose PRIMARY confluence source is listed (empty/absent
   * = every engine). Primary = first contributing item, which is what
   * Sentinel's per-engine ledger is keyed on.
   */
  engines?: string[];
  /**
   * Per-position loss cap in percent: a position is closed once its
   * unrealized NET loss reaches −maxLossPct% (e.g. 2 = close at −2% net).
   * Applies at all times, independent of the BTC regime. Absent = off.
   */
  maxLossPct?: number;
  /**
   * "fixed" (notional = capital × leverage, the legacy behaviour) or "risk"
   * (notional sized so the stop loses `riskPerTradePct` of capital; leverage
   * becomes a ceiling, not the size itself). Absent on configs saved before
   * sizing existed ⇒ server treats as "fixed".
   */
  sizing?: SizingMode;
  /** Risk mode only: % of capital lost if the stop is hit (0.1–5.0). */
  riskPerTradePct?: number;
  /** Bot-wide take-profit target (absent ⇒ TP1). */
  takeProfit?: TakeProfitTarget;
  /** Per-symbol overrides of `takeProfit`, keyed by symbol ("SOLUSDT"). */
  takeProfitOverrides?: Record<string, TakeProfitTarget>;
  /** Enter only signals at most this many minutes old, newest first. 0 = no
   * limit. Absent in older saves: the Rust side defaults it to 240 (4 h). */
  maxSignalAgeMin?: number;
  /**
   * Real-money trading for this bot. Server-owned and read-only here:
   * `bot_configure` ignores it and keeps the bot's current value; only
   * `botSetLive` changes it. Never restored after a restart (comes back
   * simulated).
   */
  live?: boolean;
}

/**
 * The management plan attached to a position at open, if the engine armed
 * one. Kept snake_case: it mirrors the Rust struct field-for-field rather
 * than being a camelCase type we designed ourselves.
 */
export interface ManagementPlan {
  breakeven_at_r: number | null;
  partial_at_r: number | null;
  partial_fraction: number | null;
}

export interface OpenPosition {
  signalId: string;
  botKind: BotKind;
  exchangeId: string;
  symbol: string;
  direction: string;
  entry: number;
  tp: number;
  sl: number;
  leverage: number;
  capital: number;
  frAtOpen?: number | null;
  ldAtOpen?: number | null;
  openedAt: number;
  /** Signal's own timeframe, when known. */
  timeframe: string | null;
  /** The signal's published entry (may differ from `entry`, the actual fill). */
  signalEntry: number;
  /** Planned risk in R at open. */
  riskR: number;
  plan: ManagementPlan | null;
  /** True once the stop has been moved to breakeven. */
  breakevenArmed: boolean;
  /** Fraction of the position banked as a partial (0 when none taken yet). */
  partialFraction: number;
  /** Price the partial was (or will be) taken at; null when no partial is planned. */
  partialPrice: number | null;
  /** Milliseconds remaining until the holding-window horizon force-closes this position. */
  horizonMs: number;
  /** True when this position is backed by a real exchange order. */
  live: boolean;
  /** Executed quantity from the real fill (0 for simulated positions). */
  qty: number;
  /** LIVE only: the opening order's id. */
  entryOrderId: number | null;
  /** LIVE only: the exchange-side stop (algo order) currently protecting the position. */
  stopAlgoId: number | null;
  /** LIVE only: the exchange-side take-profit order id. */
  tpAlgoId: number | null;
  /** LIVE only: the exchange stop has actually been moved to the fill entry. */
  stopAtBreakeven: boolean;
  /** LIVE only: no exchange stop protects this position (the engine closes it). */
  unprotected: boolean;
  /** LIVE only: quantity the partial leg actually reduced on the exchange. */
  partialQty: number;
  /** Sizing used at open ("fixed" on legacy rows). */
  sizingMode: SizingMode;
  /** Declared risk % of capital at the stop (risk mode only). */
  riskPct: number | null;
  /** Position notional in USDT (0 on legacy rows). */
  notionalUsdt: number;
  /** notional / capital — may be below the configured leverage in risk mode. */
  effectiveLeverage: number;
  /** The leverage cap bound the size: real risk sits below the declared %. */
  riskCapped: boolean;
  /** Which target `tp` is: "tp1" | "tp2" | "tp3" | "custom:40" (legacy rows: "tp1"). */
  tpTarget: string;
  /** The configured target when the signal lacked it and a lower one was used. */
  tpFallbackFrom: string | null;
}

export interface SkipNote {
  atMs: number;
  symbol: string;
  reason: string;
  /**
   * For most reasons: a preformatted value the UI interpolates as-is (e.g.
   * "+0.120%"). For "vetoClosed" only (a veto closing one or more open
   * positions, 2026-09-18): `"<count>|<reason_text>"` — see
   * `vetoClosedParams` in DeskFeed.tsx, which splits it back apart.
   */
  detail?: string;
}

export interface BotDeskStatus {
  liveTradingEnabled: boolean;
  /** False when live orders would go to a non-production Binance (the testnet). */
  binanceIsProduction: boolean;
  futures: BotConfig | null;
  spot: BotConfig | null;
  pump: BotConfig | null;
  futuresRunning: boolean;
  spotRunning: boolean;
  pumpRunning: boolean;
  openPositions: OpenPosition[];
  recentSkips: SkipNote[];
  killSwitchTripped: boolean;
  /** Sentinel's BTC regime as the engine last read it. */
  btcRegime: "normal" | "break" | "unknown";
}

export function botStatus(): Promise<BotDeskStatus> {
  return inTauri() ? invoke<BotDeskStatus>("bot_status") : mock((m) => m.mock.status());
}

/**
 * `bot_configure` rejects an out-of-bounds risk % (or any other invalid
 * config) instead of clamping it — the rejection reaches here as a normal
 * promise rejection (Tauri's `Result<_, String>` convention; see bridge.ts)
 * and must be shown, never swallowed.
 */
export function botConfigure(config: BotConfig): Promise<BotDeskStatus> {
  return inTauri() ? invoke<BotDeskStatus>("bot_configure", { config }) : mock((m) => m.mock.configure(config));
}

export function botStart(kind: BotKind): Promise<BotDeskStatus> {
  return inTauri() ? invoke<BotDeskStatus>("bot_start", { kind }) : mock((m) => m.mock.start(kind));
}

export function botStop(kind: BotKind): Promise<BotDeskStatus> {
  return inTauri() ? invoke<BotDeskStatus>("bot_stop", { kind }) : mock((m) => m.mock.stop(kind));
}

/**
 * Closes one open signal-bot position now, exit reason "manual" (mirrors
 * `bot_close_position` in bot/commands.rs). Paper: booked at the live quote.
 * LIVE: a real reduce-only market order for the exchange's own size. Rejects
 * with `positionNotFound`, or `closeNotConfirmed` when the position is still
 * open afterwards (no price, a close already in flight, or the exchange did
 * not confirm); resolves only once the position has left the book.
 */
export function botClosePosition(signalId: string, kind: BotKind): Promise<BotDeskStatus> {
  return inTauri()
    ? invoke<BotDeskStatus>("bot_close_position", { signalId, kind })
    : mock((m) => m.mock.closePosition(signalId, kind));
}

/** The exact text the user types to switch a bot to real money (mirrors
 *  `LIVE_CONFIRMATION` in bot/commands.rs). */
export const LIVE_CONFIRMATION = "LIVE";

/**
 * Switches one bot's real-money trading on or off. Rust accepts ON only for a
 * live build, the Futures bot on Binance, `confirmation.trim() === "LIVE"` and
 * a Binance key in the unlocked vault; anything else rejects with a string the
 * UI must show. OFF needs no confirmation (send ""). Live is never restored
 * after an app restart.
 */
export function botSetLive(kind: BotKind, enabled: boolean, confirmation: string): Promise<BotDeskStatus> {
  return inTauri()
    ? invoke<BotDeskStatus>("bot_set_live", { kind, enabled, confirmation })
    : mock((m) => m.mock.setLive(kind, enabled, confirmation));
}

/**
 * Defaults for a NEW bot (risk sizing at 1% per trade today), fetched from
 * the server rather than re-declared here — so a change to the server's
 * defaults (e.g. the default risk %) never has to be hunted down in two
 * places. Saved configs keep whatever they already carry; this is only for
 * a bot that has never been configured.
 */
export function botDefaultConfig(kind: BotKind, exchangeId: string): Promise<BotConfig> {
  return inTauri()
    ? invoke<BotConfig>("bot_default_config", { kind, exchangeId })
    : mock((m) => m.mock.defaultConfig(kind, exchangeId));
}

/** True when a position was filled with real money rather than simulated. */
export function isLivePosition(pos: OpenPosition): boolean {
  return pos.live;
}
