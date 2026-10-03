/**
 * Backtest IPC bridge (mirrors src-tauri/src/bot/strategy/backtest*.rs and
 * src-tauri/src/store/backtest.rs). A run replays a DCA / Grid config over
 * DataHub candles (Sentinel API, signed-in session) through the paper
 * engine's own bar driver. The UI never fetches candles itself.
 *
 * Errors are codes: `backtest.errors.<code>` first, then `strategy.errors.*`
 * (validation), optionally "code|detail".
 */

import { devMock, inTauri, invoke, listenEvent, type Unlisten } from "../bridge";
import type { MarketKind, StrategyConfig } from "./strategy";

export type BacktestInterval = "15m" | "1h" | "4h" | "1d";
export const BACKTEST_INTERVALS: BacktestInterval[] = ["15m", "1h", "4h", "1d"];

export interface BacktestCycle {
  seq: number;
  openedAt: number;
  closedAt: number;
  exit: string;
  anchorPrice: number;
  avgEntry: number | null;
  soFilled: number | null;
  gridClosingFills: number | null;
  fills: number;
  feesQuote: number;
  fundingQuote: number;
  pnlQuote: number;
  pnlPctBudget: number;
  maxAdversePct: number;
  /** Still open at the last bar; closed there at market (exit "end"). */
  openAtEnd: boolean;
}

export interface BacktestResult {
  budget: number;
  bars: number;
  firstBarMs: number;
  lastBarMs: number;
  closedCycles: number;
  wins: number;
  losses: number;
  closedPnlQuote: number;
  openAtEnd: BacktestCycle | null;
  openAtEndPnlQuote: number;
  totalPnlQuote: number;
  totalPnlPct: number;
  maxDrawdownQuote: number;
  maxDrawdownPct: number;
  longestUnderwaterMs: number;
  deepestSo: number | null;
  gridClosingFills: number | null;
  totalFills: number;
  liquidations: number;
  feesQuote: number;
  fundingQuote: number;
  exits: Record<string, number>;
  endState: string;
  deadReason: string | null;
  /** strategy.notes.* keys and how often they fired. */
  notes: Record<string, number>;
  utilisationInPosition: number;
  utilisationCommitted: number;
  equity: { ts: number; equity: number }[];
  cycles: BacktestCycle[];
  cyclesTruncated: boolean;
}

export interface BacktestData {
  source: string;
  candleCount: number;
  expectedCandles: number | null;
  coveragePct: number | null;
  missingBars: number;
  droppedCandles: number;
  requests: number;
  /** Always false: the history source has no funding rates. */
  fundingIncluded: boolean;
}

export interface BacktestRun {
  id: string;
  createdAt: number;
  config: StrategyConfig;
  symbol: string;
  market: MarketKind;
  interval: BacktestInterval;
  startMs: number;
  endMs: number;
  data: BacktestData;
  result: BacktestResult;
}

export interface BacktestRunSummary {
  id: string;
  createdAt: number;
  kind: "dca" | "grid";
  name: string;
  symbol: string;
  market: MarketKind;
  interval: BacktestInterval;
  startMs: number;
  endMs: number;
  totalPnlPct: number;
  maxDrawdownPct: number;
  closedCycles: number;
  openAtEnd: boolean;
  coveragePct: number | null;
}

export interface BacktestProgress {
  /** The token the run was started with (echoed by Rust; "" when none). */
  runToken: string;
  done: number;
  total: number;
}

type Mock = typeof import("./backtest.mock");
const mock = <T>(run: (m: Mock) => Promise<T>) => devMock(() => import("./backtest.mock"), run);

function call<T>(cmd: string, args: Record<string, unknown> | undefined, run: (m: Mock) => Promise<T>): Promise<T> {
  return inTauri() ? invoke<T>(cmd, args) : mock(run);
}

export interface BacktestRunArgs {
  config: StrategyConfig;
  symbol: string;
  market: MarketKind;
  interval: BacktestInterval;
  startMs: number;
  endMs: number;
  /** Echoed on this run's progress events (`newBacktestRunToken`). */
  runToken: string;
}

export const backtestRun = (a: BacktestRunArgs) =>
  call<BacktestRun>(
    "backtest_run",
    {
      config: a.config,
      symbol: a.symbol,
      market: a.market,
      interval: a.interval,
      startMs: a.startMs,
      endMs: a.endMs,
      runToken: a.runToken,
    },
    (m) => m.run(),
  );

/** A fresh run token ([A-Za-z0-9-], what Rust keeps of it). */
export function newBacktestRunToken(): string {
  const c = globalThis.crypto;
  if (c && typeof c.randomUUID === "function") return `rt-${c.randomUUID()}`;
  return `rt-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 12)}`;
}

/** Wraps `handler` so it only sees the progress of the run with `runToken`. */
export function progressOf(runToken: string, handler: (p: BacktestProgress) => void): (p: BacktestProgress) => void {
  return (p) => {
    if (p && p.runToken === runToken) handler(p);
  };
}

export const backtestList = () => call<BacktestRunSummary[]>("backtest_list", undefined, (m) => m.list());

export const backtestGet = (id: string) => call<BacktestRun>("backtest_get", { id }, (m) => m.get(id));

export const backtestDelete = (id: string) => call<void>("backtest_delete", { id }, (m) => m.remove(id));

/** Empties the run list; resolves to the number removed. */
export const backtestDeleteAll = () => call<number>("backtest_delete_all", undefined, (m) => m.removeAll());

/**
 * Page progress (requests done / total) of the ONE run started with
 * `runToken`; events of any other run in flight are ignored.
 */
export const onBacktestProgress = (runToken: string, handler: (p: BacktestProgress) => void): Promise<Unlisten> =>
  listenEvent<BacktestProgress>("backtest:progress", progressOf(runToken, handler));

/** A backtest run id ("bt_" + 12 base32 characters). */
export function isBacktestRunId(id: string): boolean {
  return /^bt_[a-z2-7]{12}$/.test(id);
}
