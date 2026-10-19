/**
 * DEV-ONLY in-memory TradingView link state for the browser preview (no Rust
 * in `npm run dev`). Reached through `devMock`, so the released bundle drops
 * this module. No network call is made and no alert ever arrives on its own:
 * the results table stays empty unless a test pushes a row.
 */

import type { TvAlertResult, TvCreatedUrl, TvPrefs, TvStatus } from "./tvlink";

/** Plainly not a working address. */
export const PREVIEW_URL = "https://www.ribqa.com/api/tv/preview-not-a-real-url";

function initial(): TvStatus {
  return {
    signedIn: true,
    server: { active: false, createdAt: null, lastReceivedAt: null, received24h: 0, pending: 0 },
    serverError: null,
    prefs: { receive: false, execute: false },
    results: [],
    lastError: null,
    lastPollAt: null,
  };
}

/** The server's limit on test alerts per hour. */
const TESTS_PER_HOUR = 5;

export const mock = (() => {
  let s = initial();
  let tests = 0;
  const snap = (): Promise<TvStatus> => Promise.resolve(structuredClone(s));
  const signedOut = () => Promise.reject("tvSignedOut");
  return {
    status() {
      return s.signedIn ? snap() : Promise.resolve({ ...structuredClone(s), server: null });
    },
    createUrl(): Promise<TvCreatedUrl> {
      if (!s.signedIn) return signedOut();
      const at = Date.now();
      s = { ...s, server: { active: true, createdAt: at, lastReceivedAt: null, received24h: 0, pending: 0 } };
      return Promise.resolve({ url: PREVIEW_URL, createdAt: at });
    },
    revoke() {
      if (!s.signedIn) return signedOut();
      s = { ...s, server: { active: false, createdAt: null, lastReceivedAt: null, received24h: 0, pending: 0 } };
      return snap();
    },
    setPrefs(prefs: TvPrefs) {
      s = { ...s, prefs: { receive: prefs.receive || prefs.execute, execute: prefs.execute } };
      return snap();
    },
    /** As the relay + Rust reader would: a test row when Receive is on. */
    sendTest() {
      if (!s.signedIn) return signedOut();
      const server = s.server;
      if (!server?.active) return Promise.reject("tvNoUrl");
      if (tests >= TESTS_PER_HOUR) return Promise.reject("tvTestRateLimited");
      tests += 1;
      const at = Date.now();
      const pending = server.pending + (s.prefs.receive ? 0 : 1);
      s = { ...s, server: { ...server, lastReceivedAt: at, received24h: server.received24h + 1, pending } };
      if (s.prefs.receive) {
        const row: TvAlertResult = {
          eventId: `test-${tests}`,
          receivedAt: at,
          handledAt: at,
          action: "buy",
          symbol: "BTCUSDT",
          bot: "futures",
          outcome: "tvTestOnly",
          positionId: null,
          test: true,
        };
        s = { ...s, results: [row, ...s.results].slice(0, 100), lastPollAt: at };
      }
      return snap();
    },
    results() {
      return Promise.resolve(structuredClone(s.results));
    },
    /** Tests: a row as the Rust reader would record it. */
    push(r: TvAlertResult) {
      s = { ...s, results: [r, ...s.results].slice(0, 100) };
    },
    setSignedIn(on: boolean) {
      s = { ...s, signedIn: on };
    },
    reset() {
      s = initial();
      tests = 0;
    },
  };
})();
