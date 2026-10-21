// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "@/i18n";
import { ManualChip } from "@/components/ui/Chip/Chip";
import { isUserTradeId, type BotConfig, type BotDeskStatus, type TradePreview } from "@/lib/ipc/bot/bot";
import { mock as botMock } from "@/lib/ipc/bot/bot.mock";
import { liveTradeBlockers, NewTradeForm, NewTradePreview, PREVIEW_DEBOUNCE_MS, tradeRequest, tradeTerms, useNewTrade, type TradeForm } from "./NewTrade";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
window.matchMedia ??= ((query: string) =>
  ({ matches: false, media: query, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, onchange: null, dispatchEvent: () => false }) as MediaQueryList);

const ipc = vi.hoisted(() => ({ preview: vi.fn(), open: vi.fn() }));
vi.mock("@/lib/ipc/bot/bot", async (orig) => ({ ...(await orig<object>()), botPreviewTrade: ipc.preview, botOpenTrade: ipc.open }));
// The Live confirmation reads the desk for where orders go (production Binance here).
vi.mock("@/app/DeskProvider", () => ({
  useDeskContext: () => ({ desk: { status: { binanceIsProduction: true, venueSandbox: false } }, catalog: { exchanges: null } }),
}));
vi.mock("@/lib/ipc/exchange/exchange", async (orig) => ({
  ...(await orig<object>()),
  exchangeSymbols: () => Promise.resolve(["BTCUSDT", "ETHUSDT", "SOLUSDT"]),
}));

const futures: BotConfig = { kind: "futures", exchangeId: "binance", maxPositions: 3, capital: 100, leverage: 5, live: false };

function status(over: Partial<BotDeskStatus> = {}): BotDeskStatus {
  return {
    liveTradingEnabled: false,
    binanceIsProduction: true,
    futures,
    spot: null,
    pump: null,
    futuresRunning: false,
    spotRunning: false,
    pumpRunning: false,
    openPositions: [],
    recentSkips: [],
    killSwitchTripped: false,
    btcRegime: "normal",
    ...over,
  };
}

/** What bot_preview_trade answers for 500 USDT at 5x, long from 100, TP 110, SL 95. */
const PREVIEW: TradePreview = {
  botKind: "futures",
  symbol: "SOLUSDT",
  direction: "long",
  entry: 100,
  takeProfit: 110,
  stopLoss: 95,
  qty: 5,
  notionalUsdt: 500,
  capital: 100,
  botCapital: 100,
  sizingMode: "fixed",
  effectiveLeverage: 5,
  leverage: 5,
  riskCapped: false,
  lossAtStopUsdt: -25.5,
  riskPctOfCapital: 25.5,
  gainAtTargetUsdt: 49.5,
  rewardRisk: 2,
  liquidationPrice: 82.05,
  feesUsdt: 0.5,
};

function Harness({ s }: { s: BotDeskStatus }) {
  const ctl = useNewTrade(s);
  return (
    <>
      <NewTradeForm ctl={ctl} status={s} />
      <NewTradePreview ctl={ctl} status={s} onClose={() => {}} />
    </>
  );
}

let root: Root | null = null;
let host: HTMLDivElement | null = null;

async function mount(node: React.ReactNode) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root!.render(node));
  return host;
}

const settle = () => act(async () => new Promise((r) => setTimeout(r, PREVIEW_DEBOUNCE_MS + 50)));

