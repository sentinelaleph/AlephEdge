/**
 * Strategy-bot IPC bridge (mirrors src-tauri/src/bot/strategy). DCA and Grid
 * bots are PAPER ONLY: no type here can carry a real-money intent, there is
 * no `live` field, and no command reaches an exchange order endpoint.
 *
 * Rust is the authority for defaults, bounds and validation. The UI renders
 * the codes it returns (`strategy.errors.*`, `strategy.warnings.*`,
 * `strategy.notes.*`) and never re-implements a bound.
 *
 * Outside Tauri, `npm run dev` gets an in-memory preview through `devMock`;
 * the released bundle contains neither the mock nor its sample values.
 */

import { devMock, inTauri, invoke } from "../bridge";

export type StrategyKind = "dca" | "grid";
export type StrategySide = "long" | "short" | "neutral";
export type MarketKind = "futures" | "spot";
export type Spacing = "arith" | "geom";
export type CrossDir = "up" | "down";

export interface RestartPolicy {
  cooldownMin: number;
  maxCycles: number | null;
  afterStop: boolean;
  afterLiquidation: boolean;
  /** [min, max] of the start bar's open; null = no band. */
  priceBand: [number, number] | null;
  endAtMs: number | null;
}

export type StartCondition =
  | { type: "immediately" }
  | { type: "priceCross"; price: number; direction: CrossDir };

export interface DcaParams {
  baseOrder: number | null;
  safetyOrder: number | null;
  baseWeight: number | null;
  safetyWeight: number | null;
  maxSo: number;
  soStepPct: number;
  stepScale: number;
  volumeScale: number;
  tpPct: number;
  trailingPct: number | null;
  slPct: number | null;
  maxDurationMin: number | null;
}

export type GridRange =
  | { type: "absolute"; lower: number; upper: number }
  | { type: "relative"; lowerPct: number; upperPct: number };

export interface GridParams {
  range: GridRange;
  /** Intervals (levels = nGrids + 1). */
  nGrids: number;
  spacing: Spacing;
  stopOutPct: number | null;
  trailingUp: boolean;
  trailUpLimit: number | null;
  takeProfitPct: number | null;
  maxDurationMin: number | null;
}

export type StrategyParams = ({ kind: "dca" } & DcaParams) | ({ kind: "grid" } & GridParams);

export interface StrategyConfig {
  schemaVersion: number;
  name: string;
  exchangeId: string;
  market: MarketKind;
  symbol: string;
  side: StrategySide;
  budget: number;
  leverage: number;
  start: StartCondition;
  restart: RestartPolicy;
  maxDrawdownPct: number | null;
  pauseOnBtcBreak: boolean;
  /** Inside the strategy portfolio breaker (counted, closed and held by it). */
  portfolioBreaker: boolean;
  params: StrategyParams;
  presetId: string | null;
}

export type RunState = "stopped" | "armed" | "inCycle" | "paused" | "dead";

export type ExitReason =
  | "tp"
  | "trail"
  | "sl"
  | "liq"
  | "stop"
  | "gridTp"
  | "timeout"
  | "ddstop"
  | "portfolioDd"
  | "dailyStop"
  | "remoteKill"
  | "manual"
  | "end"
  /** Losses shrank orders below 1% of budget or the 5 USDT exchange minimum. */
  | "sizeDown";

export interface StrategyNote {
  atMs: number;
  botId: string;
  symbol: string;
  /** i18n key under `strategy.notes.*`. */
  key: string;
  detail: string | null;
}

export interface OpenCycleView {
  seq: number;
  openedAt: number;
  anchorPrice: number;
  avgEntry: number | null;
  signedQty: number;
  notional: number;
  margin: number;
  reserved: number;
  cashQuote: number;
  unrealizedQuote: number;
  feesQuote: number;
  fundingQuote: number;
  soFilled: number | null;
  gridClosingFills: number | null;
  liqPrice: number | null;
  maxAdversePct: number;
  outOfRange: boolean;
  fundingUnknown: boolean;
}

