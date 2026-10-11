/**
 * DEV-ONLY browser preview for the trade history (no Rust in `npm run dev`).
 * Reached through `devMock`, so the released bundle drops this module. The
 * preview has no store behind it, so the history is empty (no sample trade,
 * P&L or win rate is invented) unless the screenshot harness hands it its
 * labelled sample history.
 */

import type { BotStats, PnlStats, TradeRecord } from "./trades";

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

let sample: TradeRecord[] = [];

/** Dev screenshot harness only: the sample history its History screen lists. */
export function setSampleTrades(rows: TradeRecord[]): void {
  sample = rows;
}

export const mock = {
  list: (): Promise<TradeRecord[]> => Promise.resolve([...sample]),
  stats: (): Promise<PnlStats> => Promise.resolve({ ...EMPTY }),
  statsByBot: (): Promise<BotStats[]> => Promise.resolve([]),
  exportCsv: (): Promise<string> => Promise.reject("No trade store in the browser preview."),
};
