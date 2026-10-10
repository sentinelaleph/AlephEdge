import { beforeAll, describe, expect, it } from "vitest";
import i18n from "@/i18n";
import { configErrorText, cycleNetQuote, neverStarted, runActionKey, strategyErrorText, strategyNoteText, universeText } from "./strategyText";
import { isLiveView } from "@/pages/Bots/strategy/useStrategyLive";
import type { StrategyLiveView } from "@/lib/ipc/strategy/strategy";

beforeAll(async () => {
  await i18n.changeLanguage("en");
});

describe("strategyErrorText", () => {
  it("puts the Rust detail into the sentence instead of a raw {{detail}}", () => {
    const text = strategyErrorText(i18n.t.bind(i18n), "liveOrderBelowMinNotional|100.00");
    expect(text).toContain("100.00");
    expect(text).not.toContain("{{");
    expect(strategyErrorText(i18n.t.bind(i18n), "liveConfirmRequired|LIVE")).toContain("LIVE");
  });

  it("states what is free under the budget cap and the cap itself", () => {
    const text = strategyErrorText(i18n.t.bind(i18n), "budgetCapReached|budget|600|1000");
    expect(text).toContain("600");
    expect(text).toContain("1000");
    expect(strategyErrorText(i18n.t.bind(i18n), "budgetCapReached||0|200")).toContain("200");
  });

  it("keeps an unknown code visible", () => {
    expect(strategyErrorText(i18n.t.bind(i18n), "someNewRustCode")).toBe("someNewRustCode");
  });

  // Every code strategy_live.rs can return has a sentence.
  it.each([
    "liveBuildDisabled",
    "liveFuturesBinanceOnly",
    "liveNeedsTradeKey",
    "liveNeedsVerifiedKey",
    "liveNeedsIdleBot",
    "liveNeedsStop",
    "liveSymbolBusy",
    "liveCloseFirst",
    "liveSymbolNotTrading",
    "liveExchangeCheckFailed",
    "liveEditLocked",
    "liveHaltedStart",
  ])("%s has a sentence", (code) => {
    expect(strategyErrorText(i18n.t.bind(i18n), code)).not.toBe(code);
  });
});

describe("cycleNetQuote", () => {
  // Audit 8 Oct: the Cycles tab showed stored % x today's budget, so a +20
  // USDT cycle read +10.00 after the budget was halved.
  it("is the row's own net money, whatever the budget is now", () => {
    expect(cycleNetQuote({ realizedQuote: 21, feesQuote: 0.8, fundingQuote: 0.2, closedAt: 1 })).toBeCloseTo(20, 12);
    expect(cycleNetQuote({ realizedQuote: 21, feesQuote: 0.8, fundingQuote: 0.2, closedAt: null })).toBeNull();
  });
});

describe("universeText", () => {
  it("names a rank range for a template that skips the top", () => {
    const t = i18n.t.bind(i18n);
    const alts = { rule: "topUsdtPerpsByQuoteVolume", rankFrom: 6, topN: 15, volumeWindowDays: 90, reselect: "monthly", exclude: [] };
    expect(universeText(t, alts)).toContain("6");
    expect(universeText(t, alts)).toContain("15");
    expect(universeText(t, alts)).not.toMatch(/^Top 15/);
    expect(universeText(t, { ...alts, rankFrom: 1, topN: 5 })).toMatch(/5/);
  });
});

describe("configErrorText", () => {
  it("names the budget that passes the 5 USDT minimum", () => {
    const t = i18n.t.bind(i18n);
    expect(configErrorText(t, "budgetBelowMinNotional", 218.1, "en")).toContain("218.10");
    // fixed sizes: no budget fixes it, the plain sentence stays
    expect(configErrorText(t, "budgetBelowMinNotional", null, "en")).toBe(strategyErrorText(t, "budgetBelowMinNotional"));
    expect(configErrorText(t, "tpInvalid", 218.1, "en")).toBe(strategyErrorText(t, "tpInvalid"));
  });
});

describe("strategyNoteText", () => {
  it("says which Start check kept a bot stopped after a restart", () => {
    const t = i18n.t.bind(i18n);
    const text = strategyNoteText(t, { key: "resumeRefused", detail: "leverageAboveCeiling|leverage" });
    expect(text).toContain(strategyErrorText(t, "leverageAboveCeiling"));
    expect(text).not.toContain("{{");
  });
});

describe("isLiveView", () => {
  const base: StrategyLiveView = {
    botId: "b",
    enabled: false,
    halted: null,
    realQty: 0,
    entryPrice: 0,
    unrealizedUsdt: 0,
    stopPrice: null,
    lastSyncMs: 0,
    realizedGrossUsdt: 0,
    feesEstUsdt: 0,
    fills: 0,
    pilotCyclesLeft: 0,
    cycleFactor: 1,
    venue: "binance",
  };
  it("is real money when switched on or while a real position is held", () => {
    expect(isLiveView(base)).toBe(false);
    expect(isLiveView({ ...base, enabled: true })).toBe(true);
    expect(isLiveView({ ...base, realQty: -0.01 })).toBe(true);
    expect(isLiveView(null)).toBe(false);
  });
});

describe("run action label", () => {
  const fresh = { acceptingNewCycles: false, cyclesDone: 0, openCycle: null, runState: "stopped" as const };
  it("a bot that never ran says Start, not Resume", () => {
    // New DCA / Grid bots are created stopped (audit 2026-10-08, ux_global 6).
    expect(neverStarted(fresh)).toBe(true);
    expect(runActionKey(fresh)).toBe("strategy.actions.startFirst");
  });
  it("a bot that ran and was paused says Resume", () => {
    expect(runActionKey({ ...fresh, cyclesDone: 3 })).toBe("strategy.actions.start");
    expect(runActionKey({ ...fresh, openCycle: {} as never })).toBe("strategy.actions.start");
  });
  it("an active bot says Pause", () => {
    expect(runActionKey({ ...fresh, acceptingNewCycles: true, runState: "armed" })).toBe("strategy.actions.stop");
  });
});
