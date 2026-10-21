// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import i18n from "@/i18n";
import { ModePicker } from "@/components/LiveMode/LiveMode";
import type { BotConfig, BotDeskStatus, TradeMode } from "@/lib/ipc/bot/bot";
import { botMode, needsArm } from "@/lib/ipc/bot/bot";
import type { StrategyBotView, StrategyLiveView } from "@/lib/ipc/strategy/strategy";
import {
  holdsPaper,
  liveBlockers,
  liveBots,
  liveChipTooltipKey,
  liveRunningCount,
  liveTermsChanged,
  liveWordKey,
  ordersOnTestnet,
  scopeLabelKey,
  signalStartAction,
  type Blocker,
} from "./liveMode";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const paper: BotConfig = { kind: "futures", exchangeId: "binance", maxPositions: 2, capital: 50, leverage: 3 };
const liveArmed: BotConfig = { ...paper, mode: "live", live: true };
const liveIdle: BotConfig = { ...paper, mode: "live", live: false };

const ready = { futures: true, keyOk: true, venueOk: true, venueName: "Binance", tradingBlocked: null };

describe("the mode of a signal bot", () => {
  it("is Paper unless stored as Live; a restored Live bot needs its arm", () => {
    expect(botMode(null)).toBe("paper");
    expect(botMode(paper)).toBe("paper");
    expect(botMode(liveIdle)).toBe("live");
    expect(botMode(liveArmed)).toBe("live");
    expect(needsArm(liveIdle)).toBe(true);
    expect(needsArm(liveArmed)).toBe(false);
    expect(needsArm(paper)).toBe(false);
  });

  it("Start with Live chosen goes through the typed LIVE unless the bot is armed on the same terms", () => {
    expect(signalStartAction(null, "paper", paper)).toBe("start");
    expect(signalStartAction(null, "live", paper)).toBe("confirmLive");
    expect(signalStartAction(paper, "live", paper)).toBe("confirmLive");
    expect(signalStartAction(liveIdle, "live", liveIdle)).toBe("confirmLive");
    expect(signalStartAction(liveArmed, "live", liveArmed)).toBe("start");
    expect(signalStartAction(liveArmed, "live", { ...liveArmed, capital: 80 })).toBe("confirmLive");
    expect(signalStartAction(liveArmed, "live", { ...liveArmed, symbols: ["SOLUSDT"] })).toBe("start");
    expect(signalStartAction(liveArmed, "paper", liveArmed)).toBe("toPaper");
    expect(liveTermsChanged(paper, { ...paper, exchangeId: "okx" })).toBe(true);
  });
});

describe("why Live cannot be chosen", () => {
  it("lists every missing condition in the order the user fixes them", () => {
    expect(liveBlockers(ready)).toEqual([]);
    const keys = (b: Blocker[]) => b.map((x) => x.key);
    expect(keys(liveBlockers({ ...ready, futures: false }))).toEqual(["liveMode.blockers.futuresOnly"]);
    expect(keys(liveBlockers({ ...ready, keyOk: false, venueOk: false }))).toEqual(["vault.venueNotDryRun", "liveMode.blockers.key"]);
    expect(keys(liveBlockers({ ...ready, tradingBlocked: "riskConfigUnreadable" }))).toEqual(["liveMode.blockers.tradingBlocked"]);
    expect(keys(liveBlockers({ ...ready, extra: [{ key: "liveMode.blockers.stop" }] }))).toEqual(["liveMode.blockers.stop"]);
  });

  it("a bot holding paper positions cannot take real money", () => {
    const pos = (botKind: "futures" | "spot", live: boolean) => ({ botKind, live }) as BotDeskStatus["openPositions"][number];
    expect(holdsPaper([pos("futures", false)], "futures")).toBe(true);
    expect(holdsPaper([pos("futures", true), pos("spot", false)], "futures")).toBe(false);
  });
});

