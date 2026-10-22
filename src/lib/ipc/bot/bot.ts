/**
 * Bot-desk IPC bridge (mirrors src-tauri/src/bot). One build: every bot is
 * Paper or Live (`BotConfig.mode`), Paper by default; a Live bot trades only
 * once armed this session (`live`, the typed LIVE). `npm run dev` gets an
 * idle, dev-only desk through `devMock`; the released bundle has none.
 */

import { devMock, inTauri, invoke } from "../bridge";
import type { SignalDirection } from "../signal/signal";

type Mock = typeof import("./bot.mock");
const mock = <T>(run: (m: Mock) => Promise<T>) => devMock(() => import("./bot.mock"), run);

/** `tradingview`: the TradingView bot, which trades the user's own alerts (never Sentinel's signals). */
export type BotKind = "futures" | "spot" | "pump" | "tradingview";

/** The signal bots (Sentinel's signals). */
export const SIGNAL_KINDS = ["futures", "spot", "pump"] as const;

/** Every bot of the book: the signal bots and the TradingView bot. */
export const ALL_KINDS: readonly BotKind[] = [...SIGNAL_KINDS, "tradingview"];

/** Whether `kind`'s bot is running (desk status). */
export function kindRunning(s: BotDeskStatus, kind: BotKind): boolean {
  switch (kind) {
    case "futures":
      return s.futuresRunning;
    case "spot":
      return s.spotRunning;
    case "pump":
      return s.pumpRunning;
    case "tradingview":
      return s.tradingviewRunning === true;
  }
}

/**
 * The TradingView bot's alert limits (mirrors `TvSettings` in bot/model.rs),
 * enforced by Rust on every alert. Its `maxPositions` is the position cap,
 * `leverage` the leverage cap, `symbols` the allow list; Rust derives
 * `capital` = budget / positions.
 */
export interface TvSettings {
  name: string;
  budgetUsdt: number;
  maxSizeUsdt: number;
  maxOpensPerDay: number;
  /** 5–120 s; an older alert (server clock) never runs. */
  staleS: number;
  /** Refuse when the live price is more than this % from the alert's price. */
  priceGuardPct: number;
  /** Refuse an entry without `price`. */
  requirePrice: boolean;
  /** Present when a passphrase is set: its Argon2id hash (the text is never sent back). */
  passphraseHash?: string;
  /** A passphrase saved by 0.3.19 (salted SHA-256), until its first matching alert rehashes it. */
  passphraseSha256?: string;
  passphraseSalt?: string;
  /** Form only: a new passphrase, "" to clear, absent to keep. */
  passphrase?: string;
}

/** Whether the TradingView bot has a passphrase, in either stored format. */
export function tvPassphraseSet(tv: TvSettings | undefined): boolean {
  return Boolean(tv?.passphraseHash || tv?.passphraseSha256);
}

/** Limits of TvSettings, as Rust checks them. */
export const TV_LIMITS = { staleMin: 5, staleMax: 120, staleDefault: 30, guardMax: 10, guardDefault: 0.5, opensMax: 200, sizeMin: 5 } as const;

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
   * Per-position loss cap in percent OF THE BOT'S CAPITAL: a position is
   * closed once its unrealized NET loss (after leverage and both fees)
   * reaches −maxLossPct%. 2 at fixed 3x ≈ a 0.57% price move; in risk
   * sizing a cap at or above the risk % never fires (the stop is first).
   * See `maxLossEffect` in lib/sizing.ts. Applies at all times, independent
   * of the BTC regime. Absent = off.
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
   * Armed for real orders THIS session (the typed LIVE). Server-owned and
   * read-only here: `bot_configure` ignores it; only `botSetLive` changes it.
   * Never restored after a restart: a Live bot comes back stopped, unarmed.
   */
  live?: boolean;
  /** Paper or Live, stored with the bot (absent = Paper). Server-owned like `live`. */
  mode?: TradeMode;
  /** The TradingView bot's alert limits (kind `tradingview` only). */
  tv?: TvSettings;
}

