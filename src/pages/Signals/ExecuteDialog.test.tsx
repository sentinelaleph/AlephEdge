// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "@/i18n";
import type { Signal } from "@/lib/ipc/signal/signal";
import type { Preset } from "@/lib/ipc/strategy/strategy";
import { presetStartConfig, querySymbol } from "@/pages/Bots/StrategyCreatePage";
import { blankFormPath, executeTemplates, signalBotsFor } from "./ExecuteDialog";
import { SignalsPage } from "./SignalsPage";
import { takeBotsFor, takeRefusalText } from "./TakeNow";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
window.matchMedia ??= ((query: string) =>
  ({ matches: false, media: query, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, onchange: null, dispatchEvent: () => false }) as MediaQueryList);

/** A template with only the fields the Execute dialog and the create page read. */
function preset(id: string, kind: "dca" | "grid", side: "long" | "short" | "neutral", opts: { failed?: boolean; btcOnly?: boolean; mean?: number } = {}): Preset {
  return {
    id,
    verdict: opts.failed ? "failed" : "presetReady",
    failReasons: opts.failed ? ["negativeSplit"] : [],
    situation: "bullMajors",
    capitalModel: "botPerSymbolMonth",
    config: { name: id, symbol: "ETHUSDT", side, market: "futures", leverage: 1, presetId: id, params: { kind } },
    universe: { rule: opts.btcOnly ? "btcOnly" : "topUsdtPerpsByQuoteVolume", rankFrom: 1, topN: 5, volumeWindowDays: 90, reselect: "monthly", exclude: [] },
    history: {
      splits: [{ split: "test", meanPerBotMonthPct: opts.mean ?? 1.9, ci95LowPct: 1, ci95HighPct: 2, monthsPositive: 8, months: 8, oneBotReturnPerDayPct: null }],
      recheck: null,
    },
  } as unknown as Preset;
}

const PRESETS: Preset[] = [
  preset("dca_long_classic", "dca", "long", { mean: 1.98 }),
  preset("dca_long_btc", "dca", "long", { btcOnly: true }),
  preset("dca_short_classic", "dca", "short", { failed: true, mean: -8.61 }),
  preset("dca_long_bad", "dca", "long", { failed: true, mean: -1.2 }),
  preset("grid_long", "grid", "long"),
  preset("grid_neutral", "grid", "neutral"),
  preset("grid_short", "grid", "short"),
];

const presetsMock = vi.hoisted(() => vi.fn());
vi.mock("@/lib/ipc/strategy/strategy", async (orig) => ({ ...(await orig<object>()), strategyPresets: presetsMock }));

function signal(symbol: string, direction: "long" | "short", market_type?: string): Signal {
  const now = Date.now();
  return {
    id: `${symbol}-${direction}`,
    symbol,
    timeframe: "1h",
    market_type,
    direction,
    mode: "hybrid",
    entry: 100,
    tp: [101, 102, 103],
    sl: 98,
    confidence: 0.7,
    confluence: [],
    rr: 1.5,
    expires_at: new Date(now + 3_600_000).toISOString(),
    created_at: new Date(now - 60_000).toISOString(),
  };
}

const take = vi.hoisted(() => ({ preview: vi.fn(), open: vi.fn() }));
vi.mock("@/lib/ipc/bot/bot", async (orig) => ({ ...(await orig<object>()), botPreviewTake: take.preview, botTakeSignal: take.open }));

/** What bot_preview_take answers for a 100 USDT, 1x futures bot. */
function preview(kind: string) {
  return {
    botKind: kind,
    signalId: "x",
    symbol: "ETHUSDT",
    direction: "long",
    entry: 100,
    publishedEntry: 99.5,
    stop: 98,
    target: 101,
    tpTarget: "tp1",
    rrAtFill: 0.5,
    rrPublished: 1.3,
    rrLow: false,
    notionalUsdt: 100,
    capital: 100,
    effectiveLeverage: 1,
    riskCapped: false,
    lossAtStopUsdt: -2.1,
    gainAtTargetUsdt: 0.9,
    feesUsdt: 0.1,
  };
}

