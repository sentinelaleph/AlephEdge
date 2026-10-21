import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import type { Preset, StrategyRiskView } from "@/lib/ipc/strategy/strategy";
import { presetEvidence, provenFirst, provenPreset } from "@/pages/Presets/presetUi";
import { freeStrategyBudget, templateCounts } from "./botNewModel";

const preset = (kind: "dca" | "grid", verdict: string, testMean: number | null, recheck = false, worstDd = -20, id?: string): Preset =>
  ({
    id: id ?? `${kind}-${verdict}-${testMean}`,
    verdict,
    config: { params: { kind } },
    history: {
      recheck: recheck ? { nextRead: "2026-11-01" } : null,
      testWorstBotDrawdownPct: worstDd,
      splits: testMean === null ? [] : [{ split: "test", meanPerBotMonthPct: testMean, ci95LowPct: testMean - 0.5, ci95HighPct: testMean + 0.5 }],
    },
  }) as unknown as Preset;

const PRESETS = [
  preset("dca", "presetReady", 1.51, true, -15.8, "dca_long_safe"),
  preset("dca", "presetReady", 1.98, true, -20.7, "dca_long_classic"),
  preset("dca", "failed", 3.2),
  preset("dca", "failed", -2.9),
  preset("grid", "failed", 1.22),
  preset("grid", "failed", -2.43),
];

describe("New bot page model", () => {
  it("counts the proven templates apart from the experimental ones", () => {
    expect(templateCounts(PRESETS, "dca")).toEqual({ proven: 2, experimental: 2, total: 4 });
    expect(templateCounts(PRESETS, "grid")).toEqual({ proven: 0, experimental: 2, total: 2 });
  });

  it("DCA Long Classic and DCA Long Safe are proven: a passed template with another id, or a higher TEST mean, is experimental", () => {
    expect(PRESETS.map(presetEvidence)).toEqual(["proven", "proven", "experimental", "experimental", "experimental", "experimental"]);
    expect(presetEvidence(preset("dca", "presetReady", 3.5, false, -20, "dca_long_quick"))).toBe("experimental");
    expect(provenPreset(PRESETS, "grid")).toBeNull();
    // A proven id that lost its verdict is not called proven.
    expect(presetEvidence(preset("dca", "failed", 1.9, false, -20, "dca_long_classic"))).toBe("experimental");
    expect(presetEvidence(preset("dca", "failed", 1.4, false, -15, "dca_long_safe"))).toBe("experimental");
  });

  it("Classic stays the recommended template even when Safe comes first in the data", () => {
    expect(provenPreset(PRESETS, "dca")?.id).toBe("dca_long_classic");
    expect(provenPreset(PRESETS)?.id).toBe("dca_long_classic");
    // Without a proven Classic, Safe is the recommended one.
    const noClassic = PRESETS.filter((p) => p.id !== "dca_long_classic");
    expect(provenPreset(noClassic, "dca")?.id).toBe("dca_long_safe");
  });

  it("lists Classic first, then Safe, and keeps the rest in order", () => {
    expect(provenFirst(PRESETS).map((p) => p.id)).toEqual([
      "dca_long_classic",
      "dca_long_safe",
      ...PRESETS.slice(2).map((p) => p.id),
    ]);
  });

  it("free budget is the form's formula, floored at zero", () => {
    const r = { balance: 10_000, budgetCapPct: 40, reservedBudget: 1_500 } as StrategyRiskView;
    expect(freeStrategyBudget(r)).toBe(2_500);
    expect(freeStrategyBudget({ ...r, reservedBudget: 9_000 })).toBe(0);
    expect(freeStrategyBudget(null)).toBeNull();
  });
});

// 0.3.11 said "LIVE opt-in (Futures: Binance, Bybit, OKX)" while only Binance
// passed its test-network run. The venue list now comes from the app
// (status.liveVenues); no locale may name a venue as live in fixed text.
describe("real-money copy names no venue by hand", () => {
  const dir = join(__dirname, "../../i18n/locales");
  for (const lang of readdirSync(dir)) {
    it(lang, () => {
      const d = JSON.parse(readFileSync(join(dir, lang, `${lang}.json`), "utf-8"));
      const mode = JSON.stringify(d.botCreate.mode) + JSON.stringify(d.liveMode);
      expect(mode).not.toMatch(/Bybit|OKX/);
    });
  }
});