/** How a bot trades (mirrors `TradeMode` in bot/model.rs). */
export type TradeMode = "paper" | "live";

/** The bot's mode: Live when stored as Live (or armed), else Paper. */
export function botMode(cfg: BotConfig | null | undefined): TradeMode {
  return cfg && (cfg.mode === "live" || cfg.live === true) ? "live" : "paper";
}

/** A Live bot that still needs its typed LIVE this session before it may start. */
export function needsArm(cfg: BotConfig | null | undefined): boolean {
  return botMode(cfg) === "live" && cfg?.live !== true;
}

/**
 * The money one real-money confirmation shows and sends back (mirrors
 * `LiveTerms` in bot/live_terms.rs). The command recomputes it and refuses
 * with `liveTermsChanged` when the settings moved under the dialog.
 */
export interface LiveTerms {
  venue: string;
  /** Most margin on the exchange at once; on isolated margin, the most it can lose. */
  marginAtRiskUsdt: number;
  /** Largest ONE position at full size (the pilot caps the first entries lower). */
  maxNotionalUsdt: number;
  /** The margin that largest position ties up (`maxNotionalUsdt / leverage`). */
  maxPositionMarginUsdt?: number;
  leverage: number;
  positions: number;
  /** First `pilotCount` real entries (signal) or cycles (DCA / Grid) capped near `pilotNotionalUsdt`. */
  pilotCount: number;
  pilotNotionalUsdt: number;
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
  direction: SignalDirection;
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
  /** Opened by the user on one signal (Execute), not by the bot's loop. Absent = false. */
  manual?: boolean;
}

/**
 * What "Open position" on one signal would do now (mirrors `TakePreview` in
 * bot/engine/manual.rs): the paper fill at the live price, the bot's sizing
 * and target, NET loss at the stop and gain at the target with both fees.
 */
export interface TakePreview {
  botKind: BotKind;
  signalId: string;
  symbol: string;
  direction: SignalDirection;
  /** Paper fill at the live price (at market inside the stop–target band). */
  entry: number;
  /** The signal's published entry. */
  publishedEntry: number;
  stop: number;
  target: number;
  tpTarget: string;
  /** Reward:risk at the fill, and to the same target from the published entry. */
  rrAtFill: number;
  rrPublished: number;
  /** `rrAtFill` below 0.5 (Rust `LOW_RR_AT_FILL`). */
  rrLow: boolean;
  notionalUsdt: number;
  capital: number;
  effectiveLeverage: number;
  riskCapped: boolean;
  /** Negative, fees included. */
  lossAtStopUsdt: number;
  gainAtTargetUsdt: number;
  /** Both taker fees on the notional. */
  feesUsdt: number;
}

export interface SkipNote {
  atMs: number;
  symbol: string;
  reason: string;
  /**
   * For most reasons: a preformatted value the UI interpolates as-is (e.g.
   * "+0.120%"). For "vetoClosed" only (a veto closing one or more open
   * positions): `"<count>|<reason_text>"`, split back apart by `skipParams`
   * in lib/skipNotes.ts.
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
  /** The TradingView bot, when one was made. */
  tradingview?: BotConfig | null;
  futuresRunning: boolean;
  spotRunning: boolean;
  pumpRunning: boolean;
  tradingviewRunning?: boolean;
  /** Real entries left in the TradingView bot's own pilot. */
  tradingviewPilotLeft?: number;
  openPositions: OpenPosition[];
  recentSkips: SkipNote[];
  killSwitchTripped: boolean;
  /** Sentinel's BTC regime as the engine last read it. */
  btcRegime: "normal" | "break" | "unknown";
  /** Real entries left in the live Futures bot's pilot (0 = full size). */
  pilotLeft?: number;
  /**
   * Venues real money can be switched on for now (Rust `live_venues`): those
   * whose order path passed a test-network dry run, every order venue in the
   * sandbox. Absent before the first status (see `liveVenueAllowed`).
   */
  liveVenues?: string[];
  /**
   * Bybit / OKX orders go to their demo / test networks, not to a real
   * account (Rust `ALEPH_EDGE_VENUE_SANDBOX=1`).
   */
  venueSandbox?: boolean;
  /** Each open position at the last price the engine read (positionMarks.ts). */
  marks?: PositionMark[];
  /** Each open position at its current stop (riskReadout.ts). */
  atStop?: PositionAtStop[];
  /**
   * Real-money position rows the book does not manage ("kind signalId"):
   * unreadable, or on a Paper bot. Money in play; shown until a restart reads them.
   */
  liveUnreadable?: string[];
  /** Why no bot may start now (risk settings or the store unreadable), an error code, or null. */
  tradingBlocked?: string | null;
}

