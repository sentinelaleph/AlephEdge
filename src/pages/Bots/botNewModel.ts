import type { Preset, StrategyKind, StrategyRiskView } from "@/lib/ipc/strategy/strategy";
import { presetEvidence } from "@/pages/Presets/presetUi";

export interface TemplateCounts {
  proven: number;
  experimental: number;
  total: number;
}

/** The templates of one bot type: the proven one, and every other (experimental). */
export function templateCounts(presets: Preset[], kind: StrategyKind): TemplateCounts {
  const of = presets.filter((p) => p.config.params.kind === kind);
  const proven = of.filter((p) => presetEvidence(p) === "proven").length;
  return { proven, experimental: of.length - proven, total: of.length };
}

/** Strategy budget still free under the risk level's cap, the form's own formula for a new bot. */
export function freeStrategyBudget(r: StrategyRiskView | null): number | null {
  if (!r) return null;
  return Math.max(0, (r.balance * r.budgetCapPct) / 100 - r.reservedBudget);
}