describe("the header's Live count", () => {
  it("counts Live bots taking new entries or cycles, and lists the rest", () => {
    const s = {
      futures: liveArmed,
      spot: { ...paper, kind: "spot" },
      pump: null,
      futuresRunning: true,
      spotRunning: true,
      pumpRunning: false,
    } as unknown as BotDeskStatus;
    const views = [
      { botId: "sb_1", enabled: true, armed: false },
      { botId: "sb_2", enabled: false },
    ] as StrategyLiveView[];
    const bots = [
      { id: "sb_1", name: "DCA BTC", acceptingNewCycles: false, runState: "stopped" },
      { id: "sb_2", name: "Grid ETH", acceptingNewCycles: true, runState: "armed" },
    ] as unknown as StrategyBotView[];
    const lines = liveBots(s, views, bots);
    expect(lines.map((l) => [l.id, l.running, l.needsArm])).toEqual([
      ["futures", true, false],
      ["sb_1", false, true],
    ]);
    expect(liveRunningCount(lines)).toBe(1);
    expect(liveRunningCount(liveBots({ ...s, futuresRunning: false }, [], null))).toBe(0);
  });

  it("lists a Live TradingView bot with its arm state", () => {
    const s = {
      futures: null,
      spot: null,
      pump: null,
      tradingview: { ...liveArmed, kind: "tradingview", live: false },
      futuresRunning: false,
      spotRunning: false,
      pumpRunning: false,
      tradingviewRunning: false,
    } as unknown as BotDeskStatus;
    expect(liveBots(s, [], null).map((l) => [l.id, l.nameKey, l.running, l.needsArm])).toEqual([
      ["tradingview", "bots.kind.tradingview", false, true],
    ]);
  });
});

describe("ModePicker", () => {
  let root: Root | null = null;
  let host: HTMLDivElement | null = null;
  beforeAll(async () => {
    await i18n.changeLanguage("en");
  });
  afterEach(async () => {
    await act(async () => root?.unmount());
    host?.remove();
  });
  const mount = async (value: TradeMode, blockers: Blocker[], onChange: (m: TradeMode) => void = () => {}) => {
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    await act(async () => root!.render(<ModePicker value={value} onChange={onChange} blockers={blockers} liveNote="note" />));
    return host;
  };
  const radio = (el: HTMLElement, text: string) => [...el.querySelectorAll<HTMLButtonElement>("[role=radio]")].find((b) => b.textContent === text)!;

  it("starts on Paper and keeps Live off while anything blocks it, saying what", async () => {
    const el = await mount("paper", [{ key: "liveMode.blockers.key", params: { exchange: "Binance" } }]);
    expect(radio(el, "Paper").getAttribute("aria-checked")).toBe("true");
    expect(radio(el, "Live").disabled).toBe(true);
    expect(el.textContent).toContain("A verified trade-only Binance key in the unlocked vault");
  });

  it("offers Live when nothing blocks it; choosing it is the user's click", async () => {
    let chosen: TradeMode | null = null;
    const el = await mount("paper", [], (m) => (chosen = m));
    expect(radio(el, "Live").disabled).toBe(false);
    expect(chosen).toBeNull();
    await act(async () => radio(el, "Live").click());
    expect(chosen).toBe("live");
  });
});

// Testnet run, 21 Oct: the testnet build said "Start with real money",
// "TradingView: real money" and a "Real money" statistics tab.
describe("the testnet build says testnet, production keeps real money", () => {
  beforeAll(async () => {
    await i18n.changeLanguage("en");
  });
  const st = (binanceIsProduction: boolean) => ({ binanceIsProduction }) as BotDeskStatus;

  it("orders go to the testnet only once a status says so", () => {
    expect(ordersOnTestnet(st(false), true)).toBe(true);
    expect(ordersOnTestnet(st(true), true)).toBe(false);
    expect(ordersOnTestnet(st(false), false)).toBe(false);
  });

  it("each real-money label has its testnet wording", () => {
    const bot = { bot: "TradingView" };
    expect(i18n.t(liveWordKey("startLive", true))).toBe("Start on testnet");
    expect(i18n.t(liveWordKey("startLive", false))).toBe("Start with real money");
    expect(i18n.t(liveWordKey("goLiveTitle", true), bot)).toBe("TradingView: testnet orders");
    expect(i18n.t(liveWordKey("goLiveTitle", false), bot)).toBe("TradingView: real money");
    expect(i18n.t(scopeLabelKey("real", true))).toBe("Testnet");
    expect(i18n.t(scopeLabelKey("real", false))).toBe("Real money");
    expect(i18n.t(scopeLabelKey("simulated", true))).toBe("Paper");
  });
});

