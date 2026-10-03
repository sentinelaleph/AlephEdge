import { describe, expect, it } from "vitest";
import i18n from "@/i18n";
import { skipParams } from "@/lib/skipNotes";

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
