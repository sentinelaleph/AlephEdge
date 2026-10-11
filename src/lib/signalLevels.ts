/**
 * A published signal's levels as moves from its entry: the target and stop
 * distances a trader actually weighs, and where each level sits on one
 * stop-to-last-target scale. Pure; the Signals table and ladder render these.
 */

import type { Signal } from "@/lib/ipc/signal/signal";

/** Signed move from the entry to `level`, % of the entry, favourable positive. */
export function movePct(s: Pick<Signal, "direction" | "entry">, level: number): number {
  if (!(s.entry > 0)) return 0;
  const raw = ((level - s.entry) / s.entry) * 100;
  return s.direction === "short" ? -raw : raw;
}

export interface LadderPoint {
  kind: "sl" | "entry" | "tp";
  /** 1-based target number for kind "tp". */
  n?: number;
  price: number;
  /** Move from the entry, favourable positive. */
  pct: number;
  /** Position on the scale: 0 = stop, 1 = the furthest target. */
  at: number;
}

/** The stop, the entry and every target on one scale; null when the levels cannot form one. */
export function ladder(s: Pick<Signal, "direction" | "entry" | "sl" | "tp">): LadderPoint[] | null {
  const tps = s.tp.filter((x) => Number.isFinite(x) && x > 0);
  if (tps.length === 0 || !(s.entry > 0) || !(s.sl > 0)) return null;
  const lo = movePct(s, s.sl);
  const hi = Math.max(...tps.map((x) => movePct(s, x)));
  if (!(hi > lo)) return null;
  const at = (pct: number) => (pct - lo) / (hi - lo);
  return [
    { kind: "sl", price: s.sl, pct: lo, at: 0 },
    { kind: "entry", price: s.entry, pct: 0, at: at(0) },
    ...tps.map((price, i) => ({ kind: "tp" as const, n: i + 1, price, pct: movePct(s, price), at: at(movePct(s, price)) })),
  ];
}

/** Middle value of the reward:risk of `signals` (null when there are none). */
export function medianRr(signals: Pick<Signal, "rr">[]): number | null {
  const v = signals.map((s) => s.rr).filter((x) => Number.isFinite(x)).sort((a, b) => a - b);
  if (v.length === 0) return null;
  const m = Math.floor(v.length / 2);
  return v.length % 2 ? v[m] : (v[m - 1] + v[m]) / 2;
}
