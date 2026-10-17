/**
 * DEV-ONLY browser preview of "Before you trade": the live payload of
 * 17 Oct 2026 02:54Z (beforeYouTrade.fixture.json), received "now".
 * `?byt=offline` pictures the cached copy after a failed fetch (3 h old);
 * `?byt=down` a first fetch that failed with nothing cached.
 */

import fixture from "./beforeYouTrade.fixture.json";
import type { BeforeYouTrade, BeforeYouTradeView } from "./beforeYouTrade";

export function beforeYouTrade(_refresh: boolean): BeforeYouTradeView {
  const mode = new URLSearchParams(window.location.search).get("byt");
  if (mode === "down") throw "bytUnavailable";
  const payload = fixture as unknown as BeforeYouTrade;
  if (mode === "offline") return { payload, fetchedAtMs: Date.now() - 3 * 3_600_000, stale: true, error: "bytUnavailable" };
  return { payload, fetchedAtMs: Date.now(), stale: false, error: null };
}
