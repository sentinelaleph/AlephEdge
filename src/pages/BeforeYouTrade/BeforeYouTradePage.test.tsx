// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";
import i18n from "@/i18n";
import fixture from "@/lib/ipc/beforeYouTrade/beforeYouTrade.fixture.json";
import coded from "@/lib/ipc/beforeYouTrade/beforeYouTrade.coded.fixture.json";
import { requestedLangs } from "@/lib/ipc/beforeYouTrade/beforeYouTrade.mock";
import type { BeforeYouTrade, ChartRead, FarmRanking, ListingRow } from "@/lib/ipc/beforeYouTrade/beforeYouTrade";
import { BeforeYouTradePage } from "./BeforeYouTradePage";
import { chartReadHiddenReason, dominanceLabelKey, farmRankingShown, impactLine, newsTranslation, reactionCell, templateFormPath, templatesInOrder } from "./bytModel";
import { barText, legacyReasonCode, REASON_CODES, reasonText, refusalText, sayText, TEXT_CODES } from "./bytText";

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
    expect(el.querySelector('[data-testid="byt-similar"]')!.textContent).toContain("a pre-registered test of choosing templates by market regime failed");
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
    expect(text).toContain("Never read successfully, so absent here: upbit.");
    expect(text).toContain("upbit (HTTP error)");
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

// ---------------------------------------------------------------- language

const CODED = coded as unknown as BeforeYouTrade;

/**
 * Every English sentence of the payload that is ours to translate: free text
 * (a space, 16+ characters) except third-party reporting (digest lines and
 * titles, announcement titles), the source descriptions the page never shows,
 * and names.
 */
function payloadSentences(p: unknown): string[] {
  const out = new Set<string>();
  const skip = /(\.lines\[\d+\]\.(text|title)|\.lines\[\d+\]\.translations\.[\w-]+|\.announcement\.title|\.source|\.dominance_source|\.name|\.not_rankable_reason)$/;
  const walk = (x: unknown, path: string) => {
    if (Array.isArray(x)) x.forEach((v, i) => walk(v, `${path}[${i}]`));
    else if (x && typeof x === "object") for (const [k, v] of Object.entries(x)) walk(v, `${path}.${k}`);
    else if (typeof x === "string" && x.includes(" ") && x.length >= 16 && !skip.test(path)) out.add(x);
  };
  walk(p, "$");
  return [...out];
}

/** The page's visible text without the third-party quotes it marks lang="en". */
function ownText(el: HTMLElement): string {
  const c = el.cloneNode(true) as HTMLElement;
  c.querySelectorAll('[lang="en"]').forEach((n) => n.remove());
  return c.textContent ?? "";
}

const ENGLISH_WORDS = /\b(the|and|is|are|not|was|which|been|only)\b/;

