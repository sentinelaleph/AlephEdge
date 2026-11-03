// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "@/i18n";
import type { BotConfig, BotDeskStatus } from "@/lib/ipc/bot/bot";
import type { PineCompileInfo } from "@/lib/ipc/pine/pine";
import { defaultSettings } from "@/lib/ipc/pine/pine.mock";
import { PineBotPage } from "./PineBotPage";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
window.matchMedia ??= ((query: string) =>
  ({ matches: false, media: query, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, onchange: null, dispatchEvent: () => false }) as MediaQueryList);

const HASH = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";
const OK: PineCompileInfo = {
  ok: true,
  error: null,
  title: "EMA cross",
  overlay: true,
  inputs: [{ id: 0, kind: "int", title: "Fast", defval: 9, minval: 1, maxval: 50, line: 2, col: 1 }, { id: 1, kind: "float", title: "Stop %", defval: 2, line: 3, col: 1 }],
  triggers: [
    { id: "alert#0", kind: "alert", title: "Long entry", line: 6 },
    { id: "shape#0", kind: "shape", title: "", line: 7 },
  ],
  hash: HASH,
};

const ctx = vi.hoisted(() => ({
  status: null as unknown as BotDeskStatus,
  configure: vi.fn(),
  configureAndStart: vi.fn(),
  setLive: vi.fn(),
  start: vi.fn(),
  compile: vi.fn(),
  terms: vi.fn(),
  minimum: vi.fn(),
}));
vi.mock("@/app/DeskProvider", () => ({
  useDeskContext: () => ({
    desk: {
      loaded: true,
      busy: false,
      status: ctx.status,
      configure: ctx.configure,
      configureAndStart: ctx.configureAndStart,
      start: ctx.start,
      stop: vi.fn(),
      setLive: ctx.setLive,
      endPilot: vi.fn(),
      closeLiveAndPaper: vi.fn(),
      closePosition: vi.fn(),
    },
    risk: { state: { limits: { maxLeverage: 5 }, maxCapitalQuote: 1000 } },
    catalog: { exchanges: [{ id: "binance", name: "Binance", supportsFutures: true }] },
    vault: { credentials: [{ exchangeId: "binance", permission: "tradeOnly" }] },
  }),
}));
vi.mock("@/lib/ipc/pine/pine", async (orig) => ({
  ...(await orig<typeof import("@/lib/ipc/pine/pine")>()),
  pineCompile: ctx.compile,
  pineStatus: () => Promise.resolve(null),
  pineResults: () => Promise.resolve([]),
  onPineResult: () => Promise.resolve(() => undefined),
}));
vi.mock("@/lib/ipc/tvlink/tvlink", async (orig) => ({ ...(await orig<typeof import("@/lib/ipc/tvlink/tvlink")>()), tvSymbolMinimum: ctx.minimum }));
vi.mock("@/lib/ipc/bot/bot", async (orig) => ({ ...(await orig<typeof import("@/lib/ipc/bot/bot")>()), botLiveTerms: ctx.terms }));

function status(over: Partial<BotDeskStatus> = {}): BotDeskStatus {
  return {
    liveTradingEnabled: true,
    binanceIsProduction: true,
    liveVenues: ["binance"],
    futures: null,
    spot: null,
    pump: null,
    tradingview: null,
    pine: null,
    futuresRunning: false,
    spotRunning: false,
    pumpRunning: false,
    tradingviewRunning: false,
    pineRunning: false,
    openPositions: [],
    recentSkips: [],
    killSwitchTripped: false,
    btcRegime: "normal",
    ...over,
  };
}

function saved(over: Partial<BotConfig> = {}, pine: Partial<ReturnType<typeof defaultSettings>> = {}): BotConfig {
  return {
    kind: "pine",
    exchangeId: "binance",
    maxPositions: 3,
    capital: 33.33,
    leverage: 3,
    symbols: ["BTCUSDT"],
    sizing: "fixed",
    pine: { ...defaultSettings(), script: "indicator('x')", scriptHash: HASH, symbols: ["BTCUSDT"], triggers: { "alert#0": "buy", "shape#0": "close" }, ...pine },
    ...over,
  };
}

let root: Root | null = null;
let host: HTMLDivElement | null = null;
const settle = (ms = 0) => act(async () => new Promise((r) => setTimeout(r, ms)));

async function mount() {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root!.render(<PineBotPage />));
  await settle();
  return host;
}
/** Past the compile debounce, then the compile reply. */
const compiled = () => settle(450);

