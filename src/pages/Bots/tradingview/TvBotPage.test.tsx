// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "@/i18n";
import type { BotConfig, BotDeskStatus } from "@/lib/ipc/bot/bot";
import { budgetFloor, configFromForm, formError, formFromConfig, parseAllowList } from "./tvBotModel";
import { TvBotPage } from "./TvBotPage";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
window.matchMedia ??= ((query: string) =>
  ({ matches: false, media: query, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, onchange: null, dispatchEvent: () => false }) as MediaQueryList);

function status(over: Partial<BotDeskStatus> = {}): BotDeskStatus {
  return {
    liveTradingEnabled: true,
    binanceIsProduction: true,
    liveVenues: ["binance"],
    futures: null,
    spot: null,
    pump: null,
    tradingview: null,
    futuresRunning: false,
    spotRunning: false,
    pumpRunning: false,
    tradingviewRunning: false,
    openPositions: [],
    recentSkips: [],
    killSwitchTripped: false,
    btcRegime: "normal",
    ...over,
  };
}

const ctx = vi.hoisted(() => ({
  status: null as unknown as BotDeskStatus,
  configure: vi.fn(),
  configureAndStart: vi.fn(),
  credentials: [] as { exchangeId: string; permission: string }[],
}));
vi.mock("@/app/DeskProvider", () => ({
  useDeskContext: () => ({
    desk: {
      loaded: true,
      busy: false,
      status: ctx.status,
      configure: ctx.configure,
      configureAndStart: ctx.configureAndStart,
      start: vi.fn(),
      stop: vi.fn(),
      setLive: vi.fn(),
      endPilot: vi.fn(),
    },
    risk: { state: { limits: { maxLeverage: 5 }, maxCapitalQuote: 1000 } },
    catalog: { exchanges: [{ id: "binance", name: "Binance", supportsFutures: true }, { id: "okx", name: "OKX", supportsFutures: true }] },
    vault: { credentials: ctx.credentials },
  }),
}));

let root: Root | null = null;
let host: HTMLDivElement | null = null;

async function mount() {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root!.render(<TvBotPage />));
  // The server's defaults arrive (bot_default_config through the dev mock).
  await act(async () => new Promise((r) => setTimeout(r, 0)));
  return host;
}

const button = (el: ParentNode, text: string) => [...el.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === text);
const typeInto = async (input: HTMLInputElement, value: string) =>
  act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
const field = (el: ParentNode, label: string) => {
  const l = [...el.querySelectorAll("label")].find((x) => x.textContent === label)!;
  return el.querySelector<HTMLInputElement>(`#${CSS.escape(l.getAttribute("for")!)}`)!;
};

