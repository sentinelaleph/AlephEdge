/**
 * History & stats over a list of closed signal trades. The totals follow
 * Rust's `trades::aggregate` rule for rule (wins = exits at the target,
 * losses = exits at the stop, profit factor over gross gains and losses,
 * drawdown on the running total), so a filter over everything gives the
 * same figures as the stats Rust reports.
 */

import type { PnlStats, TradeRecord } from "@/lib/ipc/trades/trades";

export const DAY_MS = 86_400_000;

export function utcDayStart(ms: number): number {
  const d = new Date(ms);
  return Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate());
}

const byClose = (a: TradeRecord, b: TradeRecord) => a.closedAt - b.closedAt;

export interface TradeStats extends PnlStats {
  /** Mean net P&L per trade, USDT. */
  avgPnlQuote: number;
  best: TradeRecord | null;
  worst: TradeRecord | null;
}

export function aggregateTrades(trades: TradeRecord[], dayStartMs: number): TradeStats {
  const rows = [...trades].sort(byClose);
  let wins = 0;
  let losses = 0;
  let grossProfit = 0;
  let grossLoss = 0;
  let net = 0;
  let today = 0;
  let sumPct = 0;
  let peak = 0;
  let maxDd = 0;
  let best: TradeRecord | null = null;
  let worst: TradeRecord | null = null;
  for (const t of rows) {
    net += t.pnlQuote;
    sumPct += t.pnlPct;
    if (t.closedAt >= dayStartMs) today += t.pnlQuote;
    if (t.exitReason === "tp") wins += 1;
    else if (t.exitReason === "sl") losses += 1;
    if (t.pnlQuote >= 0) grossProfit += t.pnlQuote;
    else grossLoss += -t.pnlQuote;
    peak = Math.max(peak, net);
    maxDd = Math.max(maxDd, peak - net);
    if (!best || t.pnlQuote > best.pnlQuote) best = t;
    if (!worst || t.pnlQuote < worst.pnlQuote) worst = t;
  }
  const resolved = wins + losses;
  return {
    totalTrades: rows.length,
    wins,
    losses,
    winRate: resolved > 0 ? wins / resolved : null,
    netPnlQuote: net,
    avgPnlPct: rows.length ? sumPct / rows.length : 0,
    profitFactor: grossLoss > 0 && grossProfit > 0 ? grossProfit / grossLoss : null,
    maxDrawdownQuote: maxDd,
    todayPnlQuote: today,
    avgPnlQuote: rows.length ? net / rows.length : 0,
    best,
    worst,
  };
}

/** Running net P&L after each trade, oldest first, starting from 0 before the first. */
export function cumulative(trades: TradeRecord[]): { ts: number; value: number }[] {
  const rows = [...trades].sort(byClose);
  if (rows.length === 0) return [];
  const out = [{ ts: rows[0].closedAt, value: 0 }];
  let run = 0;
  for (const t of rows) {
    run += t.pnlQuote;
    out.push({ ts: t.closedAt, value: run });
  }
  return out;
}

export interface DayNet {
  /** UTC day start, ms. */
  day: number;
  net: number;
  trades: number;
}

/** Net P&L per UTC day, every day from the first trade's to the last's (days without trades are 0). */
export function dailyNet(trades: TradeRecord[]): DayNet[] {
  if (trades.length === 0) return [];
  const m = new Map<number, DayNet>();
  for (const t of trades) {
    const day = utcDayStart(t.closedAt);
    const d = m.get(day) ?? { day, net: 0, trades: 0 };
    d.net += t.pnlQuote;
    d.trades += 1;
    m.set(day, d);
  }
  const days = [...m.keys()];
  const first = Math.min(...days);
  const last = Math.max(...days);
  const out: DayNet[] = [];
  for (let d = first; d <= last; d += DAY_MS) out.push(m.get(d) ?? { day: d, net: 0, trades: 0 });
  return out;
}

export interface Bucket {
  key: string;
  trades: number;
  wins: number;
  losses: number;
  net: number;
  avgPct: number;
}

/** Trades grouped by `key`, the largest net result (gain or loss) first. */
export function breakdown(trades: TradeRecord[], key: (t: TradeRecord) => string): Bucket[] {
  const m = new Map<string, Bucket & { sumPct: number }>();
  for (const t of trades) {
    const k = key(t);
    const b = m.get(k) ?? { key: k, trades: 0, wins: 0, losses: 0, net: 0, avgPct: 0, sumPct: 0 };
    b.trades += 1;
    b.net += t.pnlQuote;
    b.sumPct += t.pnlPct;
    if (t.exitReason === "tp") b.wins += 1;
    else if (t.exitReason === "sl") b.losses += 1;
    m.set(k, b);
  }
  return [...m.values()]
    .map(({ sumPct, ...b }) => ({ ...b, avgPct: b.trades ? sumPct / b.trades : 0 }))
    .sort((a, b) => Math.abs(b.net) - Math.abs(a.net) || b.trades - a.trades);
}

export interface Streaks {
  /** The run the latest trades are on: gains (> 0) or losses (< 0); flat trades end a run. */
  current: { kind: "gain" | "loss"; count: number } | null;
  longestGain: number;
  longestLoss: number;
}

export function streaks(trades: TradeRecord[]): Streaks {
  let run: { kind: "gain" | "loss"; count: number } | null = null;
  let longestGain = 0;
  let longestLoss = 0;
  for (const t of [...trades].sort(byClose)) {
    const kind = t.pnlQuote > 0 ? "gain" : t.pnlQuote < 0 ? "loss" : null;
    run = kind === null ? null : run && run.kind === kind ? { kind, count: run.count + 1 } : { kind, count: 1 };
    if (run?.kind === "gain") longestGain = Math.max(longestGain, run.count);
    if (run?.kind === "loss") longestLoss = Math.max(longestLoss, run.count);
  }
  return { current: run, longestGain, longestLoss };
}
