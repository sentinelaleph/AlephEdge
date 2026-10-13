import type { Preset, StrategyKind, StrategyRiskView } from "@/lib/ipc/strategy/strategy";
import { testSplit, verdictBucket } from "@/pages/Presets/presetUi";

export interface TemplateCounts {
  passed: number;
  review: number;
  failed: number;
  total: number;
}

/** The templates of one bot type, by verdict bucket (passed / under review / failed). */
export function templateCounts(presets: Preset[], kind: StrategyKind): TemplateCounts {
  const of = presets.filter((p) => p.config.params.kind === kind);
  const n = (b: string) => of.filter((p) => verdictBucket(p) === b).length;
  return { passed: n("passed"), review: n("review"), failed: n("failed"), total: of.length };
}

/**
 * The best template of a type that did not fail its historical test, by the
 * TEST split's mean per bot-month; null when every template failed.
 */
export function bestTemplate(presets: Preset[], kind: StrategyKind): Preset | null {
  const ok = presets.filter((p) => p.config.params.kind === kind && verdictBucket(p) !== "failed" && testSplit(p));
  if (ok.length === 0) return null;
  return ok.reduce((a, b) => ((testSplit(b)?.meanPerBotMonthPct ?? -Infinity) > (testSplit(a)?.meanPerBotMonthPct ?? -Infinity) ? b : a));
}

/** Strategy budget still free under the risk level's cap, the form's own formula for a new bot. */
export function freeStrategyBudget(r: StrategyRiskView | null): number | null {
  if (!r) return null;
  return Math.max(0, (r.balance * r.budgetCapPct) / 100 - r.reservedBudget);
}

/** What the header's mode chip says: a paper-only build, a live build with nothing live, or bots on real money. */
export type ScopeState = { kind: "paperBuild" } | { kind: "liveAvailable" } | { kind: "live"; bots: number };

export function scopeState(liveBuild: boolean, liveBots: number): ScopeState {
  if (!liveBuild) return { kind: "paperBuild" };
  return liveBots > 0 ? { kind: "live", bots: liveBots } : { kind: "liveAvailable" };
}