const feed = vi.hoisted(() => ({ signals: [] as Signal[] }));
vi.mock("@/app/DeskProvider", () => ({
  useDeskContext: () => ({
    feed: { signals: feed.signals, loaded: true, health: { connected: true, phase: "live" } },
    desk: { status: { openPositions: [] } },
    pnl: { trades: [] },
  }),
}));
vi.mock("@/components/Desk/Readouts", () => ({ StreamCard: () => null }));

let root: Root | null = null;
let host: HTMLDivElement | null = null;

async function mount(node: React.ReactNode) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root!.render(node));
  return host;
}

beforeAll(async () => {
  await i18n.changeLanguage("en");
});
beforeEach(() => {
  presetsMock.mockResolvedValue(PRESETS);
  take.preview.mockReset();
  take.open.mockReset();
  take.preview.mockImplementation((kind: string) => Promise.resolve(preview(kind)));
  window.location.hash = "#/signals";
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const dialogEl = () => document.querySelector<HTMLElement>(".ae-exec");
const buttons = (el: ParentNode, text: string) => [...el.querySelectorAll("button")].filter((b) => b.textContent === text);
const rowOf = (id: string) => dialogEl()!.querySelector<HTMLElement>(`[data-preset="${id}"]`);
const click = async (el: HTMLElement) => act(async () => el.click());

/** Mounts the Signals page with one signal and presses its row's Execute. */
async function openFor(s: Signal) {
  feed.signals = [s];
  const el = await mount(<SignalsPage />);
  const exec = el.querySelector<HTMLButtonElement>(`button[aria-label="Execute ${s.symbol}"]`);
  expect(exec).not.toBeNull();
  expect(dialogEl()).toBeNull();
  await click(exec!);
  return dialogEl()!;
}

describe("Execute on the Signals page", () => {
  it("opens a dialog titled with the symbol and side", async () => {
    const d = await openFor(signal("ETHUSDT", "long"));
    expect(d).not.toBeNull();
    expect(d.getAttribute("role")).toBe("dialog");
    expect(d.querySelector("h2")?.textContent).toBe("ETHUSDT · Long");
  });

  it("lists long DCA templates and long + neutral grids for a long signal, with verdict chips and test means", async () => {
    const d = await openFor(signal("ETHUSDT", "long"));
    const ids = [...d.querySelectorAll<HTMLElement>("[data-preset]")].map((r) => r.dataset.preset);
    expect(ids).toEqual(["dca_long_classic", "dca_long_bad", "grid_long", "grid_neutral"]);
    expect(rowOf("dca_long_classic")!.textContent).toContain("Passed checks");
    expect(rowOf("dca_long_classic")!.textContent).toContain("+1.98%");
    expect(rowOf("dca_long_bad")!.textContent).toContain("Did not pass");
    expect(buttons(d, "Open with ETHUSDT")).toHaveLength(4);
  });

  it("hides a BTC-only template for another pair and shows it for BTCUSDT", async () => {
    await openFor(signal("ETHUSDT", "long"));
    expect(rowOf("dca_long_btc")).toBeNull();
    await act(async () => root?.unmount());
    host?.remove();
    await openFor(signal("BTCUSDT", "long"));
    expect(rowOf("dca_long_btc")).not.toBeNull();
  });

  it("opens a passed template on the signal's pair", async () => {
    await openFor(signal("ETHUSDT", "long"));
    await click(rowOf("dca_long_classic")!.querySelector("button")!);
    expect(window.location.hash).toBe("#/bots/dca/new?preset=dca_long_classic&symbol=ETHUSDT");
  });

  it("holds a failed template behind a second confirmation", async () => {
    const d = await openFor(signal("ETHUSDT", "short"));
    expect([...d.querySelectorAll<HTMLElement>("[data-preset]")].map((r) => r.dataset.preset)).toEqual(["dca_short_classic", "grid_short"]);
    await click(rowOf("dca_short_classic")!.querySelector("button")!);
    expect(window.location.hash).toBe("#/signals");
    const confirm = document.querySelector<HTMLElement>("[role=alertdialog]");
    expect(confirm?.textContent).toContain("did not pass its checks");
    await click(buttons(confirm!, "Use anyway")[0]);
    expect(window.location.hash).toBe("#/bots/dca/new?preset=dca_short_classic&symbol=ETHUSDT");
  });

  it("offers the blank forms on the pair", async () => {
    const d = await openFor(signal("SOLUSDT", "long"));
    await click(buttons(d, "Blank Grid · SOLUSDT")[0]);
    expect(window.location.hash).toBe("#/bots/grid/new?symbol=SOLUSDT");
    expect(dialogEl()).toBeNull();
  });

  it("opens from the selected signal's panel too", async () => {
    const s = signal("ETHUSDT", "long");
    feed.signals = [s];
    window.location.hash = `#/signals?id=${s.id}`;
    const el = await mount(<SignalsPage />);
    const inPanel = el.querySelector<HTMLButtonElement>("button.ae-sigexec");
    expect(inPanel?.textContent).toBe("Execute");
    await click(inPanel!);
    expect(dialogEl()?.querySelector("h2")?.textContent).toBe("ETHUSDT · Long");
  });

  it("puts Take this signal now first, with the paper preview per fitting bot", async () => {
    const d = await openFor(signal("ETHUSDT", "long", "futures"));
    const first = d.querySelector<HTMLElement>("section.ae-exec__section");
    expect(first?.getAttribute("aria-label")).toBe("Take this signal now · paper");
    const rows = [...d.querySelectorAll<HTMLElement>("[data-bot]")];
    expect(rows.map((r) => r.dataset.bot)).toEqual(["futures"]);
    expect(take.preview).toHaveBeenCalledWith("futures", "ETHUSDT-long");
    const text = rows[0].textContent ?? "";
    expect(text).toContain("Loss at stop 98");
    expect(text).toContain("-2.10");
    expect(text).toContain("Gain at TP1 101");
    expect(text).toContain("+0.90");
    expect(text).toContain("Fees");
    expect(text).toContain("Fill now100");
    expect(text).toContain("Signal entry99.50");
    expect(text).toContain("0.50 at fill · 1.30 published");
    expect(text).not.toContain("most of the reward is gone");
  });

  it("warns when the reward:risk at the fill is below 0.5", async () => {
    take.preview.mockImplementation((kind: string) => Promise.resolve({ ...preview(kind), rrAtFill: 0.23, rrLow: true }));
    const d = await openFor(signal("ETHUSDT", "long", "futures"));
    const row = d.querySelector<HTMLElement>('[data-bot="futures"]')!;
    expect(row.querySelector("[role=note]")?.textContent).toBe("The trade has moved · most of the reward is gone");
    expect(buttons(d, "Open position")).toHaveLength(1);
  });

  it("opens the position and links to Positions & orders and the bot page", async () => {
    take.open.mockResolvedValue({ entry: 100.2 });
    const d = await openFor(signal("ETHUSDT", "long", "spot"));
    await click(buttons(d, "Open position")[0]);
    expect(take.open).toHaveBeenCalledWith("spot", "ETHUSDT-long");
    const row = d.querySelector<HTMLElement>('[data-bot="spot"]')!;
    expect(row.textContent).toContain("Opened · ETHUSDT at 100.20 · paper");
    const links = [...row.querySelectorAll("a")].map((a) => a.getAttribute("href"));
    expect(links).toEqual(["#/positions", "#/bots/signal?bot=spot"]);
  });

  it("shows a refusal with its reason and no button", async () => {
    take.preview.mockRejectedValue("manualTakePaperOnly");
    const d = await openFor(signal("ETHUSDT", "short", "futures"));
    const row = d.querySelector<HTMLElement>('[data-bot="futures"]')!;
    expect(row.textContent).toContain("This bot trades real money · Execute opens paper positions only");
    expect(buttons(d, "Open position")).toHaveLength(0);
  });

  it("closes on Escape", async () => {
    await openFor(signal("ETHUSDT", "long"));
    await act(async () => document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })));
    expect(dialogEl()).toBeNull();
  });
});