export interface StrategyBotView {
  id: string;
  kind: StrategyKind;
  name: string;
  exchangeId: string;
  symbol: string;
  side: StrategySide;
  market: MarketKind;
  leverage: number;
  /** Always true in v1. */
  paper: boolean;
  runState: RunState;
  /** Start pressed and not paused: the bot may open NEW cycles. */
  acceptingNewCycles: boolean;
  /** "priceStale" | "outOfRange" (i18n under strategy.notes.*). */
  pauseReason: string | null;
  budget: number;
  realizedQuote: number;
  realizedPct: number;
  /** Floating P&L of the open cycle, % of budget. */
  mtmPct: number;
  equity: number;
  /** Current drawdown from the equity peak, % of budget (<= 0). */
  drawdownPct: number;
  /** Worst drawdown seen, % of budget (<= 0). */
  maxDrawdownPct: number;
  cyclesDone: number;
  deadReason: ExitReason | null;
  createdAt: number;
  presetId: string | null;
  markPrice: number | null;
  markTs: number | null;
  openCycle: OpenCycleView | null;
  lastNote: StrategyNote | null;
  utilisationInPosition: number;
  utilisationCommitted: number;
}

export type OrderRole =
  | { role: "base" }
  | { role: "safety"; index: number }
  | { role: "takeProfit" }
  | { role: "stopLoss" }
  | { role: "gridBuy"; index: number }
  | { role: "gridSell"; index: number }
  | { role: "close" };

export type OrderState =
  | "planned"
  | "submitted"
  | "open"
  | "partiallyFilled"
  | "filled"
  | "canceled"
  | "rejected"
  | "expired";

export interface SimOrder {
  clientId: string;
  role: OrderRole;
  side: "buy" | "sell";
  kind: "limit" | "market" | "stopMarket";
  price: number;
  qty: number;
  activeFromLeg: number;
  state: OrderState;
}

export interface StrategyDetail {
  view: StrategyBotView;
  config: StrategyConfig;
  openOrders: SimOrder[];
}

export interface StrategyErrorDto {
  code: string;
  field: string | null;
}

export interface LadderRung {
  index: number;
  deviationPct: number;
  price: number;
  notional: number;
  qty: number;
  cumNotional: number;
  cumQty: number;
  avgEntry: number;
  tpPrice: number;
  liqPrice: number | null;
}

export interface DcaPreview {
  rungs: LadderRung[];
  lastSoPrice: number | null;
  maxCoveragePct: number;
  liqPrice: number | null;
  liqDistancePct: number | null;
  totalNotional: number;
  /** Loss of the full ladder at whichever exit comes first. */
  worstLossQuote: number | null;
  worstLossBasis: "sl" | "ddStop" | "liq" | "none";
  /** The stop loss proposed when it is switched on; null = none fits. */
  suggestedSlPct: number | null;
}

export interface GridLevelRow {
  index: number;
  price: number;
  side: "buy" | "sell" | "none";
  qty: number;
  notional: number;
}

export interface GridPreview {
  lower: number;
  upper: number;
  levels: GridLevelRow[];
  qtyPerLevel: number;
  profitPerGridMinPct: number;
  profitPerGridMaxPct: number;
  buyOrders: number;
  sellOrders: number;
  initialBaseQty: number;
  initialQuote: number;
  stopLower: number | null;
  stopUpper: number | null;
  liqPriceBottom: number | null;
  liqPriceTop: number | null;
}

/** Fractions (0.0005 = 0.05%) the paper simulation charges. */
export interface CostModel {
  maker: number;
  taker: number;
  slippage: number;
  mmr: number;
  funding: boolean;
}

export interface PreviewDto {
  kind: StrategyKind;
  referencePrice: number;
  requiredCapital: number;
  /** Smallest budget at which every order clears the 5 USDT minimum; null = the budget cannot fix it. */
  minBudget: number | null;
  costs: CostModel;
  dca: DcaPreview | null;
  grid: GridPreview | null;
  /** i18n codes under `strategy.warnings.*`. */
  warnings: string[];
  error: StrategyErrorDto | null;
}

export interface CycleRow {
  botId: string;
  seq: number;
  openedAt: number;
  closedAt: number | null;
  exitReason: string | null;
  anchorPrice: number;
  sizeFactor: number;
  avgEntry: number | null;
  maxNotional: number | null;
  soFilled: number | null;
  gridClosingFills: number | null;
  realizedQuote: number;
  feesQuote: number;
  fundingQuote: number;
  pnlPctBudget: number | null;
  maxAdversePct: number | null;
  fundingUnknown: boolean;
}

