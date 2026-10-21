// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "@/i18n";
import type { BotConfig } from "@/lib/ipc/bot/bot";
import type { TvAlertResult } from "@/lib/ipc/tvlink/tvlink";
import { mock, PREVIEW_URL } from "@/lib/ipc/tvlink/tvlink.mock";
import { splitColumns } from "@/components/ui/DataTable/columns";
import { alertBotLabel, RESULTS_BREAKPOINTS, TradingViewTab, tvOutcomeText } from "./TradingViewTab";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** The desk the tab reads the TradingView bot from. */
const desk = vi.hoisted(() => ({ tradingview: null as BotConfig | null, running: false }));
/** The handler the tab subscribed to Rust's `tvlink:result` event with. */
const events = vi.hoisted(() => ({ handlers: [] as ((id: string) => void)[] }));
vi.mock("@/lib/ipc/tvlink/tvlink", async (orig) => ({
  ...(await orig<object>()),
  onTvResult: (h: (id: string) => void) => {
    events.handlers.push(h);
    return Promise.resolve(() => {
      events.handlers = events.handlers.filter((x) => x !== h);
    });
  },
}));
vi.mock("@/app/DeskProvider", () => ({
  useDeskContext: () => ({ desk: { loaded: true, status: { tradingview: desk.tradingview, tradingviewRunning: desk.running } } }),
}));

const TV_BOT: BotConfig = {
  kind: "tradingview",
  exchangeId: "binance",
  maxPositions: 3,
  capital: 33.33,
  leverage: 3,
  mode: "live",
  live: true,
  tv: { name: "Trend A", budgetUsdt: 100, maxSizeUsdt: 50, maxOpensPerDay: 10, staleS: 30, priceGuardPct: 0.5, requirePrice: true },
};

let root: Root | null = null;
let host: HTMLDivElement | null = null;

async function mount() {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root!.render(<TradingViewTab />));
  return host;
}

const button = (el: ParentNode, text: string) => [...el.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === text);
const sw = (el: ParentNode, id: string) => el.querySelector<HTMLInputElement>(`#${id}`)!;
const click = async (el: Element) => act(async () => (el as HTMLElement).click());

function row(over: Partial<TvAlertResult>): TvAlertResult {
  return {
    eventId: "1",
    receivedAt: Date.UTC(2026, 9, 16, 9, 0, 0),
    handledAt: Date.UTC(2026, 9, 16, 9, 0, 2),
    action: "buy",
    symbol: "BTCUSDT",
    bot: "futures",
    outcome: "opened",
    positionId: "manual-1-0000000a",
    test: false,
    ...over,
  };
}

