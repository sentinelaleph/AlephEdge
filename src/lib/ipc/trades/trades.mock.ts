/**
 * DEV-ONLY browser preview for the trade history (no Rust in `npm run dev`).
 * Reached through `devMock`, so the released bundle drops this module. The
 * preview has no store behind it, so the history is empty: no sample trade,
 * P&L or win rate is invented.
 */

import type { PnlStats, TradeRecord } from "./trades";

const EMPTY: PnlStats = {
  totalTrades: 0,
  wins: 0,
  losses: 0,
  winRate: null,
  netPnlQuote: 0,
  avgPnlPct: 0,
  profitFactor: null,
  maxDrawdownQuote: 0,
  todayPnlQuote: 0,
};

export const mock = {
  list: (): Promise<TradeRecord[]> => Promise.resolve([]),
  stats: (): Promise<PnlStats> => Promise.resolve({ ...EMPTY }),
  exportCsv: (): Promise<string> => Promise.reject("No trade store in the browser preview."),
};
