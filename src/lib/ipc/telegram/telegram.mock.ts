/**
 * DEV-ONLY in-memory Telegram state for the browser preview (no Rust in
 * `npm run dev`). Reached through `devMock`, so the released bundle drops
 * this module. It mirrors the Rust rules; it is not the authority. No
 * network call is made: `link` stands in for the user's `/start <code>`.
 */

import type { TelegramPrefs, TelegramStatus } from "./telegram";

const TOKEN = /^\d{1,20}:[A-Za-z0-9_-]{30,64}$/;

function initial(): TelegramStatus {
  return {
    tokenSet: false,
    botUsername: null,
    linked: false,
    chatLabel: null,
    linking: null,
    prefs: {
      enabled: false,
      commandsEnabled: false,
      categories: { killSwitch: true, botError: true, stopLoss: true, dealClosed: true },
    },
    lastError: null,
    lastSentAt: null,
  };
}

export const mock = (() => {
  let s = initial();
  const snap = (): Promise<TelegramStatus> => Promise.resolve(structuredClone(s));
  const unlink = () => {
    s = { ...s, linked: false, chatLabel: null, linking: null, prefs: { ...s.prefs, enabled: false, commandsEnabled: false } };
  };
  return {
    status: snap,
    setToken(token: string) {
      if (!TOKEN.test(token.trim())) return Promise.reject("telegramTokenFormat");
      unlink();
      s = { ...s, tokenSet: true, botUsername: "aleph_preview_bot", lastError: null };
      return snap();
    },
    remove() {
      unlink();
      s = { ...s, tokenSet: false, botUsername: null, lastError: null, lastSentAt: null };
      return snap();
    },
    linkCode() {
      if (!s.tokenSet) return Promise.reject("telegramNoToken");
      s = { ...s, linking: { code: "K7M2Q9XR", expiresAtMs: Date.now() + 10 * 60_000 } };
      return snap();
    },
    setPrefs(prefs: TelegramPrefs) {
      if ((prefs.enabled || prefs.commandsEnabled) && !s.linked) return Promise.reject("telegramNotLinked");
      s = { ...s, prefs: structuredClone(prefs) };
      return snap();
    },
    test() {
      if (!s.tokenSet) return Promise.reject("telegramNoToken");
      if (!s.linked) return Promise.reject("telegramNotLinked");
      s = { ...s, lastSentAt: Date.now() };
      return snap();
    },
    /** Stands in for the user sending `/start <code>` to the bot. */
    link(label: string) {
      s = { ...s, linked: true, chatLabel: label, linking: null, prefs: { ...s.prefs, enabled: true } };
    },
    setError(code: string | null) {
      s = { ...s, lastError: code };
    },
    reset() {
      s = initial();
    },
  };
})();
