/**
 * Take-profit target helpers for the bot desk. The server resolves the real
 * exit level per position (src-tauri/src/bot/engine/take_profit.rs); this
 * file only labels targets and computes example distances for the form.
 */

import type { TakeProfitTarget } from "@/lib/ipc/bot/bot";
import type { Signal } from "@/lib/ipc/signal/signal";

/** Bounds for a custom target, in percent — mirror MIN/MAX_CUSTOM_TP_PCT in bot/model.rs. */
export const MIN_CUSTOM_TP_PCT = 0.1;
export const MAX_CUSTOM_TP_PCT = 1000;

/**
 * Average distance of TP1/TP2/TP3 from entry across Sentinel's 349 signals
 * in the 14 days to 2026-09-22 (all carried 3 targets). A FROZEN snapshot,
 * not a live average: the copy names its measurement date
 * (AVERAGE_TP_DISTANCE_DATE). Shown only when no signal is at hand to measure.
 */
export const AVERAGE_TP_DISTANCE_PCT = [3.8, 6.8, 17.0] as const;
/** The day AVERAGE_TP_DISTANCE_PCT was measured. */
export const AVERAGE_TP_DISTANCE_DATE = new Date(Date.UTC(2026, 8, 22));

export type TargetKind = TakeProfitTarget["kind"];
export const TARGET_KINDS: TargetKind[] = ["tp1", "tp2", "tp3", "custom"];

export function customPctValid(pct: number): boolean {
  return Number.isFinite(pct) && pct >= MIN_CUSTOM_TP_PCT && pct <= MAX_CUSTOM_TP_PCT;
}

/** Distances (%) of each published target from the signal's entry, TP1 first. */
export function signalTpDistances(sig: Signal): number[] {
  if (!(sig.entry > 0)) return [];
  return sig.tp
    .filter((v) => Number.isFinite(v) && v > 0)
    .map((v) => (Math.abs(v - sig.entry) / sig.entry) * 100);
}

/**
 * Display label for a position/trade target string ("tp3", "custom:40").
 * `customLabel` is the translated word for "Custom".
 */
export function targetLabel(target: string | null | undefined, customLabel: string): string {
  if (!target) return "TP1";
  if (target.startsWith("custom:")) return `${customLabel} +${target.slice("custom:".length)}%`;
  return target.toUpperCase();
}