describe("executeTemplates / signalBotsFor", () => {
  it("short signal: short templates only", () => {
    const r = executeTemplates(PRESETS, { symbol: "BTCUSDT", direction: "short" });
    expect(r.dca.map((p) => p.id)).toEqual(["dca_short_classic"]);
    expect(r.grid.map((p) => p.id)).toEqual(["grid_short"]);
  });
  it("names the signal bot that takes the market and side", () => {
    expect(signalBotsFor({ direction: "long", market_type: "futures" })).toEqual(["futures"]);
    expect(signalBotsFor({ direction: "long", market_type: "spot" })).toEqual(["spot"]);
    expect(signalBotsFor({ direction: "short", market_type: "spot" })).toEqual([]);
    expect(signalBotsFor({ direction: "long" })).toEqual(["futures", "spot"]);
    expect(signalBotsFor({ direction: "short" })).toEqual(["futures"]);
  });
  it("adds Pump only when it is configured and the signal is a futures one", () => {
    expect(takeBotsFor({ direction: "long", market_type: "futures" }, false)).toEqual(["futures"]);
    expect(takeBotsFor({ direction: "short", market_type: "futures" }, true)).toEqual(["futures", "pump"]);
    expect(takeBotsFor({ direction: "long", market_type: "spot" }, true)).toEqual(["spot"]);
    expect(takeBotsFor({ direction: "long" }, true)).toEqual(["futures", "spot", "pump"]);
  });
  it("words a refusal: own text, then the skip reason, then the error table", () => {
    const has = (k: string) => i18n.exists(k);
    expect(takeRefusalText(i18n.t, has, "marketHeld")).toBe("This bot already holds a position on this symbol");
    expect(takeRefusalText(i18n.t, has, "targetPassedBeforeEntry|TP1")).toBe("Price already passed the target (TP1) before entry");
    expect(takeRefusalText(i18n.t, has, "stopPassed")).toBe("Price already passed the stop");
    expect(takeRefusalText(i18n.t, has, "botMaxPositions|3/3")).toBe("Bot position limit reached (3/3)");
    expect(takeRefusalText(i18n.t, has, "botNotConfigured")).toBe("This bot has no saved settings yet.");
    expect(takeRefusalText(i18n.t, has, "somethingNew|x")).toBe("somethingNew|x");
  });
  it("blank form path carries the symbol", () => {
    expect(blankFormPath("dca", "ETHUSDT")).toBe("/bots/dca/new?symbol=ETHUSDT");
  });
});

