import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import type { Preset, StrategyRiskView } from "@/lib/ipc/strategy/strategy";
import { bestTemplate, freeStrategyBudget, scopeState, templateCounts } from "./botNewModel";

const preset = (kind: "dca" | "grid", verdict: string, testMean: number | null, recheck = false, worstDd = -20): Preset =>
  ({
    id: `${kind}-${verdict}-${testMean}`,
    verdict,
    config: { params: { kind } },
    history: {
      recheck: recheck ? { nextRead: "2026-11-01" } : null,
      testWorstBotDrawdownPct: worstDd,
      splits: testMean === null ? [] : [{ split: "test", meanPerBotMonthPct: testMean, ci95LowPct: testMean - 0.5, ci95HighPct: testMean + 0.5 }],
    },
  }) as unknown as Preset;

const PRESETS = [
  preset("dca", "presetReady", 1.98, true, -20.7),
  preset("dca", "presetReady", 1.51, true, -15.8),
  preset("dca", "failed", 3.2),
  preset("dca", "failed", -2.9),
  preset("grid", "failed", 1.22),
  preset("grid", "failed", -2.43),
];

describe("New bot page model", () => {
  it("counts templates by verdict, so 'under review' is not shown as 'failed'", () => {
    expect(templateCounts(PRESETS, "dca")).toEqual({ passed: 0, review: 2, failed: 2, total: 4 });
    expect(templateCounts(PRESETS, "grid")).toEqual({ passed: 0, review: 0, failed: 2, total: 2 });
  });

  it("picks the best non-failed template by TEST mean, never a failed one", () => {
    expect(bestTemplate(PRESETS, "dca")?.history.testWorstBotDrawdownPct).toBe(-20.7);
    // A failed template with a higher TEST mean (3.2) is not the evidence.
    expect(bestTemplate(PRESETS, "grid")).toBeNull();
  });

  it("free budget is the form's formula, floored at zero", () => {
    const r = { balance: 10_000, budgetCapPct: 40, reservedBudget: 1_500 } as StrategyRiskView;
    expect(freeStrategyBudget(r)).toBe(2_500);
    expect(freeStrategyBudget({ ...r, reservedBudget: 9_000 })).toBe(0);
    expect(freeStrategyBudget(null)).toBeNull();
  });

  it("the header readout separates a paper build, a live build with nothing live, and live bots", () => {
    expect(scopeState(false, 0)).toEqual({ kind: "paperBuild" });
    expect(scopeState(false, 3)).toEqual({ kind: "paperBuild" });
    expect(scopeState(true, 0)).toEqual({ kind: "liveAvailable" });
    expect(scopeState(true, 2)).toEqual({ kind: "live", bots: 2 });
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
      const mode = JSON.stringify(d.botCreate.mode) + JSON.stringify(d.nav.scopeTooltip);
      expect(mode).not.toMatch(/Bybit|OKX/);
    });
  }
});