describe("Before you trade in Turkish: no English prose from the payload", () => {
  beforeAll(async () => {
    await i18n.changeLanguage("tr");
  });
  afterAll(async () => {
    await i18n.changeLanguage("en");
  });

  for (const [mode, payload] of [
    ["legacy", PAYLOAD],
    ["coded", CODED],
  ] as const) {
    it(`${mode} payload: none of its English sentences is on screen`, async () => {
      window.history.replaceState({}, "", `/${mode === "legacy" ? "?byt=legacy" : ""}#/before-you-trade`);
      const el = await mount();
      const text = ownText(el);
      expect(text).toContain("Bugünkü piyasa");
      const sentences = payloadSentences(payload);
      expect(sentences.length).toBeGreaterThan(30);
      for (const s of sentences) expect(text, s).not.toContain(s);
      // Fragments of the server's sentences, and English function words.
      for (const f of ["Veto bar", "proven =", "Measured history", "Information only", "DataHub has no", "never read successfully", "days since T0", "Not computed"])
        expect(text, f).not.toContain(f);
      expect(text).not.toMatch(ENGLISH_WORDS);
      // Said from codes and numbers instead.
      expect(el.querySelector('[data-testid="byt-bar"]')!.textContent).toContain("Veto eşiği karşılanmadı: n=60 için 56,7%");
      expect(el.querySelector('[data-testid="byt-claim-rule"]')!.textContent).toContain("Kanıtlanmış = yalnız majors evreninde");
      expect(text).toContain("DataHub'da bybit mumları yok");
      expect(text).toContain("Hiç başarıyla okunamadı, bu yüzden burada yok: upbit.");
      expect(el.querySelector('[data-testid="byt-farm-noranking"]')!.textContent).toContain("T0'dan bu yana 30 günün 0,02 günü");
      expect(el.querySelector('[data-testid="byt-similar"]')!.textContent).toContain("Hesaplanmadı.");
      expect(text).toContain("upbit (HTTP hatası)");
      // Third-party digest lines stay as quoted, marked as English.
      expect(el.querySelectorAll('[lang="en"]').length).toBe(payload.news.lines.length);
      // Announcement titles are on hover only, never the link text.
      for (const r of payload.listings.recent) if (r.announcement.title) expect(text).not.toContain(r.announcement.title);
    });
  }

  it("right panel: short translated facts, not the long paragraph", async () => {
    const el = await mount();
    const items = [...el.querySelectorAll('[data-testid="byt-about"] li')].map((li) => li.textContent ?? "");
    expect(items.length).toBeGreaterThanOrEqual(3);
    expect(items[0]).toContain("tavsiye ya da tahmin değildir");
    expect(items.length).toBeLessThanOrEqual(4);
    expect(items.join(" ")).toContain("En çok işlem gören 30 paritede aynı ayarlar 3 dilimin 2 tanesinde zarar etti; en kötü bot 97,7% düştü.");
    expect(items.join(" ")).not.toContain(PAYLOAD.disclosure.slice(0, 40));
  });

  it("codes this version does not know: a short generic line in Turkish, never the English text", async () => {
    window.history.replaceState({}, "", "/?byt=future#/before-you-trade");
    const el = await mount();
    const text = ownText(el);
    expect(text).toContain(i18n.t("byt.text.unknown"));
    expect(text).toContain(i18n.t("byt.reason.unknown"));
    expect(el.querySelector('[data-testid="byt-bar"]')!.textContent).toContain(i18n.t("byt.text.unknown"));
    for (const s of payloadSentences(CODED)) expect(text, s).not.toContain(s);
    expect(text).not.toMatch(ENGLISH_WORDS);
  });

  it("an unknown English reason (no code) gets the generic line too", () => {
    const t = i18n.t.bind(i18n);
    expect(reasonText(t, { reason: "a sentence a later server writes" }, "tr")).toBe(i18n.t("byt.reason.unknown"));
    expect(reasonText(t, { reason: "x", reason_code: "brand_new_code" }, "tr")).toBe(i18n.t("byt.reason.unknown"));
    expect(sayText(t, undefined, "a later fixed sentence")).toBe(i18n.t("byt.text.unknown"));
    expect(reasonText(t, {}, "tr")).toBeNull();
  });
});

describe("bytText", () => {
  it("every code the app knows is translated in every locale", () => {
    for (const lng of ["en", "tr", "es", "hi", "id", "pt-BR", "ru", "vi"]) {
      for (const c of REASON_CODES) expect(i18n.exists(`byt.reason.${c}`, { lng }), `${lng} reason ${c}`).toBe(true);
      for (const c of TEXT_CODES) expect(i18n.exists(`byt.text.${c}`, { lng }), `${lng} text ${c}`).toBe(true);
    }
  });

  it("recovers the code and numbers of today's English sentences", () => {
    expect(legacyReasonCode("DataHub has no bybit candles")).toEqual({ code: "reaction_no_venue_candles", args: { exchange: "bybit" } });
    expect(legacyReasonCode("0.02 of 30 days since T0")).toEqual({ code: "farm_rank_days_below_min", args: { days: 0.02, min_days: 30 } });
    expect(legacyReasonCode("DataHub returned 9 closed daily candles; ATR(14) needs 15")).toEqual({ code: "datahub_too_few_candles", args: { got: 9, need: 15 } });
    expect(legacyReasonCode("never read successfully, so absent here: upbit, kraken")).toEqual({ code: "listings_channels_never_read", args: { channels: ["upbit", "kraken"] } });
    expect(legacyReasonCode("btcmacro service not wired")?.code).toBe("not_wired");
    expect(legacyReasonCode("nothing like this")).toBeNull();
  });

  it("the bar says the same from the legacy verdict and from the codes", () => {
    const t = i18n.t.bind(i18n);
    const legacyBar = PAYLOAD.market.chart_read.bar!;
    const codedBar = CODED.market.chart_read.bar!;
    expect(barText(t, legacyBar, "en")).toBe("Veto bar not met: 56.7% at n=60 (needed 60% and 5 pp over both baselines).");
    expect(barText(t, codedBar, "en")).toBe(barText(t, legacyBar, "en"));
    expect(barText(t, { status: "open", verdict: "Veto bar open: 12 of 60 graded 24h reads", min_n: 60 }, "en")).toBe("Veto bar open: 12 of 60 graded 24h reads.");
    expect(barText(t, { status: "not_met", verdict: "", min_n: 60 }, "en")).toBe("Veto bar not met at n=60.");
  });
});

