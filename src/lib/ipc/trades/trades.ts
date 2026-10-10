/**
 * Trade-history IPC bridge (mirrors src-tauri/src/store). All PnL numbers are
 * NET (fees included) — the honesty contract. `npm run dev` gets an empty,
 * dev-only history through `devMock`; the released bundle has none.
 */

import { devMock, inTauri, invoke } from "../bridge";
import type { SizingMode } from "../bot/bot";

export interface TradeRecord {
  id: number;
  signalId: string;
  botKind: string;
  exchangeId: string;
  symbol: string;
  direction: string;
  entry: number;
  exit: number;
  leverage: number;
  capital: number;
  pnlPct: number;
  pnlQuote: number;
  /**
   * Unlevered net %, after the same 0.10% fees and 0.02% funding the
   * Sentinel web ledger applies — directly comparable to a ribqa.com figure.
   * Null when the engine hasn't computed it (older rows).
   */
  unleveredNetPct: number | null;
  exitReason: string;
  frAtOpen?: number | null;
  ldAtOpen?: number | null;
  openedAt: number;
  closedAt: number;
  /** True when this trade held real exchange exposure. */
  live: boolean;
  /** LIVE only: volume-weighted entry from the exchange's fills. */
  fillEntry: number | null;
  /** LIVE only: volume-weighted exit over every closing fill (partials included). */
  fillExit: number | null;
  /** LIVE only: total commission paid in USDT. Null when unknown (fills
   *  unavailable, or a fee paid in a non-USDT asset). */
  commissionUsdt: number | null;
  /** "fixed" | "risk" (legacy rows: "fixed"). */
  sizingMode: SizingMode;
  /** Declared risk % of capital at the stop (risk mode only). */
  riskPct: number | null;
  notionalUsdt: number | null;
  effectiveLeverage: number | null;
  riskCapped: boolean;
  /** NET PnL in USDT (account-scale — same value as `pnlQuote`, named for what it is). */
  pnlUsdt: number;
  /** NET PnL as % of the bot's capital (account-scale — same value as `pnlPct`). */
  pnlPctOfCapital: number;
  /** Sentinel's veto reason code, set only when `exitReason === "veto"` (2026-09-18). */
  vetoReasonCode: string | null;
  /** Sentinel's human-readable veto reason — shown verbatim in the trade row tooltip. */
  vetoReasonText: string | null;
  /** Target the trade exited against: "tp1" | "tp2" | "tp3" | "custom:40". Null on rows from before targets were selectable (all TP1). */
  tpTarget: string | null;
  /** The configured target when the signal lacked it and a lower one was used. */
  tpFallbackFrom: string | null;
  /** Opened by hand on one signal (Execute), not by the bot's loop. Absent = false. */
  manual?: boolean;
}

export interface PnlStats {
  totalTrades: number;
  wins: number;
  losses: number;
  winRate: number | null;
  netPnlQuote: number;
  avgPnlPct: number;
  profitFactor: number | null;
  maxDrawdownQuote: number;
  todayPnlQuote: number;
}

export function tradesList(limit = 50): Promise<TradeRecord[]> {
  return inTauri() ? invoke<TradeRecord[]>("trades_list", { limit }) : devMock(() => import("./trades.mock"), (m) => m.mock.list());
}

/**
 * `live`: true = real-money trades only, false = simulated only, omitted =
 * both (only meaningful in a simulation-only build, where every trade is
 * simulated). A live build must always ask per scope: real and simulated
 * money are never summed.
 */
export function tradesStats(live?: boolean): Promise<PnlStats> {
  return inTauri()
    ? invoke<PnlStats>("trades_stats", live === undefined ? {} : { live })
    : devMock(() => import("./trades.mock"), (m) => m.mock.stats());
}

/** Exports all trades as CSV; resolves to the written file's path. */
export function tradesExportCsv(): Promise<string> {
  return inTauri() ? invoke<string>("trades_export_csv") : devMock(() => import("./trades.mock"), (m) => m.mock.exportCsv());
}

/** Exports the signal skip log (signals a bot saw and did not open) as CSV. */
export function skipsExportCsv(): Promise<string> {
  return inTauri() ? invoke<string>("skips_export_csv") : devMock(() => import("./trades.mock"), (m) => m.mock.exportCsv());
}

/** True when a closed trade held real exchange exposure. */
export function isLiveTrade(trade: TradeRecord): boolean {
  return trade.live;
}
