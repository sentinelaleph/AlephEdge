// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";
import i18n from "@/i18n";
import fixture from "@/lib/ipc/beforeYouTrade/beforeYouTrade.fixture.json";
import type { BeforeYouTrade, ChartRead, FarmRanking, ListingRow } from "@/lib/ipc/beforeYouTrade/beforeYouTrade";
import { BeforeYouTradePage } from "./BeforeYouTradePage";
import { chartReadHiddenReason, dominanceLabelKey, farmRankingShown, impactLine, reactionCell, templateFormPath, templatesInOrder } from "./bytModel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
window.matchMedia ??= ((query: string) =>
  ({ matches: false, media: query, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, onchange: null, dispatchEvent: () => false }) as MediaQueryList);

const PAYLOAD = fixture as unknown as BeforeYouTrade;

let root: Root | null = null;
let host: HTMLDivElement | null = null;
const settle = () => act(async () => new Promise((r) => setTimeout(r, 30)));

async function mount() {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root!.render(<BeforeYouTradePage />));
  await settle();
  await settle();
  return host;
}

beforeAll(async () => {
  await i18n.changeLanguage("en");
});
beforeEach(() => {
  window.history.replaceState({}, "", "/#/before-you-trade");
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

describe("bytModel", () => {
  it("names the BTC share for what it is", () => {
    expect(dominanceLabelKey("spot_volume_share")).toBe("byt.market.btcSpotShare");
    expect(dominanceLabelKey("market_cap")).toBe("byt.market.btcMarketCapDominance");
    expect(dominanceLabelKey("")).toBe("byt.market.btcDominanceUnknown");
  });

  it("never shows the chart read without its hit record and bar", () => {
    const r = PAYLOAD.market.chart_read;
    expect(chartReadHiddenReason(r)).toBeNull();
    expect(chartReadHiddenReason({ ...r, hit_record_24h: null } as ChartRead)).toBe("byt.chart.noRecord");
    expect(chartReadHiddenReason({ ...r, bar: null } as ChartRead)).toBe("byt.chart.noRecord");
    expect(chartReadHiddenReason({ ...r, status: "unavailable" } as ChartRead)).toBe("byt.chart.notOk");
  });

  it("shows the farm ranking only when the payload allows it", () => {
    const base: FarmRanking = { status: "allowed", rule: "r", min_days: 30, min_closed_cycles_per_bot: 10, min_closed_trades_signal_bot: 30, engine_check_passed: true, reasons: [], ranked: [{ unit: "T01", return_pct: 1 }] };
    expect(farmRankingShown(base)).toBe(true);
    expect(farmRankingShown({ ...base, status: "notAllowed" })).toBe(false);
    expect(farmRankingShown({ ...base, reasons: ["0.02 of 30 days"] })).toBe(false);
    expect(farmRankingShown({ ...base, ranked: [] })).toBe(false);
    expect(farmRankingShown(undefined)).toBe(false);
    expect(farmRankingShown(PAYLOAD.presets.farm.labels?.ranking)).toBe(false);
  });

  it("keeps the payload's T01..T12 order even when results would sort differently", () => {
    const p = structuredClone(PAYLOAD.presets);
    p.lab.templates[0].splits = p.lab.templates[0].splits.map((s) => ({ ...s, mean_per_bot_month_pct: -50 }));
    expect(templatesInOrder(p).map((t) => t.template_id)).toEqual(["T01", "T02", "T03", "T04", "T05", "T06", "T07", "T08", "T09", "T10", "T11", "T12"]);
    expect(templateFormPath(p.lab.templates[0])).toBe("/bots/dca/new?preset=dca_long_classic");
  });

  it("says a news impact as measured, measuring, or nothing", () => {
    expect(impactLine({ status: "measuring", n: 12, needed: 30 })).toEqual({ kind: "measuring", n: 12, needed: 30 });
    expect(impactLine(undefined)).toEqual({ kind: "none" });
    const l = impactLine(PAYLOAD.news.impact.types[0].horizons["24h"]);
    expect(l.kind).toBe("measured");
  });

  it("a listing without candles says No data with its reason", () => {
    const row = PAYLOAD.listings.recent[0];
    expect(reactionCell(row)).toEqual({ kind: "noData", values: [], reason: "DataHub has no bybit candles" });
    expect(reactionCell({ ...row, reaction: null } as ListingRow).kind).toBe("noData");
    expect(reactionCell({ ...row, reaction: { status: "ok", ref_time: null, ret_5m_pct: 1, ret_1h_pct: null, ret_24h_pct: 2 } } as ListingRow).kind).toBe("measured");
  });
});

describe("Before you trade page (live fixture of 17 Oct 2026)", () => {
  it("labels the BTC share as spot volume share, with the chart read and its record", async () => {
    const el = await mount();
    const text = el.textContent ?? "";
    expect(text).toContain("BTC share of spot volume");
    expect(text).not.toContain("market-cap dominance");
    const chart = el.querySelector('[data-testid="byt-chart-read"]')!;
    expect(chart.textContent).toContain("Lower");
    const rec = el.querySelector('[data-testid="byt-hit-record"]')!.textContent!;
    expect(rec).toContain("47.6%");
    expect(rec).toContain("59 of 124 reads, 27 days");
    expect(rec).toContain("Always-range baseline: 58.9%");
    expect(chart.textContent).toContain("Veto bar not met");
    expect(text).toMatch(/ATR 14 days.*n=14 daily bars/);
  });

  it("lists the 12 templates in order, Proven only for T01, and opens New bot from a row", async () => {
    const el = await mount();
    const table = [...el.querySelectorAll("table")].find((t) => t.textContent?.includes("T12"))!;
    const ids = [...table.querySelectorAll("tbody tr")].map((r) => /T\d\d/.exec(r.textContent ?? "")?.[0]).filter(Boolean);
    expect(ids).toEqual(["T01", "T02", "T03", "T04", "T05", "T06", "T07", "T08", "T09", "T10", "T11", "T12"]);
    const chips = [...table.querySelectorAll(".ae-chip")].map((c) => c.textContent);
    expect(chips.filter((c) => c === "Proven")).toHaveLength(1);
    expect(chips.filter((c) => c === "Experimental")).toHaveLength(11);
    const first = [...table.querySelectorAll("tbody tr")].find((r) => r.textContent?.includes("T01"))!;
    expect(first.textContent).toContain("Proven");
    const open = [...first.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.title === "Open in New bot")!;
    await act(async () => open.click());
    expect(window.location.hash).toBe("#/bots/dca/new?preset=dca_long_classic");
  });

  it("ghost and farm are measuring and unranked; similar days says why it is absent", async () => {
    const el = await mount();
    expect(el.querySelector('[data-testid="byt-ghost"]')!.textContent).toContain("Measuring: 8.9 of 30 days.");
    const farm = el.querySelector('[data-testid="byt-farm"]')!;
    expect(farm.textContent).toContain("Paper, simulated fills");
    expect(el.querySelector('[data-testid="byt-farm-ranking"]')).toBeNull();
    expect(el.querySelector('[data-testid="byt-farm-noranking"]')!.textContent).toContain("0.02 of 30 days since T0");
    expect(el.textContent).toContain("A pre-registered test of choosing presets by market regime failed");
  });

  it("says each news impact with its n and the random-hours baseline", async () => {
    const el = await mount();
    const impact = el.querySelector('[data-testid="byt-impact"]')!.textContent!;
    expect(impact).toContain(
      "After Regulation and legal news, BTC 24h: median +0.03%, up 53.2% (n=139). Random hours: up 55.4%. No measurable difference.",
    );
    expect(el.textContent).toContain("2 of 2 lines shown.");
  });

  it("shows listings with exchange, market and their measured reaction or No data", async () => {
    const el = await mount();
    const text = el.textContent ?? "";
    expect(text).toContain("Upcoming (0 of 0)");
    expect(text).toContain("Recent (6 of 6)");
    expect(text).toContain("No data · DataHub has no bybit candles");
    expect(text).toContain("never read successfully, so absent here: upbit");
    expect(text).toContain("upbit (http_error)");
  });

  it("uses no arrows and no bullish/bearish words", async () => {
    const el = await mount();
    expect(el.textContent).not.toMatch(/bullish|bearish|[↑↓▲▼⬆⬇]/i);
  });

  it("offline: the cached copy is shown with its age and the reason", async () => {
    window.history.replaceState({}, "", "/?byt=offline#/before-you-trade");
    const el = await mount();
    const banner = el.querySelector('[data-testid="byt-stale"]')!;
    expect(banner.textContent).toContain("Offline copy, received 3 hr ago.");
    expect(banner.textContent).toContain("Sentinel did not answer.");
    expect(el.textContent).toContain("Market today");
  });

  it("nothing cached and no answer: an honest error, no figures", async () => {
    window.history.replaceState({}, "", "/?byt=down#/before-you-trade");
    const el = await mount();
    expect(el.textContent).toContain("Before you trade is not available");
    expect(el.textContent).not.toContain("BTC price");
  });
});