// Testnet run, 21 Oct: a grey "Live running: 0" with no hint.
describe("the Live running chip", () => {
  it("says how to get a Live bot running when none is", async () => {
    await i18n.changeLanguage("en");
    expect(i18n.t(liveChipTooltipKey(0))).toBe("No live bot running. Start a bot in Live mode to place real orders.");
    expect(liveChipTooltipKey(2)).toBe("liveMode.header.tooltip");
    for (const lang of ["tr", "es", "hi", "id", "pt-BR", "ru", "vi"]) {
      expect(i18n.getResource(lang, "translation", "liveMode.header.tooltipNone"), lang).toBeTruthy();
    }
    expect(i18n.getResource("tr", "translation", "liveMode.header.tooltipNone")).toContain("Canlı modda başlatın");
  });
});

// Testnet run, 21 Oct: "paper" and "simulated" mixed ("Switch to simulated").
describe("one mode word per language", () => {
  const MODE_KEYS = [
    "bots.live.switchOff",
    "pnl.scope.simulated",
    "errors.liveExchangeLocked",
    "positions.noPaper",
    "positions.closeAllPaper.title",
    "positions.closeAllPaper.body",
    "settings.backup.created",
    "settings.backup.replaced",
    "settings.backup.importRule",
    "settings.backup.promise.signal",
    "settings.backup.promise.strategy",
    "liveMode.signal.toPaperTitle",
  ];
  it("English says Paper, never simulated, for the mode", () => {
    for (const k of MODE_KEYS) expect(i18n.getResource("en", "translation", k), k).not.toMatch(/simulat/i);
    expect(i18n.getResource("en", "translation", "bots.live.switchOff")).toBe("Switch to Paper");
    expect(i18n.getResource("en", "translation", "liveMode.paper")).toBe("Paper");
  });
  it("Turkish says Simülasyon / Canlı", () => {
    expect(i18n.getResource("tr", "translation", "liveMode.paper")).toBe("Simülasyon");
    expect(i18n.getResource("tr", "translation", "liveMode.live")).toBe("Canlı");
    expect(i18n.getResource("tr", "translation", "bots.live.switchOff")).toBe("Simülasyona geç");
  });
  it("each other language uses its own Paper word in the switch and the scope", () => {
    const word: Record<string, RegExp> = { es: /papel/i, hi: /पेपर/, id: /Paper/, "pt-BR": /papel/i, ru: /бума[гж]/i, vi: /giấy/i };
    for (const [lang, re] of Object.entries(word)) {
      expect(i18n.getResource(lang, "translation", "bots.live.switchOff"), lang).toMatch(re);
      expect(i18n.getResource(lang, "translation", "pnl.scope.simulated"), lang).toMatch(re);
      expect(i18n.getResource(lang, "translation", "positions.closeAllPaper.title"), lang).toMatch(re);
    }
  });
});

describe("the TradingView bot's money at risk", () => {
  it("a new budget or max size per alert is new terms to confirm", () => {
    const tvCfg: BotConfig = {
      kind: "tradingview",
      exchangeId: "binance",
      maxPositions: 3,
      capital: 33.33,
      leverage: 3,
      tv: { name: "T", budgetUsdt: 100, maxSizeUsdt: 50, maxOpensPerDay: 10, staleS: 30, priceGuardPct: 0.5, requirePrice: true },
    };
    expect(liveTermsChanged(tvCfg, { ...tvCfg, tv: { ...tvCfg.tv!, maxSizeUsdt: 60 } })).toBe(true);
    expect(liveTermsChanged(tvCfg, { ...tvCfg, tv: { ...tvCfg.tv!, staleS: 60 } })).toBe(false);
  });
});
