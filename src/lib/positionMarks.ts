/**
 * Open signal positions at their last engine price (BotDeskStatus.marks):
 * the per-row lookup, the per-bot sums, and where the price sits between the
 * stop and the target. Pure; the tables and the Dashboard only render these.
 */

import type { BotDeskStatus, BotKind, OpenPosition, PositionMark } from "@/lib/ipc/bot/bot";

/** A price older than this is shown as stale (the engine ticks every 4 s). */
export const MARK_STALE_MS = 60_000;

export const markKey = (kind: BotKind, signalId: string) => `${kind}:${signalId}`;

export function marksByPosition(status: Pick<BotDeskStatus, "marks">): Map<string, PositionMark> {
  return new Map((status.marks ?? []).map((m) => [markKey(m.botKind, m.signalId), m]));
}

export interface OpenPnl {
  /** Open positions. */
  open: number;
  /** Of those, how many have a price yet. */
  priced: number;
  /** Sum of the priced positions' P&L if closed now, USDT. */
  pnlUsdt: number;
  /** Capital of the priced positions, USDT. */
  capital: number;
}

/** Open P&L of the positions in `positions` (one bot's, or all). */
export function openPnl(positions: OpenPosition[], marks: Map<string, PositionMark>): OpenPnl {
  let priced = 0;
  let pnlUsdt = 0;
  let capital = 0;
  for (const p of positions) {
    const m = marks.get(markKey(p.botKind, p.signalId));
    if (!m) continue;
    priced += 1;
    pnlUsdt += m.pnlUsdt;
    capital += p.capital;
  }
  return { open: positions.length, priced, pnlUsdt, capital };
}

/**
 * Where `price` sits on the way from the stop (0) to the target (1), and
 * where the entry sits; null when the levels cannot form a range.
 */
export function rangePosition(p: Pick<OpenPosition, "direction" | "entry" | "sl" | "tp">, price: number): { price: number; entry: number } | null {
  const span = p.direction === "short" ? p.sl - p.tp : p.tp - p.sl;
  if (!(span > 0) || !Number.isFinite(price)) return null;
  const at = (x: number) => Math.min(1, Math.max(0, (p.direction === "short" ? p.sl - x : x - p.sl) / span));
  return { price: at(price), entry: at(p.entry) };
}
