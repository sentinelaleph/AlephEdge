/**
 * Pure readers over the portfolio risk view snapshot (src-tauri/src/riskview).
 * Nothing is computed here that Rust did not serve: these pick, order and
 * label what the snapshot already holds.
 */

import type { BookStress, Check, CheckLevel, RiskViewSnapshot, Scenario } from "@/lib/ipc/riskview/riskview";

export type BookId = "paper" | "real";

/** The scenario for one BTC move, if the book has it. */
export function scenarioAt(book: BookStress, btcMovePct: number): Scenario | null {
  return book.scenarios.find((s) => s.btcMovePct === btcMovePct) ?? null;
}

/** Books worth showing: paper always when it has exposures, real when it has any. */
export function activeBooks(s: RiskViewSnapshot): BookId[] {
  const out: BookId[] = [];
  if (s.paper.exposures > 0) out.push("paper");
  if (s.real.exposures > 0) out.push("real");
  return out;
}

/** Danger checks of both books (what raises an in-app alert), real money first. */
export function crossedChecks(s: RiskViewSnapshot | null): { book: BookId; check: Check }[] {
  if (!s) return [];
  const pick = (book: BookId, b: BookStress) => b.checks.filter((c) => c.level === "danger").map((check) => ({ book, check }));
  return [...pick("real", s.real), ...pick("paper", s.paper)];
}

/** The worst level across both books, or null when nothing was checked. */
export function worstLevel(s: RiskViewSnapshot | null): CheckLevel | null {
  if (!s) return null;
  const all = [...s.real.checks, ...s.paper.checks];
  if (all.length === 0) return null;
  const rank: CheckLevel[] = ["danger", "warn", "ok", "unavailable"];
  return rank.find((l) => all.some((c) => c.level === l)) ?? null;
}

/** The loss at a scenario as a positive share of capital, or null. */
export function lossPct(sc: Scenario | null): number | null {
  const p = sc?.book.pctOfCapital;
  return p === null || p === undefined ? null : Math.max(0, -p);
}
