/**
 * Telegram notifications IPC bridge (mirrors src-tauri/src/telegram).
 *
 * The bot token goes IN once (`telegramSetToken`) and never comes back: Rust
 * keeps it in the OS keychain and reports only `tokenSet` and the bot's
 * @username. The chat id stays in Rust too; the UI sees the chat's label.
 *
 * Outside Tauri, `npm run dev` gets an in-memory preview through `devMock`;
 * the released bundle contains neither the mock nor its values.
 */

import { devMock, inTauri, invoke } from "../bridge";

type Mock = typeof import("./telegram.mock");
const mock = <T>(run: (m: Mock) => Promise<T>) => devMock(() => import("./telegram.mock"), run);

/** The alert categories (src/app/alerts.ts) Rust has a real source for. */
export const TELEGRAM_CATEGORIES = ["killSwitch", "botError", "stopLoss", "dealClosed"] as const;
export type TelegramCategory = (typeof TELEGRAM_CATEGORIES)[number];
export type TelegramCategories = Record<TelegramCategory, boolean>;

export interface TelegramPrefs {
  /** Send notifications to the linked chat. */
  enabled: boolean;
  /** Answer /status and /stop from the linked chat. Off by default. */
  commandsEnabled: boolean;
  categories: TelegramCategories;
}

/** Which bot sends: Sentinel's official bot, or the user's own. */
export type TelegramMode = "official" | "own";

/** The official bot (@SentinelAleph_bot), through ribqa.com. */
export interface TelegramOfficial {
  /** The bot's @username without the @. */
  bot: string;
  /** From the last server read; null before one. */
  available: boolean | null;
  /** null until checked. */
  signedIn: boolean | null;
  linked: boolean;
  chatLabel: string | null;
  linkedAt: number | null;
  /** A code waiting for `/start <code>`; null when none is live. */
  linking: { code: string; expiresAtMs: number; bot: string } | null;
  /** The chat blocked the bot: nothing is sent. */
  blocked: boolean;
  /** Signed out: sending is paused. */
  paused: boolean;
  /** The last send met "official bot unavailable". */
  unavailable: boolean;
  /** Lines waiting to be sent. */
  queued: number;
  /** Why the last server read failed (`errors.*` code). */
  checkError: string | null;
}

export interface TelegramStatus {
  mode: TelegramMode;
  official: TelegramOfficial;
  tokenSet: boolean;
  botUsername: string | null;
  linked: boolean;
  /** @username or first name of the linked chat. */
  chatLabel: string | null;
  /** A code waiting for `/start <code>`; null when none is live. */
  linking: { code: string; expiresAtMs: number } | null;
  prefs: TelegramPrefs;
  /** Last failure as an `errors.*` code. */
  lastError: string | null;
  lastSentAt: number | null;
  /**
   * The saved Telegram settings could not be read at start: the store key its
   * copy was kept under ("" = the store itself could not be read). Nothing is
   * saved over them until `telegramStartOver`. Absent when they were fine.
   */
  settingsUnreadable?: string;
}

/** The answer to unreadable saved settings: start from the defaults (the copy stays). */
export function telegramStartOver(): Promise<TelegramStatus> {
  return inTauri() ? invoke<TelegramStatus>("telegram_start_over") : mock((m) => m.mock.status());
}

export function telegramStatus(): Promise<TelegramStatus> {
  return inTauri() ? invoke<TelegramStatus>("telegram_status") : mock((m) => m.mock.status());
}

export function telegramSetToken(token: string): Promise<TelegramStatus> {
  return inTauri() ? invoke<TelegramStatus>("telegram_set_token", { token }) : mock((m) => m.mock.setToken(token));
}

export function telegramRemove(): Promise<TelegramStatus> {
  return inTauri() ? invoke<TelegramStatus>("telegram_remove") : mock((m) => m.mock.remove());
}

export function telegramLinkCode(): Promise<TelegramStatus> {
  return inTauri() ? invoke<TelegramStatus>("telegram_link_code") : mock((m) => m.mock.linkCode());
}

export function telegramSetPrefs(prefs: TelegramPrefs): Promise<TelegramStatus> {
  return inTauri() ? invoke<TelegramStatus>("telegram_set_prefs", { prefs }) : mock((m) => m.mock.setPrefs(prefs));
}

export function telegramTest(): Promise<TelegramStatus> {
  return inTauri() ? invoke<TelegramStatus>("telegram_test") : mock((m) => m.mock.test());
}

export function telegramSetMode(mode: TelegramMode): Promise<TelegramStatus> {
  return inTauri() ? invoke<TelegramStatus>("telegram_set_mode", { mode }) : mock((m) => m.mock.setMode(mode));
}

/** Official bot: a one-time code and its t.me link (kept in Rust). */
export function telegramOfficialLink(): Promise<TelegramStatus> {
  return inTauri() ? invoke<TelegramStatus>("telegram_official_link") : mock((m) => m.mock.officialLink());
}

/** Opens the live code's t.me link outside the app. */
export function telegramOfficialOpen(): Promise<TelegramStatus> {
  return inTauri() ? invoke<TelegramStatus>("telegram_official_open") : mock((m) => m.mock.officialOpen());
}

export function telegramOfficialUnlink(): Promise<TelegramStatus> {
  return inTauri() ? invoke<TelegramStatus>("telegram_official_unlink") : mock((m) => m.mock.officialUnlink());
}