beforeAll(async () => {
  await i18n.changeLanguage("en");
});
beforeEach(() => {
  ctx.status = status();
  ctx.configure.mockReset().mockResolvedValue(null);
  ctx.configureAndStart.mockReset().mockResolvedValue(undefined);
  ctx.credentials.length = 0;
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

describe("TradingView bot creation", () => {
  it("starts Paper, offers Live only with a verified key, and lists what is checked", async () => {
    const el = await mount();
    const live = [...el.querySelectorAll<HTMLElement>('[role="radio"]')].find((r) => r.textContent === "Live")!;
    expect(live.getAttribute("aria-checked")).toBe("false");
    expect(live.hasAttribute("disabled") || live.getAttribute("aria-disabled") === "true").toBe(true);
    expect(el.textContent).toContain("A verified trade-only Binance key in the unlocked vault");
    expect(el.textContent).toContain("Checked on every alert");
    expect(el.textContent).toContain("Margin per position: 33.33 USDT");
    expect(el.textContent).toContain("Not saved yet");
  });

  it("saves the limits and the passphrase as the TradingView bot", async () => {
    const el = await mount();
    await typeInto(field(el, "Name"), "Trend A");
    await typeInto(field(el, "Max size per alert"), "40");
    await typeInto(field(el, "Allow list"), "btcusdt, ETHUSDT");
    await typeInto(field(el, "Stale window"), "20");
    await act(async () => [...el.querySelectorAll<HTMLElement>('[role="radio"]')].find((r) => r.textContent === "Set new")!.click());
    await typeInto(field(el, "New passphrase"), "pw1");
    await act(async () => button(el, "Save")!.click());
    expect(ctx.configure).toHaveBeenCalledTimes(1);
    const sent = ctx.configure.mock.calls[0][0] as BotConfig;
    expect(sent.kind).toBe("tradingview");
    expect(sent.symbols).toEqual(["BTCUSDT", "ETHUSDT"]);
    expect(sent.sizing).toBe("fixed");
    expect(sent.capital).toBe(33.33);
    expect(sent.tv).toMatchObject({ name: "Trend A", maxSizeUsdt: 40, staleS: 20, priceGuardPct: 0.5, requirePrice: true, passphrase: "pw1" });
  });

  it("shows the budget one max-size alert needs, and says it on every budget error", async () => {
    const el = await mount();
    await typeInto(field(el, "Max size per alert"), "50");
    await typeInto(field(el, "Max leverage"), "2");
    expect(el.textContent).toContain("Minimum 25 USDT: one alert at max size (50 USDT, 2x). All 3 positions at max size: 75 USDT.");
    expect(field(el, "Budget").placeholder).toBe("25");
    expect(el.textContent).toContain("Minimum 5 USDT (Binance minimum).");
    await typeInto(field(el, "Budget"), "");
    expect(el.textContent).toContain("Enter a budget (minimum 25 USDT)");
    await typeInto(field(el, "Budget"), "abc");
    expect(el.textContent).toContain("Not a valid number for this field. Minimum 25 USDT.");
    await typeInto(field(el, "Budget"), "20");
    expect(el.textContent).toContain("Minimum 25 USDT needed: one alert at max size (50 USDT, 2x) holds that much margin");
    await typeInto(field(el, "Max size per alert"), "");
    expect(el.textContent).toContain("Enter a size (minimum 5 USDT)");
  });

  it("refuses out-of-range limits before Rust does", async () => {
    const el = await mount();
    await typeInto(field(el, "Stale window"), "200");
    expect(el.textContent).toContain("Stale window: 5 to 120 s");
    expect(button(el, "Save")!.disabled).toBe(true);
  });
});

describe("tvBotModel", () => {
  const base: BotConfig = { kind: "tradingview", exchangeId: "binance", maxPositions: 3, capital: 33.33, leverage: 3 };

  it("reads the allow list once per pair, upper case", () => {
    expect(parseAllowList(" btcusdt, ETHUSDT ethusdt;SOLUSDT ")).toEqual(["BTCUSDT", "ETHUSDT", "SOLUSDT"]);
    expect(parseAllowList("")).toEqual([]);
  });

  it("names the first field that stops Save", () => {
    const f = formFromConfig(null, "binance");
    expect(formError(f, 5)).toBeNull();
    expect(formError({ ...f, leverage: 6 }, 5)).toEqual({ key: "leverage", params: { max: 5 } });
    expect(formError({ ...f, maxSizeUsdt: 4 }, 5)?.key).toBe("size");
    expect(formError({ ...f, staleS: 4 }, 5)?.key).toBe("stale");
    expect(formError({ ...f, priceGuardPct: 0 }, 5)?.key).toBe("guard");
    expect(formError({ ...f, allowList: "BTCUSD" }, 5)?.key).toBe("allowList");
    expect(formError({ ...f, passphraseAction: "set", passphrase: " " }, 5)?.key).toBe("passphrase");
  });

  it("sizes the minimum budget like the alert guard: max size / leverage, the risk level capping it", () => {
    const f = { ...formFromConfig(null, "binance"), maxSizeUsdt: 50, leverage: 3, maxPositions: 3 };
    expect(budgetFloor(f, 5)).toEqual({ min: 16.67, full: 50, leverage: 3 });
    expect(budgetFloor(f, 2)).toEqual({ min: 25, full: 75, leverage: 2 });
    expect(budgetFloor({ ...f, maxSizeUsdt: 4 }, 5)).toBeNull();
    expect(budgetFloor({ ...f, leverage: 0 }, 5)).toBeNull();
  });

  it("keeps, sets or clears the passphrase and never sends the stored hash back as text", () => {
    const stored: BotConfig = { ...base, tv: { ...formToTv(), passphraseSha256: "ab", passphraseSalt: "cd" } };
    const f = formFromConfig(stored, "binance");
    expect(configFromForm(f, stored).tv?.passphrase).toBeUndefined();
    expect(configFromForm({ ...f, passphraseAction: "clear" }, stored).tv?.passphrase).toBe("");
    expect(configFromForm({ ...f, passphraseAction: "set", passphrase: " x " }, stored).tv?.passphrase).toBe("x");
    expect(configFromForm(f, stored).tv).not.toHaveProperty("passphraseSha256");
  });
});

function formToTv() {
  return { name: "TV", budgetUsdt: 100, maxSizeUsdt: 50, maxOpensPerDay: 10, staleS: 30, priceGuardPct: 0.5, requirePrice: true };
}