beforeAll(async () => {
  await i18n.changeLanguage("en");
});
beforeEach(() => {
  mock.reset();
  desk.tradingview = null;
  desk.running = false;
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

describe("TradingViewTab", () => {
  it("starts with no URL, both switches off and no alerts", async () => {
    const el = await mount();
    expect(el.textContent).toContain("No webhook URL yet.");
    expect(el.textContent).toContain("No URL");
    expect(button(el, "Create URL")).toBeTruthy();
    expect(button(el, "Revoke")).toBeUndefined();
    expect(sw(el, "tv-receive").checked).toBe(false);
    expect(sw(el, "tv-execute").checked).toBe(false);
    expect(el.textContent).toContain("Paper");
    expect(el.textContent).toContain("No alerts read yet.");
    expect(el.textContent).toContain("TradingView requires 2FA for webhooks.");
  });

  it("shows the URL once after create, then rotate and revoke", async () => {
    const el = await mount();
    await click(button(el, "Create URL")!);
    expect(el.textContent).toContain(PREVIEW_URL);
    expect(el.textContent).toContain("Shown once: the app does not store it");
    expect(el.textContent).toContain("Active");
    expect(el.textContent).toContain("Last alert received");
    expect(el.textContent).toContain("Never");
    expect(button(el, "Rotate URL")).toBeTruthy();

    const writeText = vi.fn(() => Promise.resolve());
    Object.assign(navigator, { clipboard: { writeText } });
    const copies = [...el.querySelectorAll<HTMLButtonElement>("button")].filter((b) => b.textContent === "Copy");
    await click(copies[0]);
    expect(writeText).toHaveBeenCalledWith(PREVIEW_URL);
    expect(el.textContent).toContain("Copied");

    await click(button(el, "Revoke")!);
    expect(el.textContent).not.toContain(PREVIEW_URL);
    expect(el.textContent).toContain("No webhook URL yet.");
  });

  it("execute turns receive on; receive off turns execute off", async () => {
    const el = await mount();
    await click(sw(el, "tv-execute"));
    expect(sw(el, "tv-execute").checked).toBe(true);
    expect(sw(el, "tv-receive").checked).toBe(true);
    await click(sw(el, "tv-receive"));
    expect(sw(el, "tv-receive").checked).toBe(false);
    expect(sw(el, "tv-execute").checked).toBe(false);
  });

  it("signed out: a sign-in line and no switches to use", async () => {
    mock.setSignedIn(false);
    const el = await mount();
    expect(el.textContent).toContain("Sign in to use TradingView alerts.");
    expect(button(el, "Sign in")).toBeTruthy();
    expect(button(el, "Create URL")).toBeUndefined();
    expect(sw(el, "tv-receive").disabled).toBe(true);
    expect(sw(el, "tv-execute").disabled).toBe(true);
  });

  it("lists recent alerts with each outcome in words", async () => {
    mock.push(row({ eventId: "1" }));
    mock.push(row({ eventId: "2", action: "sell", outcome: "tvExpired", positionId: null }));
    mock.push(row({ eventId: "3", action: null, symbol: null, bot: null, outcome: "tvBadMessage", positionId: null }));
    mock.push(row({ eventId: "4", action: "close", outcome: "tvNothingToClose", positionId: null }));
    mock.push(row({ eventId: "5", outcome: "tradeTargetWrongSide", positionId: null }));
    const el = await mount();
    const text = el.textContent ?? "";
    expect(text).toContain("Opened");
    expect(text).toContain("Not executed · older than the bot's stale window (server clock)");
    expect(text).toContain("Unreadable · not JSON in the alert format");
    expect(text).toContain("Nothing to close");
    expect(text).toContain("Take-profit on the wrong side of the live price");
    expect(text).toContain("Futures");
  });

  it("shows requested vs filled, the fill, slippage, latency and the bot that handled it", async () => {
    mock.push(
      row({
        eventId: "9",
        botName: "Trend A",
        mode: "live",
        requestedQty: 0.0008,
        filledQty: 0.0007,
        avgFill: 65010.5,
        alertPrice: 65000,
        slippagePct: 0.016,
        latencyMs: 840,
      }),
    );
    mock.push(row({ eventId: "10", outcome: "livePending|liveOrderUnconfirmed", positionId: null, botName: "Trend A", mode: "live" }));
    const el = await mount();
    // Narrow windows fold the lower-priority columns into each row's details.
    for (const b of el.querySelectorAll<HTMLButtonElement>("button[aria-expanded]")) await click(b);
    const text = el.textContent ?? "";
    expect(text).toContain("Trend A");
    expect(text).toContain("0.0008 / 0.0007");
    expect(text).toContain("65,011");
    expect(text).toContain("Trend ALIVE");
    expect(text).toContain("+0.02%");
    expect(text).toContain("840 ms");
    expect(text).toContain("Order outcome unknown · being looked up, never sent again · The exchange did not confirm the entry");
  });

  // Testnet run, 21 Oct: latency read "0 mins"; a stale row read Bot = "Futures".
  it("shows latency in ms or seconds, and a futures alert row as the TradingView bot's", async () => {
    mock.push(row({ eventId: "11", latencyMs: 3_240, botName: "Trend A", mode: "paper" }));
    mock.push(row({ eventId: "12", outcome: "tvExpired", positionId: null }));
    const el = await mount();
    for (const b of el.querySelectorAll<HTMLButtonElement>("button[aria-expanded]")) await click(b);
    const text = el.textContent ?? "";
    expect(text).toContain("3.2 sec");
    expect(text).not.toContain("0 min");
    // The stale futures alert, recorded without running: the TradingView bot's.
    expect(alertBotLabel(i18n.t, { bot: "futures", botName: null })).toBe(i18n.t("bots.kind.tradingview"));
    expect(alertBotLabel(i18n.t, { bot: "futures", botName: null })).not.toBe(i18n.t("bots.kind.futures"));
    expect(alertBotLabel(i18n.t, { bot: "spot", botName: null })).toBe(i18n.t("bots.kind.spot"));
    expect(alertBotLabel(i18n.t, { bot: "futures", botName: "Trend A" })).toBe("Trend A");
    expect(alertBotLabel(i18n.t, { bot: null, botName: null })).toBeNull();
  });

  // Owner check, 21 Oct: in a narrow window the table cut off slippage and latency.
  it("keeps slippage and latency in view in the narrowest table; bot, quantity and fill fold first", async () => {
    mock.push(row({ eventId: "21", botName: "Trend A", mode: "live", requestedQty: 0.02, filledQty: 0.02, avgFill: 2448, slippagePct: 0.03, latencyMs: 842 }));
    const el = await mount();
    // jsdom measures the table at 0 px: only the columns that never fold by width are shown.
    const heads = [...el.querySelectorAll("th")].map((th) => th.textContent);
    expect(heads).toContain(i18n.t("settings.tv.col.slippage"));
    expect(heads).toContain(i18n.t("settings.tv.col.latency"));
    expect(heads).toContain(i18n.t("settings.tv.col.action"));
    for (const folded of ["bot", "qty", "fill"]) expect(heads).not.toContain(i18n.t(`settings.tv.col.${folded}`));
    // The folded values are one click away in the row's details.
    for (const b of el.querySelectorAll<HTMLButtonElement>("button[aria-expanded]")) await click(b);
    expect(el.textContent).toContain("0.02 / 0.02");
    expect(el.textContent).toContain("Trend A");
  });

  it("shows every results column from a 720 px content column", () => {
    const cols = [{ id: "a" }, { id: "b", priority: 2 as const }, { id: "c", priority: 3 as const }];
    expect(splitColumns(cols, 720, RESULTS_BREAKPOINTS).hidden).toEqual([]);
    expect(splitColumns(cols, 600, RESULTS_BREAKPOINTS).hidden.map((c) => c.id)).toEqual(["c"]);
  });

  it("names the pair's minimum when an alert is below it", () => {
    const has = (k: string) => i18n.exists(k);
    expect(tvOutcomeText(i18n.t, has, "tvBelowSymbolMinimum|BTCUSDT|83.00|50.00|50.00")).toBe(
      "Not executed · BTCUSDT needs at least 83.00 USDT per order; this alert asked for 50.00 USDT (max size 50.00 USDT)",
    );
  });

  // Testnet run, 21 Oct: new rows appeared only after switching tabs.
  it("adds a handled alert's row as soon as Rust reports it, without a poll", async () => {
    const el = await mount();
    expect(el.textContent).toContain("No alerts read yet");
    expect(events.handlers.length).toBeGreaterThan(0);
    mock.push(row({ eventId: "77", symbol: "SOLUSDT", outcome: "opened" }));
    await act(async () => {
      events.handlers[events.handlers.length - 1]("77");
      await new Promise((r) => setTimeout(r, 0));
    });
    expect(el.textContent).toContain("SOLUSDT");
    expect(el.textContent).not.toContain("No alerts read yet");
  });

  it("says why the stream fell back to the poll", async () => {
    const st = await mock.status();
    vi.spyOn(mock, "status").mockResolvedValueOnce({ ...st, prefs: { receive: true, execute: false }, transport: "poll", online: true, streamError: "tvStreamIdle" });
    const el = await mount();
    expect(el.textContent).toContain("Online · polling · stream failed: The TradingView event stream sent nothing for 45 s; reading by poll");
  });

  it("says webhooks need a paid TradingView plan, and the test needs a URL and Receive on", async () => {
    const el = await mount();
    const note = "TradingView sends webhooks only on paid plans; the test button checks the rest of the chain.";
    expect(el.textContent).toContain(note);
    expect(button(el, "Send test alert")).toBeUndefined();
    await click(button(el, "Create URL")!);
    expect(button(el, "Send test alert")!.disabled).toBe(true);
    expect(el.textContent).toContain("Turn on Receive alerts to read the test alert.");
    await click(sw(el, "tv-receive"));
    expect(button(el, "Send test alert")!.disabled).toBe(false);
    expect(el.textContent).not.toContain("Turn on Receive alerts to read the test alert.");
  });

  it("a test alert is listed with a TEST chip and no order, even with Execute on", async () => {
    const el = await mount();
    await click(button(el, "Create URL")!);
    await click(sw(el, "tv-execute"));
    await click(button(el, "Send test alert")!);
    const text = el.textContent ?? "";
    expect(text).toContain("Test alert sent.");
    expect(text).toContain("Test only · no order");
    expect(el.querySelector('.ae-chip[data-tone="warn"]')?.textContent).toBe("Test");
    expect(text).not.toContain("Opened");
    expect(text).toContain("BTCUSDT");
  });

  it("the sixth test in an hour is refused in words", async () => {
    const el = await mount();
    await click(button(el, "Create URL")!);
    await click(sw(el, "tv-receive"));
    for (let i = 0; i < 5; i++) await click(button(el, "Send test alert")!);
    await click(button(el, "Send test alert")!);
    expect(el.querySelector('[role="alert"]')?.textContent).toBe("Test limit reached · 5 test alerts an hour");
    expect(el.querySelectorAll('.ae-chip[data-tone="warn"]').length).toBe(5);
  });

  it("the message builder writes the JSON, switches to strategy mode and copies it", async () => {
    const el = await mount();
    const built = () => el.querySelector('[data-testid="tv-built"]')?.textContent ?? null;
    expect(built()).toBe('{"action":"buy","symbol":"{{ticker}}","tp_pct":3,"sl_pct":1.5,"price":"{{close}}","id":"{{timenow}}"}');
    expect(el.textContent).toContain("symbol: {{ticker}} or a pair.");
    expect(el.textContent).toContain("Add price ({{close}})");
    const radio = (name: string) => [...el.querySelectorAll<HTMLElement>('[role="radio"]')].find((r) => r.textContent === name)!;
    // Close: no levels.
    await click(radio("Close"));
    expect(JSON.parse(built()!)).toEqual({ action: "close", symbol: "{{ticker}}", price: "{{close}}", id: "{{timenow}}" });
    // Strategy mode: TradingView's order action and the position after it.
    await click(radio("Strategy"));
    expect(JSON.parse(built()!)).toMatchObject({ action: "{{strategy.order.action}}", market_position: "{{strategy.market_position}}" });
    expect(el.textContent).toContain("Strategy mode sends {{strategy.order.action}} and {{strategy.market_position}}");
    expect(el.textContent).toContain("A reversal (long to short, short to long) closes first");
    const writeText = vi.fn(() => Promise.resolve());
    Object.assign(navigator, { clipboard: { writeText } });
    const copyBuilt = [...el.querySelectorAll<HTMLButtonElement>("button")].find(
      (b) => b.textContent === "Copy" && b.previousElementSibling?.getAttribute("data-testid") === "tv-built",
    )!;
    await click(copyBuilt);
    expect(writeText).toHaveBeenCalledWith(built());
    // A level the reader would refuse: no JSON, the reason instead.
    const tp = [...el.querySelectorAll<HTMLInputElement>("input")].find((i) => i.value === "3")!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(tp, "");
      tp.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(built()).toBeNull();
    expect(el.textContent).toContain("Give a take-profit and a stop-loss above 0");
  });

  it("shows the link state, the TradingView bot and the offline notice", async () => {
    const el = await mount();
    expect(el.textContent).toContain("Receive off");
    expect(el.textContent).toContain("None yet · create one");
    await click(button(el, "Create URL")!);
    // Telegram not linked: the notice cannot be switched on.
    expect(sw(el, "tv-notice").disabled).toBe(true);
    expect(el.textContent).toContain("Link Telegram in Notifications to use it.");
  });

  it("names a Live TradingView bot and what its alerts do", async () => {
    desk.tradingview = TV_BOT;
    desk.running = true;
    const el = await mount();
    expect(el.textContent).toContain("Trend A · Running");
    expect(el.textContent).toContain("futures alerts place real orders on Binance");
    expect(el.textContent).toContain("Last seen by the server");
  });
});

describe("tvOutcomeText", () => {
  const t = i18n.t.bind(i18n);
  const exists = (k: string) => i18n.exists(k);
  it("reads this panel's codes, then New trade's and the shared ones", () => {
    expect(tvOutcomeText(t, exists, "received")).toBe("Received · not executed (Execute off)");
    expect(tvOutcomeText(t, exists, "closed")).toBe("Closed");
    expect(tvOutcomeText(t, exists, "flipped")).toBe("Reversed · closed, then opened");
    expect(tvOutcomeText(t, exists, "tvPriceMoved|0.73%")).toBe("Not executed · price moved 0.73% from the alert's price");
    expect(tvOutcomeText(t, exists, "tvSymbolHeldBy|futures")).toBe("Not opened · this pair is held by the Futures bot (one real position per pair)");
    expect(tvOutcomeText(t, exists, "tvCloseNotOwn|strategy")).toBe("Not closed · this pair is held by the DCA / Grid bot, not the TradingView bot");
    expect(tvOutcomeText(t, exists, "tvFlipOpenRefused|tvBudgetFull|10.00")).toBe(
      "Reversal: closed, the new position was refused · The bot's budget is full · 10.00 USDT left",
    );
    expect(tvOutcomeText(t, exists, "liveTermsChanged|50.00")).toContain("50.00");
    expect(tvOutcomeText(t, exists, "tvExpiredOffline")).toBe("Not executed · arrived while the app was offline");
    expect(tvOutcomeText(t, exists, "tvTestOnly")).toBe("Test only · no order");
    expect(tvOutcomeText(t, exists, "tradeStopInsideFees|0.10%")).toBe("Stop closer than both fees · needs more than 0.10%");
    expect(tvOutcomeText(t, exists, "manualTakePaperOnly")).toBe("This is a Live bot: choose Live for a real trade");
    expect(tvOutcomeText(t, exists, "spotLongOnly")).toContain("long-only");
    expect(tvOutcomeText(t, exists, "positionNotFound")).toBe("This position is no longer open.");
    expect(tvOutcomeText(t, exists, "somethingNew")).toBe("somethingNew");
  });
});