describe("Before you trade layout", () => {
  it("one sub-block per full-width row inside each block, none side by side", async () => {
    const el = await mount();
    expect(el.querySelector(".ae-byt__grid")).toBeNull();
    const blocks = [...el.querySelectorAll(".ae-byt > .ae-byt__block")];
    expect(blocks).toHaveLength(4);
    const rows = [...blocks[0].querySelectorAll(":scope > .ae-byt__row")].map((r) => r.getAttribute("data-testid"));
    expect(rows).toEqual(["byt-regime", "byt-chart-read", "byt-vol", "byt-funding"]);
    for (const r of el.querySelectorAll(".ae-byt__row")) expect(r.parentElement!.classList.contains("ae-byt__block")).toBe(true);
    const models = [...blocks[1].querySelectorAll(":scope > .ae-byt__row")].map((r) => r.getAttribute("data-testid"));
    expect(models).toEqual([null, "byt-ghost", "byt-farm", "byt-similar"]);
  });
});

// ---------------------------------------------------------------- schema_minor 2

describe("farm signal bots and translated news lines (schema_minor 2)", () => {
  afterAll(async () => {
    await i18n.changeLanguage("en");
  });

  it("lists the farm's signal bots, paper and simulated, in the payload's order", async () => {
    await i18n.changeLanguage("en");
    const el = await mount();
    const block = el.querySelector('[data-testid="byt-farm-signal"]')!;
    const text = block.textContent ?? "";
    expect(text).toContain("Signal bots");
    expect(text).toContain("Paper, simulated fills");
    const rows = [...block.querySelectorAll("tbody tr")].map((r) => r.textContent ?? "");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toContain("Futures");
    expect(rows[0]).toContain("0 closed / 3 open");
    expect(rows[1]).toContain("Spot");
    expect(rows[1]).toContain("1 closed / 2 open");
    expect(text).toContain("Signal bots follow the published signals with the app's default settings.");
    expect(text).not.toMatch(/rank(ed)? #|best|top bot/i);
  });

  it("says the signal bots in Turkish from codes", async () => {
    await i18n.changeLanguage("tr");
    const el = await mount();
    const text = el.querySelector('[data-testid="byt-farm-signal"]')!.textContent ?? "";
    expect(text).toContain("Sinyal botları");
    expect(text).toContain("Simülasyonda işlem; sıralama yok");
    expect(text).toContain("1 kapandı / 2 açık");
    expect(text).toContain("Vadeli");
  });

  it("an older payload without signal bots shows no signal block", async () => {
    await i18n.changeLanguage("en");
    window.history.replaceState({}, "", "/?byt=legacy#/before-you-trade");
    const el = await mount();
    expect(el.querySelector('[data-testid="byt-farm"]')).not.toBeNull();
    expect(el.querySelector('[data-testid="byt-farm-signal"]')).toBeNull();
  });

  it("Turkish: the checked translation with a çeviri label and the original on hover; English where none passed", async () => {
    await i18n.changeLanguage("tr");
    const el = await mount();
    const tr = [...el.querySelectorAll('[data-testid="byt-news-translated"]')];
    expect(tr).toHaveLength(1);
    expect(tr[0].querySelector('[lang="tr"]')!.textContent).toBe(CODED.news.lines[0].translations!.tr);
    const label = tr[0].querySelector("summary")!;
    expect(label.textContent).toBe("çeviri");
    expect(label.getAttribute("title")).toBe(CODED.news.lines[0].text);
    expect(tr[0].querySelector('details [lang="en"]')!.textContent).toBe(CODED.news.lines[0].text);
    // Line 2 has no Turkish translation: English, marked, with the source-language note.
    const quotes = [...el.querySelectorAll('[lang="en"]')].map((n) => n.textContent);
    expect(quotes).toContain(CODED.news.lines[1].text);
    const text = el.textContent ?? "";
    expect(text).toContain(i18n.t("byt.news.sourceLanguage"));
    expect(text).toContain(i18n.t("byt.text.news_machine_translated_v1"));
  });

  it("Spanish: every line translated, so no source-language note", async () => {
    await i18n.changeLanguage("es");
    const el = await mount();
    expect(el.querySelectorAll('[data-testid="byt-news-translated"]')).toHaveLength(2);
    expect(el.querySelectorAll('summary.ae-byt__trlabel')[0].textContent).toBe("traducción");
    expect(el.textContent).not.toContain(i18n.t("byt.news.sourceLanguage"));
  });

  it("English readers see the English line, no translation label", async () => {
    await i18n.changeLanguage("en");
    const el = await mount();
    expect(el.querySelector('[data-testid="byt-news-translated"]')).toBeNull();
    expect(el.textContent).toContain("Digest lines are shown in their source language.");
    expect(el.textContent).not.toContain("Translated lines are machine translations");
  });

  it("newsTranslation uses only the server's translation for the exact locale", () => {
    const l = CODED.news.lines[0];
    expect(newsTranslation(l, "tr")).toBe(l.translations!.tr);
    expect(newsTranslation(l, "en")).toBeNull();
    expect(newsTranslation({ ...l, translations: { tr: "  " } }, "tr")).toBeNull();
    expect(newsTranslation({ ...l, text: "" }, "tr")).toBeNull();
    expect(newsTranslation(CODED.news.lines[1], "tr")).toBeNull();
    expect(newsTranslation(PAYLOAD.news.lines[0], "tr")).toBeNull();
  });

  it("a refusal count is said with the app's own skip reason, without its per-signal detail", async () => {
    await i18n.changeLanguage("en");
    const t = i18n.t.bind(i18n);
    expect(refusalText(t, "capitalCap", 3)).toBe("Above per-position cap: 3");
    expect(refusalText(t, "expired", 2)).toBe("Signal expired: 2");
    expect(refusalText(t, "notAKnownReason", 1)).toBe("Other reason: 1");
    expect(refusalText(t, "../x", 1)).toBe("Other reason: 1");
  });
});

describe("language of the request and a failed newest digest run", () => {
  afterAll(async () => {
    await i18n.changeLanguage("en");
  });

  it("asks with the UI language and asks again when it changes", async () => {
    await i18n.changeLanguage("tr");
    requestedLangs.length = 0;
    await mount();
    expect(requestedLangs).toEqual(["tr"]);
    await act(async () => {
      await i18n.changeLanguage("pt-BR");
    });
    await settle();
    expect(requestedLangs).toEqual(["tr", "pt-BR"]);
  });

  it("a one-locale payload renders like the all-locale one", async () => {
    await i18n.changeLanguage("tr");
    const el = await mount();
    expect(el.querySelectorAll('[data-testid="byt-news-translated"]')).toHaveLength(1);
  });

  it("says that the newest run failed and the previous digest is shown", async () => {
    await i18n.changeLanguage("en");
    window.history.replaceState({}, "", "/?byt=newerfailed#/before-you-trade");
    const el = await mount();
    const note = el.querySelector('[data-testid="byt-news-newer-run"]')!.textContent ?? "";
    expect(note).toMatch(/^The newest digest run \(.+\) produced no summary; the previous digest is shown\.$/);
    expect(el.textContent).toContain("2 of 2 lines shown.");
  });

  it("no newer failed run: no note", async () => {
    await i18n.changeLanguage("en");
    const el = await mount();
    expect(el.querySelector('[data-testid="byt-news-newer-run"]')).toBeNull();
  });
});