/**
 * An open position at its current stop (the entry once breakeven armed):
 * what that fill would book, fees included, the trade record's formula.
 */
export interface PositionAtStop {
  signalId: string;
  botKind: BotKind;
  stop: number;
  pnlUsdt: number;
}

/**
 * An open position at the last price the engine read for it, and what
 * closing it there would book: the trade record's formula, fees and a banked
 * partial included, as a share of the position's capital.
 */
export interface PositionMark {
  signalId: string;
  botKind: BotKind;
  price: number;
  atMs: number;
  pnlPct: number;
  pnlUsdt: number;
}

/** Largest real entry while a signal bot's pilot runs (model.rs PILOT_NOTIONAL_USDT). */
export const PILOT_NOTIONAL_USDT = 50;

/** Ends a live bot's pilot: entries go back to full size. */
export function botEndPilot(kind: BotKind): Promise<BotDeskStatus> {
  return inTauri() ? invoke<BotDeskStatus>("bot_end_pilot", { kind }) : mock((m) => m.mock.status());
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

/**
 * Opens one signal on a signal bot's PAPER book now (mirrors
 * `bot_take_signal`). The bot's own filters are skipped, its risk limits
 * are not. Rejects with a code (`key` or `key|detail`): `manualTakePaperOnly`
 * for a bot on real money, a skip reason (`bots.skipReasons.*`) otherwise.
 */
export function botTakeSignal(kind: BotKind, signalId: string): Promise<OpenPosition> {
  return inTauri() ? invoke<OpenPosition>("bot_take_signal", { kind, signalId }) : mock((m) => m.mock.takeSignal(kind));
}

/** What `botTakeSignal` would open now, or the same refusal. */
export function botPreviewTake(kind: BotKind, signalId: string): Promise<TakePreview> {
  return inTauri() ? invoke<TakePreview>("bot_preview_take", { kind, signalId }) : mock((m) => m.mock.takeSignal(kind));
}

/**
 * Every "New trade" position's id starts with this (Rust
 * `user_trade::ID_PREFIX`); a Sentinel signal id never does.
 */
export const USER_TRADE_ID_PREFIX = "manual-";

/** Whether a position or trade is a "New trade" (the user's own levels), not a Sentinel signal. */
export function isUserTradeId(signalId: string): boolean {
  return signalId.startsWith(USER_TRADE_ID_PREFIX);
}

/** The "New trade" form (mirrors `TradeRequest` in bot/engine/user_trade.rs). */
export interface TradeRequest {
  /** Futures or Spot (Pump is refused). */
  kind: BotKind;
  symbol: string;
  direction: SignalDirection;
  /** USDT notional; absent = the bot's own sizing. */
  notionalUsdt?: number;
  takeProfit: number;
  stopLoss: number;
  /** Paper (default) or Live: must match the bot; Live needs the armed Live Futures bot. */
  mode?: TradeMode;
  /** The typed word, for a Live trade. */
  confirmation?: string;
  /** The margin the Live confirmation showed (the preview's `capital`); Rust re-plans and refuses `liveTermsChanged` when it moved. */
  confirmedMarginUsdt?: number;
}

/** What `botOpenTrade` would open now (mirrors `TradePreview` in user_trade.rs). */
export interface TradePreview {
  botKind: BotKind;
  symbol: string;
  direction: SignalDirection;
  /** Paper fill: the live price. */
  entry: number;
  takeProfit: number;
  stopLoss: number;
  qty: number;
  notionalUsdt: number;
  /** The position's margin. */
  capital: number;
  /** The bot's configured capital, the base of `riskPctOfCapital`. */
  botCapital: number;
  sizingMode: SizingMode;
  effectiveLeverage: number;
  /** Isolated exchange leverage (1 for spot). */
  leverage: number;
  riskCapped: boolean;
  /** Negative, fees included. */
  lossAtStopUsdt: number;
  riskPctOfCapital: number;
  gainAtTargetUsdt: number;
  rewardRisk: number;
  /** Null for spot and a 1x long. */
  liquidationPrice: number | null;
  feesUsdt: number;
}

/**
 * "New trade": opens ONE position on a signal bot's PAPER book at the live
 * price with the user's own take-profit and stop (mirrors `bot_open_trade`).
 * Rejects with a code (`key` or `key|detail`): `manualTakePaperOnly`, a
 * `trade*` code, or a skip reason (`bots.skipReasons.*`).
 */
export function botOpenTrade(request: TradeRequest): Promise<OpenPosition> {
  return inTauri() ? invoke<OpenPosition>("bot_open_trade", { request }) : mock((m) => m.mock.trade(request));
}

/** What `botOpenTrade` would open now, or the same refusal. */
export function botPreviewTrade(request: TradeRequest): Promise<TradePreview> {
  return inTauri() ? invoke<TradePreview>("bot_preview_trade", { request }) : mock((m) => m.mock.trade(request));
}

/** The exact text the user types to switch a bot to real money (mirrors
 *  `LIVE_CONFIRMATION` in bot/commands.rs). */
export const LIVE_CONFIRMATION = "LIVE";

/**
 * Real money for one signal bot: "Go live" on a Paper bot and the
 * per-session arm of a Live bot. Rust accepts ON only for the Futures bot on
 * a dry-run venue, `confirmation.trim() === "LIVE"`, a verified trade-only key
 * in the unlocked vault, no paper position in its book and `atRiskUsdt` equal
 * to the margin the confirmation showed (`botLiveTerms`); anything else
 * rejects with a code the UI must show. OFF needs no confirmation (send ""):
 * the bot is disarmed and stopped, and becomes Paper once it holds no real
 * position. The arm is never restored after a restart.
 */
export function botSetLive(kind: BotKind, enabled: boolean, confirmation: string, atRiskUsdt?: number): Promise<BotDeskStatus> {
  return inTauri()
    ? invoke<BotDeskStatus>("bot_set_live", { kind, enabled, confirmation, atRiskUsdt: atRiskUsdt ?? null })
    : mock((m) => m.mock.setLive(kind, enabled, confirmation));
}

/** The money the typed LIVE would put at risk on a signal bot's saved settings. */
export function botLiveTerms(kind: BotKind): Promise<LiveTerms> {
  return inTauri() ? invoke<LiveTerms>("bot_live_terms", { kind }) : mock((m) => m.mock.liveTerms(kind));
}

/**
 * "Close positions and switch" (mirrors `bot_close_live_and_paper`): the
 * Live bot is disarmed and stopped, each of ITS real positions is closed
 * with a reduce-only market order (the manual close path), and it becomes
 * Paper once none is left. Rejects with `closeNotConfirmed|n` when some are
 * still open (the bot then stays a disarmed Live bot). Never touches another
 * bot's positions.
 */
export function botCloseLiveAndPaper(kind: BotKind): Promise<BotDeskStatus> {
  return inTauri() ? invoke<BotDeskStatus>("bot_close_live_and_paper", { kind }) : mock((m) => m.mock.closeLiveAndPaper(kind));
}

/** What "Stop all live" did (mirrors `LiveStopReport`). */
export interface LiveStopReport {
  signalStopped: number;
  strategyStopped: number;
}

/**
 * "Stop all live": every Live bot stops taking new entries or cycles and is
 * disarmed. Open real positions stay managed under their exchange stops.
 */
export function liveStopAll(): Promise<LiveStopReport> {
  return inTauri() ? invoke<LiveStopReport>("live_stop_all") : mock((m) => m.mock.stopAllLive());
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
