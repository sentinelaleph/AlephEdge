/**
 * TradingView alerts IPC bridge (mirrors src-tauri/src/tvlink).
 *
 * The webhook URL comes back once, from `tvCreateUrl`, and is never stored:
 * Rust does not keep it and the server keeps only its hash. Losing it means
 * rotating. Everything else here is the relay's status, the two switches and
 * the last results the background reader recorded.
 *
 * Outside Tauri, `npm run dev` gets an in-memory preview through `devMock`;
 * the released bundle contains neither the mock nor its values.
 */

import { devMock, inTauri, invoke } from "../bridge";

type Mock = typeof import("./tvlink.mock");
const mock = <T>(run: (m: Mock) => Promise<T>) => devMock(() => import("./tvlink.mock"), run);

/** The relay's view (`GET /api/edge/tv-webhook`); times are UNIX ms. */
export interface TvServerStatus {
  active: boolean;
  createdAt: number | null;
  lastReceivedAt: number | null;
  received24h: number;
  pending: number;
}

export interface TvPrefs {
  /** Read alerts and record them. */
  receive: boolean;
  /** Run them as paper trades (implies receive). Off by default. */
  execute: boolean;
}

/** `received`, `opened`, `closed`, or a refusal code (`key|detail`). */
export interface TvAlertResult {
  eventId: string;
  receivedAt: number | null;
  handledAt: number;
  action: "buy" | "sell" | "close" | null;
  symbol: string | null;
  bot: "futures" | "spot" | null;
  outcome: string;
  positionId: string | null;
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

export function tvResults(): Promise<TvAlertResult[]> {
  return inTauri() ? invoke<TvAlertResult[]>("tv_results") : mock((m) => m.mock.results());
}

/** The alert message, one per action. `{{ticker}}` is TradingView's own placeholder. */
export const TV_TEMPLATES = {
  buy: '{"action":"buy","symbol":"{{ticker}}","tp_pct":3,"sl_pct":1.5,"bot":"futures"}',
  sell: '{"action":"sell","symbol":"{{ticker}}","tp_pct":3,"sl_pct":1.5,"bot":"futures"}',
  close: '{"action":"close","symbol":"{{ticker}}","bot":"futures"}',
} as const;
export type TvTemplate = keyof typeof TV_TEMPLATES;
