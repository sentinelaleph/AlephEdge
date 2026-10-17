/** Pure rules of the Assistant page (unit-tested). */

import { formatNumber } from "@/lib/format";
import type { BotDraft, ModelInfo, ToolUse, Usage } from "@/lib/ipc/assistant/assistant";

type T = (key: string, opts?: Record<string, unknown>) => string;

/** The message's cost exactly as OpenRouter reported it, or that it did not. */
export function costText(t: T, usage: Usage | null, locale: string): string {
  if (!usage || usage.costUsd === null || !Number.isFinite(usage.costUsd)) return t("assistant.costNotReported");
  const digits = usage.costUsd < 0.01 ? 5 : 4;
  return t("assistant.cost", { cost: formatNumber(usage.costUsd, locale, { minimumFractionDigits: 2, maximumFractionDigits: digits }) });
}

/** Tools the answer read, and calls that were refused (with their code). */
export function toolSummary(tools: ToolUse[]): { read: string[]; refused: string[] } {
  const read: string[] = [];
  const refused: string[] = [];
  for (const u of tools) {
    if (u.outcome === "ok" || u.outcome === "draft") {
      if (!read.includes(u.name)) read.push(u.name);
    } else {
      refused.push(`${u.name || "?"} (${u.outcome})`);
    }
  }
  return { read, refused };
}

export function draftLabelKey(botType: BotDraft["botType"]): string {
  return `assistant.draft.types.${botType}`;
}

/** Models matching the search (id or name), the chosen one always kept; at most 200 listed. */
export function filterModels(models: ModelInfo[], query: string, chosen: string | null): ModelInfo[] {
  const q = query.trim().toLowerCase();
  const hit = q ? models.filter((m) => m.id.toLowerCase().includes(q) || m.name.toLowerCase().includes(q)) : models;
  const out = hit.slice(0, 200);
  if (chosen && !out.some((m) => m.id === chosen)) {
    const c = models.find((m) => m.id === chosen);
    if (c) out.unshift(c);
  }
  return out;
}
