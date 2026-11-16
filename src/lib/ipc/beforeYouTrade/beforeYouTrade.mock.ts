/**
 * DEV-ONLY browser preview of "Before you trade": the live payload of
 * 17 Oct 2026 02:54Z, received "now".
 *
 * Default: the payload as the staged server (schema_minor 2) serves it, with
 * its codes (beforeYouTrade.coded.fixture.json, made from the live one), the
 * farm's signal bots (export of 11:37Z) and checked translations of the
 * digest lines (line 1 in every locale, line 2 in Spanish only).
 * `?byt=legacy` the live payload exactly as today's server sends it (no codes).
 * `?byt=future` codes this app version does not know (generic lines).
 * `?byt=newerfailed` the newest digest run failed: the previous digest is shown.
 * `?byt=knife` schema_minor 3: coins the scanner withheld a long on (rs96 gate).
 * `?byt=offline` pictures the cached copy after a failed fetch (3 h old);
 * `?byt=down` a first fetch that failed with nothing cached.
 */

import coded from "./beforeYouTrade.coded.fixture.json";
import legacy from "./beforeYouTrade.fixture.json";
import type { BeforeYouTrade, BeforeYouTradeView } from "./beforeYouTrade";

/** Every code replaced by one this app does not know (a later server). */
export function withUnknownCodes(p: BeforeYouTrade): BeforeYouTrade {
  const walk = (x: unknown): unknown => {
    if (Array.isArray(x)) return x.map(walk);
    if (x && typeof x === "object") {
      const out: Record<string, unknown> = {};
      for (const [k, v] of Object.entries(x)) {
        if ((k === "code" || k.endsWith("_code")) && typeof v === "string" && v) out[k] = `${v}_v99`;
        else out[k] = walk(v);
      }
      return out;
    }
    return x;
  };
  return walk(p) as BeforeYouTrade;
}

/** The languages the page asked for, in order (tests). */
export const requestedLangs: (string | undefined)[] = [];

export function beforeYouTrade(_refresh: boolean, lang?: string): BeforeYouTradeView {
  requestedLangs.push(lang);
  const mode = new URLSearchParams(window.location.search).get("byt");
  if (mode === "down") throw "bytUnavailable";
  let payload = (mode === "legacy" ? legacy : coded) as unknown as BeforeYouTrade;
  if (mode === "future") payload = withUnknownCodes(payload);
  if (mode === "newerfailed")
    payload = { ...payload, news: { ...payload.news, newer_run_at: "2026-10-17T02:02:00Z", newer_run_status: "error", newer_run_code: "news_newer_run_failed_v1" } };
  if (mode === "knife")
    payload = {
      ...payload,
      market: {
        ...payload.market,
        falling_knife: {
          status: "ok",
          as_of: "2026-11-03T12:18:00Z",
          source: "scanner rs96 gate",
          window_hours: 24,
          total: 3,
          coins: [
            { symbol: "BLURUSDT", withheld_at: "2026-11-03T12:10:00Z" },
            { symbol: "PEPEUSDT", withheld_at: "2026-11-03T11:40:00Z" },
          ],
          rule: "The scanner does not give a long on a coin that fell more than 5.4% against BTC over the last 96 hours.",
          rule_code: "rs96_falling_knife_v1",
        },
      },
    };
  if (mode === "offline") return { payload, fetchedAtMs: Date.now() - 3 * 3_600_000, stale: true, error: "bytUnavailable" };
  return { payload, fetchedAtMs: Date.now(), stale: false, error: null };
}
