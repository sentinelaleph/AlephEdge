/**
 * DEV-ONLY in-memory Telegram state for the browser preview (no Rust in
 * `npm run dev`). Reached through `devMock`, so the released bundle drops
 * this module. It mirrors the Rust rules; it is not the authority. No
 * network call is made: `link` stands in for the user's `/start <code>` to
 * their own bot, `officialLinked` for the same to the official bot.
 */

import type { TelegramMode, TelegramOfficial, TelegramPrefs, TelegramStatus } from "./telegram";

const TOKEN = /^\d{1,20}:[A-Za-z0-9_-]{30,64}$/;
const BOT = "SentinelAleph_bot";

function initialOfficial(): TelegramOfficial {
  return {
    bot: BOT,
    available: true,
    signedIn: true,
    linked: false,
    chatLabel: null,
    linkedAt: null,
    linking: null,
    blocked: false,
    paused: false,
    unavailable: false,
    queued: 0,
    checkError: null,
  };
}

function initial(): TelegramStatus {
  return {
    mode: "official",
    official: initialOfficial(),
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
  const linkedNow = () => (s.mode === "official" ? s.official.linked : s.linked);
  const unlink = () => {
    s = { ...s, linked: false, chatLabel: null, linking: null, prefs: { ...s.prefs, enabled: false, commandsEnabled: false } };
  };
  const signedOut = () => s.official.signedIn === false;
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
      const commands = s.mode === "official" ? s.prefs.commandsEnabled : prefs.commandsEnabled;
      if ((prefs.enabled || (commands && s.mode === "own")) && !linkedNow()) return Promise.reject("telegramNotLinked");
      s = { ...s, prefs: { ...structuredClone(prefs), commandsEnabled: commands } };
      return snap();
    },
    test() {
      if (s.mode === "own" && !s.tokenSet) return Promise.reject("telegramNoToken");
      if (!linkedNow()) return Promise.reject("telegramNotLinked");
      if (s.mode === "official" && signedOut()) return Promise.reject("tgSignedOut");
      s = { ...s, lastSentAt: Date.now(), official: { ...s.official, blocked: false, unavailable: false } };
      return snap();
    },
    setMode(mode: TelegramMode) {
      s = { ...s, mode, linking: null, lastError: null, official: { ...s.official, linking: null, queued: 0 } };
      return snap();
    },
    officialLink() {
      if (s.mode !== "official") return Promise.reject("tgNotOfficial");
      if (signedOut()) return Promise.reject("tgSignedOut");
      s = { ...s, official: { ...s.official, linking: { code: "Q4RT8WXM", expiresAtMs: Date.now() + 10 * 60_000, bot: BOT } } };
      return snap();
    },
    officialOpen() {
      if (!s.official.linking) return Promise.reject("tgLinkExpired");
      return snap();
    },
    officialUnlink() {
      if (signedOut()) return Promise.reject("tgSignedOut");
      s = {
        ...s,
        official: { ...s.official, linked: false, chatLabel: null, linkedAt: null, linking: null, blocked: false },
        prefs: { ...s.prefs, enabled: s.mode === "official" ? false : s.prefs.enabled },
        lastError: null,
      };
      return snap();
    },
    /** Stands in for the user sending `/start <code>` to their own bot. */
    link(label: string) {
      s = { ...s, linked: true, chatLabel: label, linking: null, prefs: { ...s.prefs, enabled: true } };
    },
    /** Stands in for the user sending `/start <code>` to the official bot. */
    officialLinked(label: string) {
      s = {
        ...s,
        official: { ...s.official, linked: true, chatLabel: label, linkedAt: Date.now(), linking: null, blocked: false },
        prefs: { ...s.prefs, enabled: s.mode === "official" ? true : s.prefs.enabled },
      };
    },
    setOfficial(patch: Partial<TelegramOfficial>) {
      s = { ...s, official: { ...s.official, ...patch } };
    },
    setError(code: string | null) {
      s = { ...s, lastError: code };
    },
    reset() {
      s = initial();
    },
  };
})();
