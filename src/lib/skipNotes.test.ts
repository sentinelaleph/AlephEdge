import { describe, expect, it } from "vitest";
import i18n from "@/i18n";
import { minimumOrderNotice, skipParams } from "@/lib/skipNotes";

describe("skip note details are localised when they are error codes", () => {
  it("an error code detail becomes the error sentence", async () => {
    await i18n.changeLanguage("en");
    const d = skipParams({ atMs: 0, symbol: "BTCUSDT", reason: "storeWriteFailed", detail: "storeWriteFailed" }).detail;
    expect(d).not.toBe("storeWriteFailed");
    expect(d.length).toBeGreaterThan(0);
  });
  it("plain text passes through unchanged", () => {
    expect(skipParams({ atMs: 0, symbol: "X", reason: "frAgainst", detail: "+0.081%" }).detail).toBe("+0.081%");
  });
  it("vetoClosed keeps its count|detail packing", () => {
    expect(skipParams({ atMs: 0, symbol: "X", reason: "vetoClosed", detail: "3|BTC turned bearish" })).toEqual({ count: "3", detail: "BTC turned bearish" });
  });
});

describe("Sentinel veto codes are shown in the user's language", () => {
  it("a veto code becomes the localised reason, in tr too", async () => {
    await i18n.changeLanguage("en");
    const en = skipParams({ atMs: 0, symbol: "X", reason: "vetoedByRegime", detail: "btc_regime_turn_bearish" }).detail;
    expect(en).not.toBe("btc_regime_turn_bearish");
    await i18n.changeLanguage("tr");
    const tr = skipParams({ atMs: 0, symbol: "X", reason: "vetoClosed", detail: "2|btc_regime_turn_bearish" });
    expect(tr.count).toBe("2");
    expect(tr.detail).not.toBe(en);
    await i18n.changeLanguage("en");
  });
});

describe("a refusal under the exchange minimum is said in plain words", () => {
  const note = (atMs: number, symbol: string, reason: string, detail?: string) => ({ atMs, symbol, reason, detail });
  it("counts the refusals and names the newest order size", async () => {
    await i18n.changeLanguage("en");
    const n = minimumOrderNotice([
      note(1, "ETHUSDT", "belowExchangeMinimum", "2.80"),
      note(3, "SOLUSDT", "belowExchangeMinimum", "2.10"),
      note(2, "XRPUSDT", "tooOld"),
    ]);
    expect(n).toEqual({ count: 2, orderUsdt: "2.10", symbol: "SOLUSDT" });
    expect(i18n.t("bots.skipReasons.belowExchangeMinimum", { detail: "2.10" })).toBe(
      "Order would be 2.10 USDT, under the exchange minimum of 5 USDT",
    );
  });
  it("nothing to say when no signal was refused for it", () => {
    expect(minimumOrderNotice([note(1, "X", "tooOld")])).toBeNull();
    expect(minimumOrderNotice([])).toBeNull();
  });
});
