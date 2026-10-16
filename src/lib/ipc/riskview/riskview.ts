/**
 * Portfolio risk view IPC bridge (mirrors src-tauri/src/riskview): stress
 * test of every open exposure under BTC shocks, crash replay, and the
 * what-if before a bot or trade starts. Every figure is computed in Rust
 * from DataHub candles; a value Rust could not compute arrives as null.
 *
 * In the browser preview the mock answers with sample data, and every
 * response carries `sample: true` so the page labels it as such.
 */

import { devMock, inTauri, invoke } from "../bridge";
import type { BotKind } from "../bot/bot";
import type { StrategyConfig } from "../strategy/strategy";

type Mock = typeof import("./riskview.mock");
const mock = <T>(run: (m: Mock) => Promise<T>) => devMock(() => import("./riskview.mock"), run);

function call<T>(cmd: string, args: Record<string, unknown> | undefined, run: (m: Mock) => Promise<T>): Promise<T> {
  return inTauri() ? invoke<T>(cmd, args) : mock(run);
}

export type ExposureSource = "signal" | "manual" | "dca" | "grid";
export type ExposureDir = "long" | "short" | "neutral";

export interface BetaInfo {
  /** Clamped to [0, 3]; what the shocks use. */
  beta: number;
  /** Before the clamp. */
  raw: number;
  /** Share of the coin's hourly variance BTC explains, 0..1. */
  r2: number;
  samples: number;
  clamped: boolean;
}

export interface ExposureRow {
  id: string;
  source: ExposureSource;
  label: string;
  symbol: string;
  market: "futures" | "spot";
  live: boolean;
  direction: ExposureDir;
  capital: number;
  /** null: no price read yet, left out of every figure. */
  mark: number | null;
  stop: number | null;
  liq: number | null;
  beta: BetaInfo | null;
  betaError: string | null;
}

export interface Contribution {
  id: string;
  symbol: string;
  label: string;
  source: ExposureSource;
  pnl: number;
  /** Exit taken on the way (`riskview.exit.*`), null = still open. */
  exit: string | null;
  /** Replay only: own path or beta x BTC. */
  basis?: "history" | "beta";
}

export interface BookResult {
  pnl: number;
  pctOfCapital: number | null;
  btcSharePct: number | null;
  top: Contribution[];
  included: number;
  excluded: number;
  /** Replay: included exposures that followed beta x BTC (no own history). */
  estimated: number;
}

export interface Scenario {
  btcMovePct: number;
  book: BookResult;
}

export interface Concentration {
  sameDirectionPct: number | null;
  dominant: ExposureDir | null;
  topCoin: string | null;
  topCoinPct: number | null;
  avgBeta: number | null;
  withBeta: number;
}

export type CheckKey = "sameDirection" | "singleCoin" | "stress20";
export type CheckLevel = "ok" | "warn" | "danger" | "unavailable";

export interface Check {
  key: CheckKey;
  value: number | null;
  threshold: number;
  level: CheckLevel;
  sinceMs: number | null;
}

export interface BookStress {
  live: boolean;
  exposures: number;
  capital: number;
  scenarios: Scenario[];
  concentration: Concentration;
  checks: Check[];
}

export interface ViewThresholds {
  sameDirectionPct: number;
  singleCoinPct: number;
  stress20Pct: number;
}

export interface RiskViewSnapshot {
  computedAt: number;
  sample: boolean;
  betaDays: number;
  exposures: ExposureRow[];
  paper: BookStress;
  real: BookStress;
  thresholds: ViewThresholds;
  unpriced: number;
}

export interface ReplayWindow {
  id: "oct2025" | "ftx2022" | "luna2022" | "may2021" | "mar2020";
  startMs: number;
  endMs: number;
  status: "ok" | "unavailable";
  error: string | null;
  btcMovePct: number | null;
  btcLowPct: number | null;
  paper: BookResult | null;
  real: BookResult | null;
}

export interface ReplayReport {
  computedAt: number;
  sample: boolean;
  windows: ReplayWindow[];
}

/** What the New trade preview already holds (bot_preview_trade). */
export interface TradeCandidate {
  botKind: BotKind;
  symbol: string;
  direction: "long" | "short";
  entry: number;
  takeProfit: number;
  stopLoss: number;
  capital: number;
  notionalUsdt: number;
  effectiveLeverage: number;
  leverage: number;
}

export type WhatIfCandidate =
  | { type: "strategy"; config: StrategyConfig; price: number }
  | { type: "trade"; trade: TradeCandidate };

export interface WhatIfBrief {
  pnl: number;
  pctOfCapital: number | null;
  capital: number;
  included: number;
  excluded: number;
}

export interface WhatIf {
  btcMovePct: number;
  live: boolean;
  before: WhatIfBrief | null;
  after: WhatIfBrief | null;
  candidatePnl: number | null;
  candidateBeta: BetaInfo | null;
  error: string | null;
  sample: boolean;
}

/** The latest snapshot (`refresh` recomputes now). */
export const riskviewGet = (refresh = false) =>
  call<RiskViewSnapshot>("riskview_get", { refresh }, (m) => m.snapshot());

/** Today's book through the crash windows (several DataHub reads the first time). */
export const riskviewReplay = () => call<ReplayReport>("riskview_replay", undefined, (m) => m.replay());

export const riskviewWhatIf = (candidate: WhatIfCandidate) =>
  call<WhatIf>("riskview_whatif", { candidate }, (m) => m.whatIf(candidate));
