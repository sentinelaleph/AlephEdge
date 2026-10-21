/**
 * TradingView alerts IPC bridge (mirrors src-tauri/src/tvlink).
 *
 * The webhook URL comes back once, from `tvCreateUrl`, and is never stored:
 * Rust does not keep it and the server keeps only its hash. Losing it means
 * rotating. Everything else here is the relay's status, the two switches,
 * the server's offline notice and the last results the background reader
 * recorded. Alerts run on the TradingView bot (bot kind `tradingview`).
 *
 * Outside Tauri, `npm run dev` gets an in-memory preview through `devMock`;
 * the released bundle contains neither the mock nor its values.
 */

import { devMock, inTauri, invoke, listenEvent, type Unlisten } from "../bridge";

type Mock = typeof import("./tvlink.mock");
const mock = <T>(run: (m: Mock) => Promise<T>) => devMock(() => import("./tvlink.mock"), run);

/** The relay's view (`GET /api/edge/tv-webhook`); times are UNIX ms. */
export interface TvServerStatus {
  active: boolean;
  createdAt: number | null;
  lastReceivedAt: number | null;
  received24h: number;
  pending: number;
  /** The server's Telegram notice for an alert that arrives while the app is offline. */
  notifyOffline: boolean | null;
  offlineAfterS: number | null;
  /** The app's last events read or open stream, on the server's clock. */
  lastSeenAt: number | null;
}

/** `GET|PATCH /api/edge/tv-webhook/settings`. */
export interface TvNotice {
  notifyOffline: boolean;
  offlineAfterS: number;
  telegram: "linked" | "not_linked" | "unavailable" | "unknown";
}

export interface TvPrefs {
  /** Read alerts and record them. */
  receive: boolean;
  /** Run them on the TradingView bot (implies receive). Off by default. */
  execute: boolean;
}

/** `received`, `opened`, `closed`, `flipped`, `livePending|…`, or a refusal code (`key|detail`). */
export interface TvAlertResult {
  eventId: string;
  receivedAt: number | null;
  handledAt: number;
  action: "buy" | "sell" | "close" | null;
  symbol: string | null;
  bot: "futures" | "spot" | null;
  outcome: string;
  positionId: string | null;
  /** A test alert ("Send test alert"): recorded as `tvTestOnly`, never executed. */
  test: boolean;
  /** The bot that handled it (its name) and how. */
  botName?: string | null;
  mode?: "paper" | "live" | null;
  /** Quantity sized at the live price, and what filled. */
  requestedQty?: number | null;
  filledQty?: number | null;
  avgFill?: number | null;
  /** The alert's `price` ({{close}}). */
  alertPrice?: number | null;
  /** (fill − alert price) / alert price in %, positive = worse for the trade. */
  slippagePct?: number | null;
  /** Relay receive to fill, on the server's clock. */
  latencyMs?: number | null;
  /** Strategy alerts: "buy long", "sell flat"… */
  strategy?: string | null;
}

export interface TvStatus {
  signedIn: boolean;
  server: TvServerStatus | null;
  /** Why `server` is missing while signed in (`errors.*` code). */
  serverError: string | null;
  prefs: TvPrefs;
  /** Newest first, at most 100. */
  results: TvAlertResult[];
  /** Last failed read of the background reader. */
  lastError: string | null;
  lastPollAt: number | null;
  /** `stream` (server push) or `poll`; null while not reading. */
  transport?: "stream" | "poll" | null;
  /** Reading now: an open stream, or a read in the last 15 s. */
  online?: boolean;
  notice?: TvNotice | null;
  /** The user set the notice by hand: the app no longer turns it on itself. */
  noticeTouched?: boolean;
  /** Real entries whose outcome is still being looked up by client id. */
  pending?: number;
  /** Why the event stream last failed (`errors.*` code) while the poll reads; null once it streams. */
  streamError?: string | null;
}

/** The smallest order a Binance USDT-M pair takes now (mirrors `SymbolMinimum`). */
export interface TvSymbolMinimum {
  symbol: string;
  /** `ok`, `unavailable` (Binance or its price did not answer), `invalid` (not a USDT pair). */
  status: "ok" | "unavailable" | "invalid";
  price: number | null;
  minQty: number | null;
  minNotional: number | null;
  /** Lot size and MIN_NOTIONAL at the live price, rounded up to the cent. */
  minUsdt: number | null;
}

/** The smallest order `symbol` takes on Binance futures now (public exchange rules, live price). */
export function tvSymbolMinimum(symbol: string): Promise<TvSymbolMinimum> {
  return inTauri() ? invoke<TvSymbolMinimum>("tv_symbol_minimum", { symbol }) : mock((m) => m.mock.symbolMinimum(symbol));
}

/** Rust's `RESULT_EVENT`: an alert's row was written or updated (payload: its event id). */
export const TV_RESULT_EVENT = "tvlink:result";

/** Subscribes to new or updated alert rows (a no-op outside Tauri). */
export function onTvResult(handler: (eventId: string) => void): Promise<Unlisten> {
  return listenEvent<string>(TV_RESULT_EVENT, handler);
}

/** Shown once, right after create / rotate. */
export interface TvCreatedUrl {
  url: string;
  createdAt: number | null;
}

export function tvStatus(): Promise<TvStatus> {
  return inTauri() ? invoke<TvStatus>("tv_status") : mock((m) => m.mock.status());
}

export function tvCreateUrl(): Promise<TvCreatedUrl> {
  return inTauri() ? invoke<TvCreatedUrl>("tv_create_url") : mock((m) => m.mock.createUrl());
}

export function tvRevoke(): Promise<TvStatus> {
  return inTauri() ? invoke<TvStatus>("tv_revoke") : mock((m) => m.mock.revoke());
}

export function tvSetPrefs(prefs: TvPrefs): Promise<TvStatus> {
  return inTauri() ? invoke<TvStatus>("tv_set_prefs", { prefs }) : mock((m) => m.mock.setPrefs(prefs));
}

/**
 * One test alert through the relay onto the active URL (5 an hour). With
 * Receive on, Rust reads it at once; it is listed as a test and never trades.
 * Refusals: `tvNoUrl`, `tvTestRateLimited`.
 */
export function tvSendTest(): Promise<TvStatus> {
  return inTauri() ? invoke<TvStatus>("tv_send_test") : mock((m) => m.mock.sendTest());
}

export function tvResults(): Promise<TvAlertResult[]> {
  return inTauri() ? invoke<TvAlertResult[]>("tv_results") : mock((m) => m.mock.results());
}

/**
 * The server's offline notice (Telegram through @SentinelAleph_bot). Set by
 * hand here, the app no longer turns it on by itself for a Live bot.
 * Refusals: `tvNoUrl`, `tvOfflineAfterRange|10–600`.
 */
export function tvSetNotice(notifyOffline?: boolean, offlineAfterS?: number): Promise<TvStatus> {
  return inTauri()
    ? invoke<TvStatus>("tv_set_notice", { notifyOffline, offlineAfterS })
    : mock((m) => m.mock.setNotice(notifyOffline, offlineAfterS));
}