const button = (el: ParentNode, text: string) => [...el.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === text);
const typeInto = async (input: HTMLInputElement | HTMLTextAreaElement, value: string) =>
  act(async () => {
    const proto = input instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
const field = <T extends Element = HTMLInputElement>(el: ParentNode, label: string) => {
  const l = [...el.querySelectorAll("label")].find((x) => x.textContent === label)!;
  return el.querySelector<T & HTMLElement>(`#${CSS.escape(l.getAttribute("for")!)}`)!;
};
const choose = async (sel: HTMLSelectElement, value: string) =>
  act(async () => {
    sel.value = value;
    sel.dispatchEvent(new Event("change", { bubbles: true }));
  });

beforeAll(async () => {
  await i18n.changeLanguage("en");
});
beforeEach(() => {
  ctx.status = status();
  ctx.configure.mockReset().mockResolvedValue(null);
  ctx.configureAndStart.mockReset().mockResolvedValue(undefined);
  ctx.setLive.mockReset().mockResolvedValue(null);
  ctx.start.mockReset().mockResolvedValue(null);
  ctx.compile.mockReset().mockResolvedValue(OK);
  ctx.minimum.mockReset().mockImplementation((symbol: string) =>
    Promise.resolve({ symbol, status: "ok", price: 77_900, minQty: 0.001, minNotional: 50, minUsdt: symbol === "BTCUSDT" ? 54.41 : 5.5 }),
  );
  ctx.terms.mockReset().mockResolvedValue({ venue: "binance", marginAtRiskUsdt: 100, maxNotionalUsdt: 50, leverage: 3, positions: 3, pilotCount: 3, pilotNotionalUsdt: 50 });
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

describe("Pine bot page", () => {
  it("shows a compile that passes: title, counts, the short hash, inputs and triggers", async () => {
    const el = await mount();
    await typeInto(field<HTMLTextAreaElement>(el, "Pine-lite script"), "indicator('EMA cross')");
    await compiled();
    expect(ctx.compile).toHaveBeenCalledWith("indicator('EMA cross')");
    expect(el.textContent).toContain("Compiles");
    expect(el.textContent).toContain("EMA cross");
    expect(el.textContent).toContain("2 inputs · 2 triggers");
    expect(el.textContent).toContain(HASH.slice(0, 12));
    expect(field(el, "Fast").value).toBe("9");
    expect(el.textContent).toContain("Long entry");
    expect(el.textContent).toContain("plotshape #1");
  });

  it("names a compile error with its kind, line and column, the engine's message as data", async () => {
    ctx.compile.mockResolvedValue({ ...OK, ok: false, error: { kind: "syntax", message: "Unexpected token ')'", line: 4, col: 12 }, inputs: [], triggers: [] });
    const el = await mount();
    await typeInto(field<HTMLTextAreaElement>(el, "Pine-lite script"), "bad(");
    await compiled();
    expect(el.textContent).toContain("Syntax error at line 4:12:");
    expect(el.textContent).toContain("Unexpected token ')'");
    expect(el.textContent).toContain("The script does not compile: fix the error above");
    expect(button(el, "Save")!.disabled).toBe(true);
  });

  it("saves the trigger map (Ignore left out), the inputs that differ and the market", async () => {
    const el = await mount();
    await typeInto(field<HTMLTextAreaElement>(el, "Pine-lite script"), "indicator('EMA cross')");
    await compiled();
    await choose(field<HTMLSelectElement>(el, "Long entry"), "buy");
    await choose(field<HTMLSelectElement>(el, "plotshape #1"), "close");
    await typeInto(field(el, "Fast"), "12");
    await typeInto(field(el, "Pairs"), "btcusdt, ETHUSDT");
    await act(async () => button(el, "Save")!.click());
    expect(ctx.configure).toHaveBeenCalledTimes(1);
    const sent = ctx.configure.mock.calls[0][0] as BotConfig;
    expect(sent.kind).toBe("pine");
    expect(sent.symbols).toEqual(["BTCUSDT", "ETHUSDT"]);
    expect(sent.pine).toMatchObject({ triggers: { "alert#0": "buy", "shape#0": "close" }, inputs: { "0": 12 }, scriptHash: HASH, stopPct: 2, takeProfitPct: 4 });
    await choose(field<HTMLSelectElement>(el, "Long entry"), "ignore");
    await act(async () => button(el, "Save")!.click());
    expect((ctx.configure.mock.calls[1][0] as BotConfig).pine?.triggers).toEqual({ "shape#0": "close" });
  });

  it("refuses an input outside its declaration before Save", async () => {
    const el = await mount();
    await typeInto(field<HTMLTextAreaElement>(el, "Pine-lite script"), "indicator('EMA cross')");
    await compiled();
    await typeInto(field(el, "Fast"), "80");
    expect(el.textContent).toContain("At most 50");
    expect(button(el, "Save")!.disabled).toBe(true);
  });

  it("says above the settings why Save and Start are off (owner, 1 Nov 2026: no pair entered)", async () => {
    ctx.status = status({ pine: saved({}, { symbols: [] }) });
    const el = await mount();
    await compiled();
    expect(button(el, "Save")!.disabled).toBe(true);
    expect(button(el, "Start")!.disabled).toBe(true);
    expect(el.querySelector('[data-testid="pine-blocked"]')!.textContent).toBe("Save and Start stay off until this is fixed: Enter at least one pair");
  });

  it("keeps Save and Start off while every trigger is on Ignore (owner, 2 Nov 2026: bot ran, placed nothing)", async () => {
    ctx.status = status({ pine: saved({}, { triggers: {} }) });
    const el = await mount();
    await compiled();
    expect(button(el, "Save")!.disabled).toBe(true);
    expect(button(el, "Start")!.disabled).toBe(true);
    expect(el.querySelector('[data-testid="pine-blocked"]')!.textContent).toContain("Map at least one trigger to Buy, Sell or Close");
    // Said in the Triggers panel too, where the fix is.
    expect(el.querySelector('[data-testid="pine-map-error"]')!.textContent).toContain("Map at least one trigger to Buy, Sell or Close");
    await choose(field<HTMLSelectElement>(el, "plotshape #1"), "close");
    expect(button(el, "Save")!.disabled).toBe(false);
    expect(el.querySelector('[data-testid="pine-blocked"]')).toBeNull();
  });

  it("the Now panel says when the form has changes the running bot does not have yet (owner, 2 Nov 2026)", async () => {
    ctx.status = status({ pine: saved(), pineRunning: true });
    const el = await mount();
    await compiled();
    expect(el.querySelector('[data-testid="pine-unsaved"]')).toBeNull();
    const tf = [...el.querySelectorAll<HTMLElement>('[role="radio"]')].find((b) => b.textContent === "15m")!;
    await act(async () => tf.click());
    expect(el.querySelector('[data-testid="pine-unsaved"]')!.textContent).toBe("Unsaved changes: the bot runs the saved settings until you press Save.");
  });

  it("reads Binance's minimum for the bot's pairs and says when the size is below it (owner, 2 Nov 2026)", async () => {
    ctx.status = status({ pine: saved({}, { symbols: ["BTCUSDT", "ETHUSDT"], maxSizeUsdt: 50 }) });
    const el = await mount();
    await compiled();
    await settle(700);
    expect(ctx.minimum.mock.calls.map((c) => c[0]).sort()).toEqual(["BTCUSDT", "ETHUSDT"]);
    expect(el.textContent).toContain("Below Binance's minimum for BTCUSDT: a Buy or Sell there is skipped until the size is at least 54.41 USDT.");
    await typeInto(field(el, "Size per entry"), "100");
    expect(el.textContent).not.toContain("Below Binance's minimum");
    // In range: the field's hint lists the minimums it read.
    expect(el.textContent).toContain("BTCUSDT 54.41 · ETHUSDT 5.5");
  });

  it("needs the bot's own stop for Live, and says so next to the field", async () => {
    ctx.status = status({ pine: saved({ mode: "live", live: false }, { stopPct: null, stopInput: "Stop %" }) });
    const el = await mount();
    await compiled();
    expect(el.textContent).toContain("Live needs the bot's own stop (above 0 %)");
    expect(button(el, "Start with real money")!.disabled).toBe(true);
    await typeInto(field(el, "Stop"), "1.5");
    expect(button(el, "Start with real money")!.disabled).toBe(false);
  });

  it("the Live arm shows the script hash, which triggers map to which orders, and the money at risk", async () => {
    ctx.status = status({ pine: saved({ mode: "live", live: false }) });
    const el = await mount();
    await compiled();
    expect(el.textContent).toContain("Live, not armed this session");
    await act(async () => button(el, "Start with real money")!.click());
    await settle();
    const dialog = document.querySelector('[aria-modal="true"]')!;
    expect(dialog).not.toBeNull();
    expect(dialog.textContent).toContain(HASH.slice(0, 12));
    expect(dialog.textContent).toContain("Long entry → Buy · plotshape #1 → Close");
    expect(dialog.textContent).toContain("BTCUSDT · 1h");
    expect(dialog.textContent).toContain("100");
  });

  it("warns that saving script edits on an armed bot disarms it", async () => {
    ctx.status = status({ pine: saved({ mode: "live", live: true }) });
    const el = await mount();
    await compiled();
    expect(el.textContent).not.toContain("saving stops and disarms this bot");
    await choose(field<HTMLSelectElement>(el, "Long entry"), "sell");
    expect(el.textContent).toContain("saving stops and disarms this bot");
  });
});
