import { describe, expect, it } from "vitest";
import type { LiveTemplate } from "@/lib/ipc/strategy/presetsLive";
import type { Preset } from "@/lib/ipc/strategy/strategy";
import { pairListText, presetPairCheck } from "./presetPair";

const preset = (rule: string) => ({ id: "dca_long_classic", universe: { rule } }) as unknown as Preset;
const live = (symbols: string[], universeMonth: string | null = "2026-10") => ({ id: "dca_long_classic", symbols, universeMonth }) as unknown as LiveTemplate;
const majors = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "ZECUSDT", "XRPUSDT"];

describe("presetPairCheck", () => {
  it("names a pair on this month's list and one outside it", () => {
    expect(presetPairCheck(preset("topUsdtPerpsByQuoteVolume"), "ethusdt ", live(majors))).toEqual({ state: "inList", month: "2026-10", list: majors });
    expect(presetPairCheck(preset("topUsdtPerpsByQuoteVolume"), "PEPEUSDT", live(majors))).toMatchObject({ state: "outside", month: "2026-10" });
  });

  it("says nothing it cannot know", () => {
    const p = preset("topUsdtPerpsByQuoteVolume");
    expect(presetPairCheck(p, "PEPEUSDT", null).state).toBe("unknown");
    expect(presetPairCheck(p, "PEPEUSDT", live([])).state).toBe("unknown");
    expect(presetPairCheck(p, "PEPEUSDT", live(majors, null)).state).toBe("unknown");
    expect(presetPairCheck(p, "", live(majors)).state).toBe("unknown");
    // BTC-only templates keep their own exact check.
    expect(presetPairCheck(preset("btcOnly"), "ETHUSDT", live(["BTCUSDT"])).state).toBe("unknown");
  });

  it("lists pairs the way people say them", () => {
    expect(pairListText(["BTCUSDT", "1000PEPEUSDT"])).toBe("BTC, 1000PEPE");
  });
});