export interface OrderRow {
  seq: number;
  order: SimOrder;
  updatedAt: number;
}

/** Net result of closed paper DCA / Grid cycles, deleted bots included (strategy_pnl). */
export interface StrategyPnlTotals {
  cycles: number;
  /** Net: realized minus fees and funding, USDT. */
  netQuote: number;
  /** Cycles closed since the UTC day start. */
  todayCycles: number;
  todayQuote: number;
}

/** One closed cycle with its bot's name (History, alerts). */
export interface ClosedCycleRow {
  botId: string;
  botName: string;
  kind: StrategyKind;
  market: MarketKind;
  symbol: string;
  side: StrategySide;
  seq: number;
  openedAt: number;
  closedAt: number;
  exitReason: ExitReason | null;
  /** Net: realized minus fees and funding, USDT. */
  pnlQuote: number;
  pnlPctBudget: number | null;
  /** The bot was deleted; its history stays. */
  archived: boolean;
}

export interface StrategyPnl {
  totals: StrategyPnlTotals;
  /** Newest first. */
  recent: ClosedCycleRow[];
}

export interface FillRow {
  seq: number;
  clientId: string;
  ts: number;
  price: number;
  qty: number;
  liquidity: string;
  feeQuote: number;
  realizedQuote: number;
  kind: string;
}

export interface EquityRow {
  ts: number;
  equity: number;
  margin: number;
  reserved: number;
}

export interface StrategyStats {
  bots: number;
  cycles: number;
  winRate: number | null;
  meanCycleReturnPct: number | null;
  medianCycleReturnPct: number | null;
  returnPerDayOfCapitalPct: number | null;
  totalPnlQuote: number;
  totalPnlPct: number;
  maxDrawdownPct: number;
  exits: Record<string, number>;
  feesPctOfBudget: number;
  fundingPctOfBudget: number;
  safetyOrdersFilledMean: number | null;
  gridClosingFillsPerCycle: number | null;
  utilisationInPosition: number;
  utilisationCommitted: number;
}

export interface StrategyRiskView {
  portfolioDdPct: number;
  portfolioDdDefaultPct: number;
  budgetCapPct: number;
  levelBudgetCapPct: number;
  maxLeverage: number;
  balance: number;
  reservedBudget: number;
  pnlQuote: number;
  peakPnlQuote: number;
  tripped: boolean;
  dayPnlQuote: number;
  /** Bots inside the portfolio breaker, of `botCount`, and their budgets. */
  breakerBots: number;
  breakerBudget: number;
  botCount: number;
}

export interface CloseAllReport {
  closed: number;
  unpriced: number;
}

export interface ExportPaths {
  cyclesPath: string;
  fillsPath: string;
}

export interface SplitResult {
  split: "train" | "valid" | "test";
  meanPerBotMonthPct: number;
  ci95LowPct: number;
  ci95HighPct: number;
  monthsPositive: number;
  months: number;
  /** Measured for dca_long_classic only. */
  oneBotReturnPerDayPct: number | null;
}

export interface HistoricalSimulation {
  label: "historicalSimulation";
  simulator: string;
  barInterval: string;
  makerFeePct: number;
  takerFeePct: number;
  slippagePct: number;
  fillRule: string;
  splits: SplitResult[];
  testBots: number;
  testShareBotsPositive: number;
  testWorstBotDrawdownPct: number;
  testMeanAtTaker010Pct: number | null;
  testSignTestP: number;
  bonferroniBar: number;
  oneBotWorstDrawdownPct: number | null;
  oneBotMedianDrawdownPct: number | null;
  maxDealDays: number;
  openDealsAtDataEnd: number;
  liquidations: number;
  stoppedDeals: number;
  /** A later re-run that did not confirm every check (Rust presets::Recheck). */
  recheck: PresetRecheck | null;
}

export interface PresetRecheck {
  runOn: string;
  testWindowEnd: string;
  partialMonth: string;
  partialMonthDays: number;
  partialMonthMeanPct: number;
  nextRead: string;
}

