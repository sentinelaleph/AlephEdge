/**
 * Risk & safety readouts: what the open positions would book if every stop
 * filled, how much room the daily stop and the DCA / Grid limits have left.
 * Pure; the page renders these. The per-position figures come from Rust
 * (BotDeskStatus.atStop, the trade record's own formula).
 */

import type { OpenPosition, PositionAtStop } from "@/lib/ipc/bot/bot";
import type { StrategyRiskView } from "@/lib/ipc/strategy/strategy";
import { markKey } from "./positionMarks";

export interface StopRisk {
  positions: number;
  /** Sum of every position's result at its current stop (a loss is negative). */
  atStops: number;
  capital: number;
}

export function stopRisk(positions: OpenPosition[], atStop: PositionAtStop[] | undefined): StopRisk {
  const by = new Map((atStop ?? []).map((a) => [markKey(a.botKind, a.signalId), a.pnlUsdt]));
  let atStops = 0;
  for (const p of positions) atStops += by.get(markKey(p.botKind, p.signalId)) ?? 0;
  return { positions: positions.length, atStops, capital: positions.reduce((n, p) => n + p.capital, 0) };
}

export interface DailyRoom {
  limit: number;
  /** Today's realized loss (>= 0; a gain uses none of the limit). */
  used: number;
  /**
   * What can still be lost before the stop trips. The engine trips when
   * today's realized NET result reaches -limit (engine/mod.rs), so a gain
   * made today adds to the room.
   */
  left: number;
}

/** The daily stop: `pct` of the balance, against today's realized net result. */
export function dailyRoom(balance: number, pct: number, today: number): DailyRoom {
  const limit = (balance * pct) / 100;
  return { limit, used: Math.max(0, -today), left: Math.max(0, limit + today) };
}

/** True when every stop filling would take the realized loss to the daily stop. */
export function stopsWouldTrip(atStops: number, room: DailyRoom): boolean {
  return atStops < 0 && -atStops >= room.left;
}

export interface StrategyRoom {
  cap: number;
  reserved: number;
  free: number;
  /** Drawdown from the P&L peak of the bots inside the breaker. */
  ddUsed: number;
  ddLimit: number;
  ddLeft: number;
}

export function strategyRoom(r: StrategyRiskView): StrategyRoom {
  const cap = (r.balance * r.budgetCapPct) / 100;
  const ddUsed = Math.max(0, r.peakPnlQuote - r.pnlQuote);
  const ddLimit = (r.breakerBudget * r.portfolioDdPct) / 100;
  return { cap, reserved: r.reservedBudget, free: Math.max(0, cap - r.reservedBudget), ddUsed, ddLimit, ddLeft: Math.max(0, ddLimit - ddUsed) };
}