/** Types into a controlled input the way React listens for it. */
async function type(el: HTMLElement, label: string, value: string) {
  const input = [...el.querySelectorAll<HTMLLabelElement>("label")].find((l) => l.textContent === label)?.control as HTMLInputElement;
  expect(input, label).toBeTruthy();
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  await act(async () => {
    setter.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

async function fill(el: HTMLElement) {
  await type(el, "Pair", "solusdt");
  await type(el, "Take-profit price", "110");
  await type(el, "Stop-loss price", "95");
}

const preview = () => document.querySelector<HTMLElement>("section.ae-newtrade")!;
const button = (text: string) => [...document.querySelectorAll("button")].find((b) => b.textContent === text);

beforeAll(async () => {
  await i18n.changeLanguage("en");
});
beforeEach(() => {
  ipc.preview.mockReset();
  ipc.open.mockReset();
  ipc.preview.mockResolvedValue(PREVIEW);
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

describe("tradeRequest", () => {
  const base: TradeForm = { mode: "paper", kind: "futures", symbol: "solusdt", direction: "long", sizing: "bot", notional: "", takeProfit: "110", stopLoss: "95" };
  it("waits for the required fields, refuses non-numbers, sends the size only when set", () => {
    expect(tradeRequest({ ...base, symbol: " " })).toBe("incomplete");
    expect(tradeRequest({ ...base, stopLoss: "" })).toBe("incomplete");
    expect(tradeRequest({ ...base, sizing: "usdt" })).toBe("incomplete");
    expect(tradeRequest({ ...base, takeProfit: "abc" })).toBe("tradeLevelsInvalid");
    expect(tradeRequest({ ...base, sizing: "usdt", notional: "0" })).toBe("tradeSizeInvalid");
    expect(tradeRequest(base)).toEqual({ kind: "futures", symbol: "SOLUSDT", direction: "long", takeProfit: 110, stopLoss: 95 });
    expect(tradeRequest({ ...base, sizing: "usdt", notional: "250,5" })).toMatchObject({ notionalUsdt: 250.5 });
  });
});

describe("New trade form", () => {
  it("shows the paper preview as the user types: fill, risk, reward:risk, liquidation, fees", async () => {
    const el = await mount(<Harness s={status()} />);
    expect(preview().textContent).toContain("Enter pair, take-profit and stop-loss");
    expect(el.querySelectorAll(".ae-chip[data-tone=paper]").length).toBeGreaterThan(0);
    await fill(el);
    await settle();
    expect(ipc.preview).toHaveBeenLastCalledWith({ kind: "futures", symbol: "SOLUSDT", direction: "long", takeProfit: 110, stopLoss: 95 });
    const fig = (id: string) => preview().querySelector(`[data-figure="${id}"]`)?.textContent;
    expect(fig("fill")).toBe("Fill now100.00");
    expect(fig("qty")).toBe("Quantity5");
    expect(fig("risk")).toContain("Loss at stop 95");
    expect(fig("risk")).toContain("-25.50 USDT");
    expect(fig("risk")).toContain("25.50% of bot capital");
    expect(fig("gain")).toContain("+49.50 USDT");
    expect(fig("rr")).toBe("Reward:risk2.00");
    expect(fig("liq")).toBe("Liquidation at 5x82.05");
    expect(fig("fees")).toContain("0.50");
    expect(button("Open paper trade")).toBeTruthy();
  });

  it("sends a USDT size and opens the trade with the same request", async () => {
    ipc.open.mockResolvedValue({ symbol: "SOLUSDT", entry: 100.1 });
    const el = await mount(<Harness s={status()} />);
    await fill(el);
    await act(async () => [...el.querySelectorAll<HTMLButtonElement>("[role=radio]")].find((b) => b.textContent === "USDT")!.click());
    await type(el, "Notional (USDT)", "500");
    await settle();
    const req = { kind: "futures", symbol: "SOLUSDT", direction: "long", takeProfit: 110, stopLoss: 95, notionalUsdt: 500 };
    expect(ipc.preview).toHaveBeenLastCalledWith(req);
    await act(async () => button("Open paper trade")!.click());
    expect(ipc.open).toHaveBeenCalledWith(req);
    expect(preview().textContent).toContain("Opened · SOLUSDT at 100.10 · paper");
  });

  it("shows each refusal in words, with no Open button", async () => {
    const cases: [string, string][] = [
      ["tradeStopInsideFees|0.10%", "Stop closer than both fees · needs more than 0.10%"],
      ["tradeTargetWrongSide", "Take-profit on the wrong side of the live price"],
      ["manualTakePaperOnly", "This is a Live bot: choose Live for a real trade"],
      ["belowExchangeMinimum|4.00", "Order would be 4.00 USDT, under the exchange minimum of 5 USDT"],
      ["btcGuardUnknown", "BTC regime unknown · entry refused"],
    ];
    for (const [code, text] of cases) {
      ipc.preview.mockRejectedValue(code);
      const el = await mount(<Harness s={status()} />);
      await fill(el);
      await settle();
      expect(preview().querySelector("[role=status]")?.textContent).toBe(text);
      expect(button("Open paper trade")).toBeUndefined();
      await act(async () => root?.unmount());
      host?.remove();
    }
  });

  it("keeps spot long only", async () => {
    const el = await mount(<Harness s={status({ spot: { ...futures, kind: "spot", leverage: 1 } })} />);
    await act(async () => [...el.querySelectorAll<HTMLButtonElement>("[role=radio]")].find((b) => b.textContent?.startsWith("Spot"))!.click());
    const short = [...el.querySelectorAll<HTMLButtonElement>("[role=radio]")].find((b) => b.textContent === "Short")!;
    expect(short.disabled || short.getAttribute("aria-disabled") === "true").toBe(true);
  });
});

describe("Paper or Live on New trade", () => {
  const radio = (el: HTMLElement, text: string) => [...el.querySelectorAll<HTMLButtonElement>("[role=radio]")].find((b) => b.textContent === text)!;
  const armed: BotConfig = { ...futures, mode: "live", live: true };

  it("Paper is the default; Live is offered only on the Live Futures bot armed this session, and says why not", async () => {
    expect(liveTradeBlockers(status()).map((b) => b.key)).toEqual(["liveMode.blockers.tradeBotPaper"]);
    expect(liveTradeBlockers(status({ futures: { ...futures, mode: "live", live: false } })).map((b) => b.key)).toEqual(["liveMode.blockers.notArmed"]);
    expect(liveTradeBlockers(status({ futures: armed }))).toEqual([]);
    expect(liveTradeBlockers(status({ futures: armed, tradingBlocked: "riskConfigUnreadable" })).map((b) => b.key)).toEqual(["liveMode.blockers.tradingBlocked"]);

    const el = await mount(<Harness s={status()} />);
    expect(radio(el, "Paper").getAttribute("aria-checked")).toBe("true");
    expect(radio(el, "Live").disabled).toBe(true);
    expect(el.textContent).toContain("The Futures bot is a Paper bot");
    // A paper trade never carries a mode or a word.
    await fill(el);
    await settle();
    await act(async () => button("Open paper trade")!.click());
    expect(ipc.open).toHaveBeenCalledWith(expect.not.objectContaining({ mode: "live" }));
  });

  it("a Live trade shows the money at risk, needs the typed LIVE and sends mode and word", async () => {
    ipc.open.mockResolvedValue({ symbol: "SOLUSDT", entry: 100.1 });
    const el = await mount(<Harness s={status({ futures: armed, pilotLeft: 3 })} />);
    await act(async () => radio(el, "Live").click());
    expect(radio(el, "Live").getAttribute("aria-checked")).toBe("true");
    expect([...el.querySelectorAll<HTMLButtonElement>("[role=radio]")].find((b) => b.textContent?.startsWith("Spot"))!.disabled).toBe(true);
    await fill(el);
    await settle();
    expect(ipc.preview).toHaveBeenLastCalledWith(expect.objectContaining({ mode: "live", kind: "futures" }));
    await act(async () => button("Open real position")!.click());
    expect(ipc.open).not.toHaveBeenCalled();
    const dialog = document.querySelector<HTMLElement>(".ae-dialog")!;
    expect(dialog.textContent).toContain("Most margin at risk");
    expect(dialog.textContent).toContain("100.00 USDT");
    expect(dialog.textContent).toContain("Orders go to Binance production");
    const confirm = dialog.querySelector<HTMLButtonElement>(".ae-dialog__confirm")!;
    expect(confirm.disabled).toBe(true);
    const input = dialog.querySelector<HTMLInputElement>("input")!;
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    await act(async () => {
      setter.call(input, "LIVE");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(confirm.disabled).toBe(false);
    await act(async () => confirm.click());
    expect(ipc.open).toHaveBeenCalledWith(expect.objectContaining({ mode: "live", confirmation: "LIVE", kind: "futures", symbol: "SOLUSDT" }));
    // The margin the confirmation showed goes back: Rust re-plans and refuses `liveTermsChanged` when it moved.
    expect(ipc.open).toHaveBeenCalledWith(expect.objectContaining({ confirmedMarginUsdt: PREVIEW.capital }));
  });

  it("the pilot caps the size the confirmation shows", () => {
    const t = tradeTerms(PREVIEW, "binance", 2);
    expect(t.maxNotionalUsdt).toBe(50);
    expect(t.marginAtRiskUsdt).toBe(100);
    expect(tradeTerms(PREVIEW, "binance", 0).maxNotionalUsdt).toBe(500);
  });
});

describe("the browser preview's mock", () => {
  const req = { kind: "futures" as const, symbol: "SOLUSDT", direction: "long" as const, takeProfit: 110, stopLoss: 95 };
  it("refuses like Rust up to the live price, which it does not have", async () => {
    await expect(botMock.trade(req)).rejects.toBe("botNotConfigured");
    await botMock.configure({ ...futures, capital: 10 });
    await expect(botMock.trade({ ...req, kind: "pump" })).rejects.toBe("botNotConfigured");
    await expect(botMock.trade({ ...req, symbol: "SOL-USD" })).rejects.toBe("tradeSymbolInvalid");
    await expect(botMock.trade({ ...req, stopLoss: 0 })).rejects.toBe("tradeLevelsInvalid");
    await expect(botMock.trade({ ...req, notionalUsdt: 4 })).rejects.toBe("belowExchangeMinimum|4.00");
    await expect(botMock.trade(req)).rejects.toBe("priceUnavailable");
  });
});

describe("own trades are told apart from Sentinel signals", () => {
  it("labels a New trade Own and a taken signal Manual", async () => {
    expect(isUserTradeId("manual-1790000000000-0000abcd")).toBe(true);
    expect(isUserTradeId("4f1c2a7e-0d3b-4a51-9a0c-1e2f3a4b5c6d")).toBe(false);
    const el = await mount(
      <>
        <ManualChip signalId="manual-1790000000000-0000abcd" />
        <ManualChip signalId="4f1c2a7e" />
      </>,
    );
    expect([...el.querySelectorAll(".ae-chip")].map((c) => c.textContent)).toEqual(["Own", "Manual"]);
  });
});