export interface PresetUniverse {
  rule: string;
  /** First rank taken (1 = the most traded pair). */
  rankFrom: number;
  topN: number;
  volumeWindowDays: number;
  reselect: string;
  exclude: string[];
}

export type PresetVerdict = "presetReady" | "failed";

export interface Preset {
  id: string;
  verdict: PresetVerdict;
  /** Checks a failed template did not pass (strategy.preset.reason.*). */
  failReasons: string[];
  /** The market it is meant for (strategy.preset.situation.*). */
  situation: string;
  config: StrategyConfig;
  capitalModel: string;
  universe: PresetUniverse;
  history: HistoricalSimulation;
}

type Mock = typeof import("./strategy.mock");
const mock = <T>(run: (m: Mock) => Promise<T>) => devMock(() => import("./strategy.mock"), run);

function call<T>(cmd: string, args: Record<string, unknown> | undefined, run: (m: Mock) => Promise<T>): Promise<T> {
  return inTauri() ? invoke<T>(cmd, args) : mock(run);
}

export const strategyList = () => call<StrategyBotView[]>("strategy_list", undefined, (m) => m.list());

export const strategyDetail = (id: string) =>
  call<StrategyDetail>("strategy_detail", { id }, (m) => m.detail(id));

export const strategyDefault = (kind: StrategyKind, exchangeId: string, symbol: string) =>
  call<StrategyConfig>("strategy_default", { kind, exchangeId, symbol }, (m) => m.defaults(kind, exchangeId, symbol));

/** `lastPrice` null = Rust reads the live price itself. */
export const strategyPreview = (config: StrategyConfig, lastPrice: number | null = null) =>
  call<PreviewDto>("strategy_preview", { config, lastPrice }, (m) => m.preview(config));

export interface ValidationReport {
  ok: boolean;
  error: StrategyErrorDto | null;
  /** As PreviewDto.minBudget (price-free). */
  minBudget: number | null;
}

/** Bounds check without a price (used when the preview has no price). */
export const strategyValidate = (config: StrategyConfig) =>
  call<ValidationReport>("strategy_validate", { config }, (m) => m.validate(config));

export const strategyCreate = (config: StrategyConfig) =>
  call<StrategyBotView>("strategy_create", { config }, (m) => m.create(config));

export const strategyUpdate = (id: string, config: StrategyConfig) =>
  call<StrategyBotView>("strategy_update", { id, config }, (m) => m.update(id, config));

/** Resume: arms the bot for new cycles. */
export const strategyStart = (id: string) => call<StrategyBotView>("strategy_start", { id }, (m) => m.start(id));

/** Pause: no new cycles; an open cycle keeps being managed. */
export const strategyStop = (id: string) => call<StrategyBotView>("strategy_stop", { id }, (m) => m.stop(id));

/** Closes the open paper cycle at market and stops the bot. */
export const strategyClose = (id: string) =>
  call<StrategyBotView>("strategy_close", { id, mode: "market" }, (m) => m.close(id));

export const strategyCloseAll = (reason?: string) =>
  call<CloseAllReport>("strategy_close_all", { reason: reason ?? null }, (m) => m.closeAll());

/** Delete = archive: hidden from the desk, history kept. */
export const strategyArchive = (id: string) => call<void>("strategy_archive", { id }, (m) => m.archive(id));

export const strategyCycles = (id: string, limit?: number) =>
  call<CycleRow[]>("strategy_cycles", { id, limit: limit ?? null }, (m) => m.cycles(id));

export const strategyOrders = (id: string) => call<OrderRow[]>("strategy_orders", { id }, (m) => m.orders(id));

export const strategyFills = (id: string, limit?: number) =>
  call<FillRow[]>("strategy_fills", { id, limit: limit ?? null }, (m) => m.fills(id));

export const strategyEquity = (id: string, fromMs?: number) =>
  call<EquityRow[]>("strategy_equity", { id, fromMs: fromMs ?? null }, (m) => m.equity(id));

export const strategyStats = (scope: "bot" | "all", id?: string) =>
  call<StrategyStats>("strategy_stats", { scope, id: id ?? null }, (m) => m.stats(id));

