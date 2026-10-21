// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import i18n from "@/i18n";
import type { OpenPosition } from "@/lib/ipc/bot/bot";
import { PositionsTable } from "./PositionsTable";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
window.matchMedia ??= ((query: string) =>
  ({ matches: false, media: query, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, onchange: null, dispatchEvent: () => false }) as MediaQueryList);

vi.mock("@/app/DeskProvider", () => ({
  useDeskContext: () => ({ desk: { loaded: true, status: { openPositions: [], marks: [] } } }),
}));

function position(over: Partial<OpenPosition>): OpenPosition {
  return {
    signalId: "manual-tv-a",
    botKind: "tradingview",
    exchangeId: "binance",
    symbol: "ETHUSDT",
    direction: "long",
    entry: 2448,
    tp: 2520,
    sl: 2410,
    leverage: 3,
    capital: 16.32,
    openedAt: Date.now(),
    timeframe: null,
    signalEntry: 2448,
    riskR: 38,
    plan: null,
    breakevenArmed: false,
    partialFraction: 0,
    partialPrice: null,
    horizonMs: 0,
    live: true,
    qty: 0.02,
    entryOrderId: 501,
    stopAlgoId: 9001,
    tpAlgoId: null,
    stopAtBreakeven: false,
    unprotected: false,
    partialQty: 0,
    sizingMode: "fixed",
    riskPct: null,
    notionalUsdt: 48.96,
    effectiveLeverage: 3,
    riskCapped: false,
    tpTarget: "tp1",
    tpFallbackFrom: null,
    ...over,
  };
}

let root: Root | null = null;
let host: HTMLDivElement | null = null;

async function mount(positions: OpenPosition[]) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root!.render(<PositionsTable positions={positions} showBot emptyKey="positions.noLiveBot" />));
  // Narrow windows fold lower-priority columns into each row's details.
  for (const b of host.querySelectorAll<HTMLButtonElement>("button[aria-expanded]")) await act(async () => b.click());
  return host;
}

beforeAll(async () => {
  await i18n.changeLanguage("en");
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

// Testnet run, 21 Oct: a live TradingView position showed only "ETHUSDT 0.02 @ 2,448"
// and its stop and take-profit were nowhere in the app.
describe("a live bot position", () => {
  it("shows the bot, entry, SL, TP, size and its exchange stop and take-profit orders", async () => {
    const el = await mount([position({})]);
    const text = el.textContent ?? "";
    expect(text).toContain("ETHUSDT");
    expect(text).toContain(i18n.t("bots.kind.tradingview"));
    expect(text).toContain("2,448");
    expect(text).toContain("2,410");
    expect(text).toContain("2,520");
    expect(text).toContain("0.02");
    expect(text).toContain("Exchange stop");
    expect(text).toContain("Order 9001");
    // No exchange take-profit was placed: said, not hidden.
    expect(text).toContain("Exchange TP");
    expect(text).toContain("None");
  });

  it("a paper-only list shows no exchange order columns", async () => {
    const el = await mount([position({ live: false, stopAlgoId: null, qty: 0 })]);
    expect(el.textContent).not.toContain("Exchange stop");
  });

  it("an empty live list says no bot holds a real position", async () => {
    const el = await mount([]);
    expect(el.textContent).toContain("No bot holds a real position");
  });
});
