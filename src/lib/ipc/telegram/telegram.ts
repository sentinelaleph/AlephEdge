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

export interface TelegramStatus {
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