export const strategyPnl = (limit?: number) =>
  call<StrategyPnl>("strategy_pnl", limit === undefined ? undefined : { limit }, (m) => m.pnl());

export const strategyExportCsv = () => call<ExportPaths>("strategy_export_csv", undefined, (m) => m.exportCsv());

export const strategyRiskGet = () => call<StrategyRiskView>("strategy_risk_get", undefined, (m) => m.risk());

export const strategyRiskSet = (args: { portfolioDdPct?: number; budgetCapPct?: number; rearm?: boolean }) =>
  call<StrategyRiskView>(
    "strategy_risk_set",
    { portfolioDdPct: args.portfolioDdPct ?? null, budgetCapPct: args.budgetCapPct ?? null, rearm: args.rearm ?? null },
    (m) => m.riskSet(args),
  );

export const strategyNotes = () => call<StrategyNote[]>("strategy_notes", undefined, (m) => m.notes());

export const strategyPresets = () => call<Preset[]>("strategy_presets", undefined, (m) => m.presets());

/** A strategy-bot id ("sb_" + 12 base32 characters). */
export function isStrategyBotId(id: string): boolean {
  return /^sb_[a-z2-7]{12}$/.test(id);
}

/** "code|field" (or "code") from a rejected command. */
export function parseStrategyError(raw: string): StrategyErrorDto {
  const [code, field] = raw.split("|");
  return { code: code.trim(), field: field?.trim() || null };
}

/** True while Pause applies (the bot may open new cycles). */
export function isActive(v: StrategyBotView): boolean {
  return v.acceptingNewCycles && v.runState !== "dead";
}

/** Real-money state of one DCA / Grid bot (bot/strategy_live.rs). */
export interface StrategyLiveView {
  botId: string;
  enabled: boolean;
  /** i18n key under strategy.notes when the mirror stopped acting. */
  halted: string | null;
  realQty: number;
  entryPrice: number;
  unrealizedUsdt: number;
  stopPrice: number | null;
  lastSyncMs: number;
  /** Closed cycles' cash flow from real fills, before fees. */
  realizedGrossUsdt: number;
  /** 0.05% taker estimate over every real fill. */
  feesEstUsdt: number;
  fills: number;
  /** Pilot cycles left (0 = full size) and the current real / simulated size. */
  pilotCyclesLeft: number;
  cycleFactor: number;
  /** Exchange the real orders go to. */
  venue: string;
}

/** The word typed to switch a DCA / Grid bot to real money. */
export const STRATEGY_LIVE_WORD = "LIVE";

export const strategyLiveStatus = () =>
  call<StrategyLiveView[]>("strategy_live_status", undefined, async () => []);

/** `venue`: the exchange real orders go to (decisions stay on Binance prices). */
export const strategySetLive = (id: string, enabled: boolean, confirmation: string, venue?: string) =>
  call<StrategyLiveView[]>("strategy_set_live", { id, enabled, confirmation, venue: venue ?? null }, async () => {
    throw "liveBuildDisabled";
  });

/** One real fill against the simulated fills it mirrored (strategy_live_report). */
export interface StrategyDiffRow {
  seq: number;
  ts: number;
  kind: string;
  side: "buy" | "sell";
  realQty: number;
  realPrice: number;
  /** null: the exchange made the fill (its own stop) or nothing matched. */
  simPrice: number | null;
  /** Positive = worse than the simulation. */
  slippageBps: number | null;
  slippageUsdt: number | null;
  delayS: number | null;
}

export interface StrategyDiffReport {
  rows: StrategyDiffRow[];
  summary: {
    matched: number;
    unmatched: number;
    avgSlippageBps: number | null;
    slippageUsdt: number;
    avgDelayS: number | null;
    feesEstUsdt: number;
  };
}

export const strategyLiveReport = (id: string) =>
  call<StrategyDiffReport>("strategy_live_report", { id }, async () => ({
    rows: [],
    summary: { matched: 0, unmatched: 0, avgSlippageBps: null, slippageUsdt: 0, avgDelayS: null, feesEstUsdt: 0 },
  }));

/** Ends the pilot: full size from the next cycle. */
export const strategyEndPilot = (id: string) => call<StrategyLiveView[]>("strategy_end_pilot", { id }, async () => []);