describe("StrategyCreatePage ?symbol", () => {
  it("normalizes the query symbol", () => {
    expect(querySymbol(" ethusdt")).toBe("ETHUSDT");
    expect(querySymbol("")).toBeNull();
    expect(querySymbol(null)).toBeNull();
  });
  it("opens a template on the given pair, keeping its link", () => {
    const cfg = presetStartConfig(PRESETS[0], "dca", "SOLUSDT");
    expect(cfg.symbol).toBe("SOLUSDT");
    expect(cfg.presetId).toBe("dca_long_classic");
    expect(presetStartConfig(PRESETS[0], "dca", null).symbol).toBe("ETHUSDT");
  });
  it("keeps a BTC-only template on BTCUSDT and refuses another pair", () => {
    expect(presetStartConfig(PRESETS[1], "dca", null).symbol).toBe("BTCUSDT");
    expect(presetStartConfig(PRESETS[1], "dca", "BTCUSDT").symbol).toBe("BTCUSDT");
    expect(() => presetStartConfig(PRESETS[1], "dca", "ETHUSDT")).toThrow();
    try {
      presetStartConfig(PRESETS[1], "dca", "ETHUSDT");
    } catch (e) {
      expect(e).toEqual({ key: "strategy.form.presetPairOnly", params: { id: "dca_long_btc", symbol: "ETHUSDT" } });
    }
  });
  it("refuses a template of the other bot type", () => {
    expect(() => presetStartConfig(PRESETS[4], "dca", null)).toThrow();
  });
});
