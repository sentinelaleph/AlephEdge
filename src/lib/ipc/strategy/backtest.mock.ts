/**
 * DEV-ONLY browser preview for the backtest IPC (no Rust, no DataHub in
 * `npm run dev`). Reached through `devMock`, so the released bundle drops
 * this module. There is no history source in a browser preview, so a run
 * fails honestly and the list stays empty: no sample run is invented. Only
 * the screenshot harness seeds runs (labelled "Sample data" there).
 */

import type { BacktestRun, BacktestRunSummary } from "./backtest";

const runs = new Map<string, BacktestRun>();

/** Screenshot harness only. */
export function seed(entries: BacktestRun[]) {
  for (const r of entries) runs.set(r.id, r);
}

function summary(r: BacktestRun): BacktestRunSummary {
  return {
    id: r.id,
    createdAt: r.createdAt,
    kind: r.config.params.kind,
    name: r.config.name,
    symbol: r.symbol,
    market: r.market,
    interval: r.interval,
    startMs: r.startMs,
    endMs: r.endMs,
    totalPnlPct: r.result.totalPnlPct,
    maxDrawdownPct: r.result.maxDrawdownPct,
    closedCycles: r.result.closedCycles,
    openAtEnd: r.result.openAtEnd !== null,
    coveragePct: r.data.coveragePct,
  };
}

export const run = (): Promise<BacktestRun> => Promise.reject("historyNotAvailable");
export const list = (): Promise<BacktestRunSummary[]> => Promise.resolve([...runs.values()].map(summary));
export const get = (id: string): Promise<BacktestRun> => {
  const r = runs.get(id);
  return r ? Promise.resolve(r) : Promise.reject("runNotFound");
};
export const remove = (id: string): Promise<void> => (runs.delete(id) ? Promise.resolve() : Promise.reject("runNotFound"));
export const removeAll = (): Promise<number> => {
  const n = runs.size;
  runs.clear();
  return Promise.resolve(n);
};
