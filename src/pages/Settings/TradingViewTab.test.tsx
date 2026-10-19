// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "@/i18n";
import { TV_TEMPLATES, type TvAlertResult } from "@/lib/ipc/tvlink/tvlink";
import { mock, PREVIEW_URL } from "@/lib/ipc/tvlink/tvlink.mock";
import { TradingViewTab, tvOutcomeText } from "./TradingViewTab";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

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
    expect(text).toContain("Opened · paper");
    expect(text).toContain("Not executed · older than 120 s when read");
    expect(text).toContain("Unreadable · not JSON in the alert format");
    expect(text).toContain("Nothing to close");
    expect(text).toContain("Take-profit on the wrong side of the live price");
    expect(text).toContain("BTCUSDT · Futures");
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
    expect(text).not.toContain("Opened · paper");
    expect(text).toContain("BTCUSDT · Futures");
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

  it("offers the three message templates with TradingView's ticker placeholder", async () => {
    const el = await mount();
    for (const tpl of Object.values(TV_TEMPLATES)) expect(el.textContent).toContain(tpl);
    expect(el.textContent).toContain("symbol: {{ticker}} or a pair.");
    expect(JSON.parse(TV_TEMPLATES.buy)).toMatchObject({ action: "buy", tp_pct: 3, sl_pct: 1.5, bot: "futures" });
    expect(JSON.parse(TV_TEMPLATES.close)).toEqual({ action: "close", symbol: "{{ticker}}", bot: "futures" });
  });
});

describe("tvOutcomeText", () => {
  const t = i18n.t.bind(i18n);
  const exists = (k: string) => i18n.exists(k);
  it("reads this panel's codes, then New trade's and the shared ones", () => {
    expect(tvOutcomeText(t, exists, "received")).toBe("Received · not executed (Execute off)");
    expect(tvOutcomeText(t, exists, "closed")).toBe("Closed · paper");
    expect(tvOutcomeText(t, exists, "tvTestOnly")).toBe("Test only · no order");
    expect(tvOutcomeText(t, exists, "tradeStopInsideFees|0.10%")).toBe("Stop closer than both fees · needs more than 0.10%");
    expect(tvOutcomeText(t, exists, "manualTakePaperOnly")).toBe("This bot trades real money · New trade opens paper positions only");
    expect(tvOutcomeText(t, exists, "spotLongOnly")).toContain("long-only");
    expect(tvOutcomeText(t, exists, "positionNotFound")).toBe("This position is no longer open.");
    expect(tvOutcomeText(t, exists, "somethingNew")).toBe("somethingNew");
  });
});
