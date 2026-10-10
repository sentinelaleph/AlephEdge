/**
 * Dev-only sample data for the screenshot harness (ShotHarness.tsx), kept OUT
 * of the production bundle: main.tsx reaches it only behind
 * `import.meta.env.DEV`, through a dynamic import.
 *
 * Every number here is invented to make the screens readable. None of it is a
 * record of real trades or of Sentinel's performance, every position and trade
 * is simulated (`live: false`), and the harness labels each screen "Sample
 * data" so a screenshot cannot be mistaken for a track record.
 */

import type { DeskContextValue } from "@/app/DeskProvider";
import type { MembershipController } from "@/components/Membership/useMembership";
import type { BotDeskController } from "@/components/BotDesk/useBotDesk";
import type { VaultController } from "@/components/Vault/useVault";
import type { PnlController } from "@/components/PnlPanel/usePnl";
import type { RiskController } from "@/components/RiskSelector/useRisk";
import type { BotConfig, BotDeskStatus, OpenPosition } from "@/lib/ipc/bot/bot";
import type { ExchangeInfo } from "@/lib/ipc/exchange/exchange";
import type { RiskState } from "@/lib/ipc/risk/risk";
import type { Signal } from "@/lib/ipc/signal/signal";
import type { PnlStats, TradeRecord } from "@/lib/ipc/trades/trades";
import type { StrategyBotView, StrategyConfig } from "@/lib/ipc/strategy/strategy";
import type { StrategyDeskController } from "@/lib/ipc/strategy/useStrategyDesk";
import { seed as seedStrategyMock } from "@/lib/ipc/strategy/strategy.mock";
import { seed as seedBacktestMock } from "@/lib/ipc/strategy/backtest.mock";

const MIN = 60_000;
const HOUR = 60 * MIN;
const now = Date.now();
const noop = () => Promise.resolve();

export const SAMPLE_MEMBERSHIP: MembershipController = {
  view: {
    authenticated: true,
    active: true,
    state: "active",
    email: "sample@example.com",
    displayName: "Sample account",
    tier: "aleph",
    cancelAtPeriodEnd: false,
  },
  initializing: false,
  busy: false,
  error: null,
  signIn: noop,
  signOut: noop,
  refresh: noop,
};

export const SAMPLE_EXCHANGES: ExchangeInfo[] = [
  { id: "binance", name: "Binance", supportsFutures: true },
];

export const SAMPLE_FUTURES_CONFIG: BotConfig = {
  kind: "futures",
  exchangeId: "binance",
  maxPositions: 4,
  capital: 500,
  leverage: 5,
  direction: undefined,
  symbols: [],
  combos: [],
  engines: [],
  maxLossPct: 2,
  sizing: "risk",
  riskPerTradePct: 1,
  takeProfit: { kind: "tp2" },
  takeProfitOverrides: { SOLUSDT: { kind: "custom", pct: 12 }, ETHUSDT: { kind: "tp1" } },
};

export const SAMPLE_SPOT_CONFIG: BotConfig = {
  kind: "spot",
  exchangeId: "binance",
  maxPositions: 2,
  capital: 200,
  leverage: 1,
  symbols: ["BTCUSDT", "ETHUSDT"],
  combos: [],
  engines: [],
  sizing: "fixed",
  takeProfit: { kind: "tp1" },
  takeProfitOverrides: {},
};

function position(p: Partial<OpenPosition> & Pick<OpenPosition, "signalId" | "symbol" | "direction" | "entry" | "tp" | "sl">): OpenPosition {
  return {
    botKind: "futures",
    exchangeId: "binance",
    leverage: 5,
    capital: 500,
    frAtOpen: 0.004,
    ldAtOpen: 2_400_000,
    openedAt: now - 3 * HOUR,
    timeframe: "4h",
    signalEntry: p.entry,
    riskR: 1,
    plan: { breakeven_at_r: 1, partial_at_r: 1.5, partial_fraction: 0.5 },
    breakevenArmed: false,
    partialFraction: 0,
    partialPrice: null,
    horizonMs: now + 41 * HOUR,
    live: false,
    qty: 0,
    entryOrderId: null,
    stopAlgoId: null,
    tpAlgoId: null,
    stopAtBreakeven: false,
    unprotected: false,
    partialQty: 0,
    sizingMode: "risk",
    riskPct: 1,
    notionalUsdt: 0,
    effectiveLeverage: 0,
    riskCapped: false,
    tpTarget: "tp2",
    tpFallbackFrom: null,
    ...p,
  };
}

export const SAMPLE_POSITIONS: OpenPosition[] = [
  position({
    signalId: "sample-1",
    symbol: "SOLUSDT",
    direction: "long",
    entry: 142.35,
    tp: 159.43,
    sl: 136.1,
    notionalUsdt: 1116,
    effectiveLeverage: 2.23,
    tpTarget: "custom:12",
    breakevenArmed: true,
    partialFraction: 0.5,
    partialPrice: 151.2,
  }),
  position({
    signalId: "sample-2",
    symbol: "LINKUSDT",
    direction: "short",
    entry: 14.82,
    tp: 13.81,
    sl: 15.37,
    notionalUsdt: 1321,
    effectiveLeverage: 2.64,
    openedAt: now - 70 * MIN,
    horizonMs: now + 63 * HOUR,
  }),
  position({
    signalId: "sample-3",
    symbol: "ETHUSDT",
    direction: "long",
    entry: 2461.5,
    tp: 2555.1,
    sl: 2392.0,
    notionalUsdt: 1754,
    effectiveLeverage: 3.51,
    tpTarget: "tp1",
    openedAt: now - 25 * MIN,
    horizonMs: now + 70 * HOUR,
  }),
];

export const SAMPLE_DESK_STATUS: BotDeskStatus = {
  liveTradingEnabled: false,
  binanceIsProduction: true,
  futures: SAMPLE_FUTURES_CONFIG,
  spot: SAMPLE_SPOT_CONFIG,
  pump: null,
  futuresRunning: true,
  spotRunning: false,
  pumpRunning: false,
  openPositions: SAMPLE_POSITIONS,
  recentSkips: [
    { atMs: now - 4 * MIN, symbol: "DOGEUSDT", reason: "frAgainst", detail: "+0.061%" },
    { atMs: now - 11 * MIN, symbol: "AVAXUSDT", reason: "fillCrossedLevel", detail: "0.74%" },
    { atMs: now - 19 * MIN, symbol: "ARBUSDT", reason: "botMaxPositions", detail: "4" },
    { atMs: now - 34 * MIN, symbol: "OPUSDT", reason: "vetoedByRegime", detail: "BTC 1h -2.1%" },
    { atMs: now - 52 * MIN, symbol: "SUIUSDT", reason: "ldTooThin", detail: "7.4% of depth" },
    { atMs: now - 71 * MIN, symbol: "XRPUSDT", reason: "spotLongOnly" },
  ],
  killSwitchTripped: false,
  btcRegime: "normal",
};

export const SAMPLE_DESK: BotDeskController = {
  status: SAMPLE_DESK_STATUS,
  loaded: true,
  busy: false,
  error: null,
  configureAndStart: noop,
  stop: noop,
  setLive: () => Promise.resolve(null),
  endPilot: () => Promise.resolve(null),
  closePosition: () => Promise.resolve(null),
};

export const SAMPLE_RISK_STATE: RiskState = {
  limits: {
    level: "balanced",
    maxLeverage: 5,
    maxConcurrentPositions: 8,
    maxCapitalPct: 6,
    dailyLossLimitPct: 6,
    frThresholdPct: 0.1,
    maxDepthSharePct: 10,
  },
  balance: 2000,
  closeOnStop: true,
  maxCapitalQuote: 120,
  dailyLossOverridePct: 3,
  effectiveDailyLossPct: 3,
  dailyLossOverrideCapped: false,
};

export const SAMPLE_RISK: RiskController = {
  state: SAMPLE_RISK_STATE,
  busy: false,
  error: null,
  retry: () => undefined,
  selectLevel: noop,
  setBalance: noop,
  setCloseOnStop: noop,
  setDailyLoss: noop,
};

function trade(t: Partial<TradeRecord> & Pick<TradeRecord, "id" | "symbol" | "direction" | "entry" | "exit" | "pnlPct" | "exitReason">): TradeRecord {
  const pnlUsdt = +(500 * (t.pnlPct / 100)).toFixed(2);
  return {
    signalId: `sample-t${t.id}`,
    botKind: "futures",
    exchangeId: "binance",
    leverage: 5,
    capital: 500,
    pnlQuote: pnlUsdt,
    unleveredNetPct: null,
    frAtOpen: 0.006,
    ldAtOpen: 3_100_000,
    openedAt: now - t.id * 5 * HOUR - 3 * HOUR,
    closedAt: now - t.id * 5 * HOUR,
    live: false,
    fillEntry: null,
    fillExit: null,
    commissionUsdt: null,
    sizingMode: "risk",
    riskPct: 1,
    notionalUsdt: 1200,
    effectiveLeverage: 2.4,
    riskCapped: false,
    pnlUsdt,
    pnlPctOfCapital: t.pnlPct,
    vetoReasonCode: null,
    vetoReasonText: null,
    tpTarget: "tp2",
    tpFallbackFrom: null,
    ...t,
  };
}

export const SAMPLE_TRADES: TradeRecord[] = [
  trade({ id: 1, symbol: "BNBUSDT", direction: "long", entry: 584.2, exit: 612.9, pnlPct: 1.62, unleveredNetPct: 4.79, exitReason: "tp" }),
  trade({ id: 2, symbol: "ADAUSDT", direction: "short", entry: 0.4012, exit: 0.4159, pnlPct: -1.0, unleveredNetPct: -3.78, exitReason: "sl" }),
  trade({ id: 3, symbol: "SOLUSDT", direction: "long", entry: 138.4, exit: 138.4, pnlPct: -0.05, unleveredNetPct: -0.12, exitReason: "breakeven" }),
  trade({ id: 4, symbol: "DOTUSDT", direction: "short", entry: 4.21, exit: 4.02, pnlPct: 1.12, unleveredNetPct: 4.39, exitReason: "tp", tpTarget: "tp1" }),
  trade({ id: 5, symbol: "NEARUSDT", direction: "long", entry: 2.94, exit: 2.87, pnlPct: -0.61, unleveredNetPct: -2.5, exitReason: "veto", vetoReasonCode: "btc_turn", vetoReasonText: "BTC 4h trend turned bearish" }),
  trade({ id: 6, symbol: "ETHUSDT", direction: "long", entry: 2398.0, exit: 2330.1, pnlPct: -1.0, unleveredNetPct: -2.95, exitReason: "sl", tpTarget: "tp1" }),
  trade({ id: 7, symbol: "LTCUSDT", direction: "short", entry: 71.8, exit: 70.9, pnlPct: 0.31, unleveredNetPct: 1.13, exitReason: "horizon" }),
];

function stats(trades: TradeRecord[]): PnlStats {
  const wins = trades.filter((t) => t.exitReason === "tp").length;
  const losses = trades.filter((t) => t.exitReason === "sl").length;
  const gains = trades.filter((t) => t.pnlUsdt > 0).reduce((s, t) => s + t.pnlUsdt, 0);
  const pains = trades.filter((t) => t.pnlUsdt < 0).reduce((s, t) => s - t.pnlUsdt, 0);
  // Running drawdown over the chronological (oldest-first) sequence.
  let equity = 0;
  let peak = 0;
  let dd = 0;
  for (const t of [...trades].reverse()) {
    equity += t.pnlUsdt;
    peak = Math.max(peak, equity);
    dd = Math.max(dd, peak - equity);
  }
  return {
    totalTrades: trades.length,
    wins,
    losses,
    winRate: wins + losses > 0 ? wins / (wins + losses) : null,
    netPnlQuote: +(gains - pains).toFixed(2),
    avgPnlPct: trades.reduce((s, t) => s + t.pnlPct, 0) / trades.length,
    profitFactor: pains > 0 ? gains / pains : null,
    maxDrawdownQuote: +dd.toFixed(2),
    todayPnlQuote: trades.filter((t) => now - t.closedAt < 24 * HOUR).reduce((s, t) => s + t.pnlUsdt, 0),
  };
}

export const SAMPLE_PNL: PnlController = {
  stats: stats(SAMPLE_TRADES),
  split: false,
  scope: "simulated",
  setScope: () => undefined,
  trades: SAMPLE_TRADES,
  exportedTo: null,
  exportError: null,
  exportCsv: noop,
};

function signal(s: Partial<Signal> & Pick<Signal, "id" | "symbol" | "direction" | "entry" | "tp" | "sl" | "confidence" | "rr">, ageMin: number): Signal {
  return {
    mode: "hybrid",
    timeframe: "4h",
    confluence: [],
    expires_at: new Date(now - ageMin * MIN + 4 * HOUR).toISOString(),
    created_at: new Date(now - ageMin * MIN).toISOString(),
    management_plan: { breakeven_at_r: 1, partial_at_r: 1.5, partial_fraction: 0.5 },
    ...s,
  };
}

export const SAMPLE_SIGNALS: Signal[] = [
  signal({ id: "s1", symbol: "ETHUSDT", direction: "long", entry: 2461.5, tp: [2555.1, 2628.9, 2880.0], sl: 2392.0, confidence: 0.64, rr: 1.3 }, 1),
  signal({ id: "s2", symbol: "LINKUSDT", direction: "short", entry: 14.82, tp: [14.26, 13.81, 12.3], sl: 15.37, confidence: 0.58, rr: 1.0 }, 4),
  signal({ id: "s3", symbol: "SOLUSDT", direction: "long", entry: 142.35, tp: [147.76, 152.03, 166.5], sl: 136.1, confidence: 0.71, rr: 0.9 }, 9),
  signal({ id: "s4", symbol: "AVAXUSDT", direction: "long", entry: 21.07, tp: [21.87, 22.5, 24.65], sl: 20.21, confidence: 0.55, rr: 0.9 }, 14),
  signal({ id: "s5", symbol: "DOGEUSDT", direction: "short", entry: 0.1182, tp: [0.1137, 0.1102, 0.0981], sl: 0.1226, confidence: 0.49, rr: 1.0 }, 22),
  signal({ id: "s6", symbol: "ARBUSDT", direction: "long", entry: 0.5031, tp: [0.5222, 0.5373, 0.5886], sl: 0.4855, confidence: 0.61, rr: 1.1 }, 31),
  signal({ id: "s7", symbol: "SUIUSDT", direction: "long", entry: 1.874, tp: [1.945, 2.001, 2.193], sl: 1.809, confidence: 0.52, rr: 1.1 }, 47),
];

/** An unlocked vault holding one trade-only Binance key. */
export const SAMPLE_VAULT: VaultController = {
  status: { state: "unlocked", credentialCount: 1, idleTimeoutMinutes: 30 },
  credentials: [
    { exchangeId: "binance", label: "Binance", permission: "tradeOnly", hasPassphrase: false, addedAt: now - 60 * 24 * HOUR },
  ],
  initializing: false,
  busy: false,
  error: null,
  create: noop,
  unlock: noop,
  lock: noop,
  addKey: noop,
  removeKey: noop,
  renewKey: noop,
  renameKey: noop,
  changePassword: noop,
  setIdleMinutes: noop,
  reset: noop,
};

const SAMPLE_DCA_CONFIG: StrategyConfig = {
  schemaVersion: 1,
  name: "DCA ETHUSDT #1",
  exchangeId: "binance",
  market: "futures",
  symbol: "ETHUSDT",
  side: "long",
  budget: 1000,
  leverage: 1,
  start: { type: "immediately" },
  restart: { cooldownMin: 0, maxCycles: null, afterStop: true, afterLiquidation: false, priceBand: null, endAtMs: null },
  maxDrawdownPct: 25,
  pauseOnBtcBreak: true,
  portfolioBreaker: true,
  params: {
    kind: "dca",
    baseOrder: null,
    safetyOrder: null,
    baseWeight: 1,
    safetyWeight: 1,
    maxSo: 6,
    soStepPct: 1.5,
    stepScale: 1,
    volumeScale: 1.5,
    tpPct: 1.5,
    trailingPct: null,
    slPct: null,
    maxDurationMin: null,
  },
  presetId: null,
};

const SAMPLE_GRID_CONFIG: StrategyConfig = {
  ...SAMPLE_DCA_CONFIG,
  name: "Grid SOLUSDT #1",
  symbol: "SOLUSDT",
  side: "neutral",
  budget: 800,
  params: {
    kind: "grid",
    range: { type: "relative", lowerPct: 8, upperPct: 8 },
    nGrids: 16,
    spacing: "geom",
    stopOutPct: 3,
    trailingUp: false,
    trailUpLimit: null,
    takeProfitPct: null,
    maxDurationMin: null,
  },
};

export const SAMPLE_STRATEGY_BOTS: StrategyBotView[] = [
  {
    id: "sb_sampledcabot",
    kind: "dca",
    name: SAMPLE_DCA_CONFIG.name,
    exchangeId: "binance",
    symbol: "ETHUSDT",
    side: "long",
    market: "futures",
    leverage: 1,
    paper: true,
    runState: "inCycle",
    acceptingNewCycles: true,
    pauseReason: null,
    budget: 1000,
    realizedQuote: 18.42,
    realizedPct: 1.842,
    mtmPct: -0.61,
    equity: 1012.32,
    drawdownPct: -0.9,
    maxDrawdownPct: -3.4,
    cyclesDone: 7,
    deadReason: null,
    createdAt: now - 6 * 24 * HOUR,
    presetId: null,
    markPrice: 2471.5,
    markTs: now - MIN,
    openCycle: {
      seq: 8,
      openedAt: now - 9 * HOUR,
      anchorPrice: 2512.4,
      avgEntry: 2489.1,
      signedQty: 0.1205,
      notional: 299.94,
      margin: 299.94,
      reserved: 1000,
      cashQuote: 0,
      unrealizedQuote: -6.1,
      feesQuote: 0.09,
      fundingQuote: 0.02,
      soFilled: 2,
      gridClosingFills: null,
      liqPrice: null,
      maxAdversePct: -0.82,
      outOfRange: false,
      fundingUnknown: false,
    },
    lastNote: null,
    utilisationInPosition: 0.41,
    utilisationCommitted: 0.93,
  },
  {
    id: "sb_samplegridbt",
    kind: "grid",
    name: SAMPLE_GRID_CONFIG.name,
    exchangeId: "binance",
    symbol: "SOLUSDT",
    side: "neutral",
    market: "futures",
    leverage: 1,
    paper: true,
    runState: "stopped",
    acceptingNewCycles: false,
    pauseReason: null,
    budget: 800,
    realizedQuote: 0,
    realizedPct: 0,
    mtmPct: 0,
    equity: 800,
    drawdownPct: 0,
    maxDrawdownPct: 0,
    cyclesDone: 0,
    deadReason: null,
    createdAt: now - 2 * HOUR,
    presetId: null,
    markPrice: null,
    markTs: null,
    openCycle: null,
    lastNote: null,
    utilisationInPosition: 0,
    utilisationCommitted: 0,
  },
];

seedStrategyMock([
  { cfg: SAMPLE_DCA_CONFIG, view: SAMPLE_STRATEGY_BOTS[0] },
  { cfg: SAMPLE_GRID_CONFIG, view: SAMPLE_STRATEGY_BOTS[1] },
]);

/** One sample backtest run for the report shot (labelled "Sample data" by the harness). */
const SAMPLE_BT_T0 = Date.UTC(2026, 6, 1);
const SAMPLE_BT_H = 3_600_000;
seedBacktestMock([
  {
    id: "sample",
    createdAt: SAMPLE_BT_T0 + 92 * 24 * SAMPLE_BT_H,
    config: { ...SAMPLE_DCA_CONFIG, name: "DCA BTCUSDT sample" },
    symbol: "BTCUSDT",
    market: "futures",
    interval: "1h",
    startMs: SAMPLE_BT_T0,
    endMs: SAMPLE_BT_T0 + 90 * 24 * SAMPLE_BT_H,
    data: { source: "sample", candleCount: 2158, expectedCandles: 2160, coveragePct: 99.9, missingBars: 2, droppedCandles: 0, requests: 1, fundingIncluded: false },
    result: {
      budget: 1000,
      bars: 2158,
      firstBarMs: SAMPLE_BT_T0,
      lastBarMs: SAMPLE_BT_T0 + 90 * 24 * SAMPLE_BT_H - 1,
      closedCycles: 2,
      wins: 2,
      losses: 0,
      closedPnlQuote: 38.4,
      openAtEnd: { seq: 3, openedAt: SAMPLE_BT_T0 + 80 * 24 * SAMPLE_BT_H, closedAt: SAMPLE_BT_T0 + 90 * 24 * SAMPLE_BT_H - 1, exit: "end", anchorPrice: 64000, avgEntry: 62100, soFilled: 3, gridClosingFills: null, fills: 5, feesQuote: 0.9, fundingQuote: 0, pnlQuote: -21.3, pnlPctBudget: -2.13, maxAdversePct: -4.2, openAtEnd: true },
      openAtEndPnlQuote: -21.3,
      totalPnlQuote: 17.1,
      totalPnlPct: 1.71,
      maxDrawdownQuote: -42,
      maxDrawdownPct: -4.2,
      longestUnderwaterMs: 9 * 24 * SAMPLE_BT_H,
      deepestSo: 5,
      gridClosingFills: null,
      totalFills: 14,
      liquidations: 0,
      feesQuote: 2.6,
      fundingQuote: 0,
      exits: { tp: 2 },
      endState: "inCycle",
      deadReason: null,
      notes: {},
      utilisationInPosition: 0.41,
      utilisationCommitted: 0.97,
      equity: [0, 10, 20, 30, 40, 50, 60, 70, 80, 90].map((d, i) => ({ ts: SAMPLE_BT_T0 + d * 24 * SAMPLE_BT_H, equity: [1000, 996, 1012, 990, 1021, 1030, 1024, 1038, 1031, 1017.1][i] })),
      cycles: [
        { seq: 1, openedAt: SAMPLE_BT_T0, closedAt: SAMPLE_BT_T0 + 30 * 24 * SAMPLE_BT_H, exit: "tp", anchorPrice: 60000, avgEntry: 58800, soFilled: 5, gridClosingFills: null, fills: 7, feesQuote: 0.9, fundingQuote: 0, pnlQuote: 19.6, pnlPctBudget: 1.96, maxAdversePct: -4.2, openAtEnd: false },
        { seq: 2, openedAt: SAMPLE_BT_T0 + 30 * 24 * SAMPLE_BT_H, closedAt: SAMPLE_BT_T0 + 80 * 24 * SAMPLE_BT_H, exit: "tp", anchorPrice: 61500, avgEntry: 61000, soFilled: 2, gridClosingFills: null, fills: 4, feesQuote: 0.8, fundingQuote: 0, pnlQuote: 18.8, pnlPctBudget: 1.88, maxAdversePct: -2.6, openAtEnd: false },
        { seq: 3, openedAt: SAMPLE_BT_T0 + 80 * 24 * SAMPLE_BT_H, closedAt: SAMPLE_BT_T0 + 90 * 24 * SAMPLE_BT_H - 1, exit: "end", anchorPrice: 64000, avgEntry: 62100, soFilled: 3, gridClosingFills: null, fills: 5, feesQuote: 0.9, fundingQuote: 0, pnlQuote: -21.3, pnlPctBudget: -2.13, maxAdversePct: -4.2, openAtEnd: true },
      ],
      cyclesTruncated: false,
    },
  },
]);

export const SAMPLE_STRATEGY: StrategyDeskController = {
  bots: SAMPLE_STRATEGY_BOTS,
  risk: {
    portfolioDdPct: 15,
    portfolioDdDefaultPct: 15,
    budgetCapPct: 40,
    levelBudgetCapPct: 40,
    maxLeverage: 5,
    balance: 10000,
    reservedBudget: 1000,
    pnlQuote: 12.32,
    peakPnlQuote: 21.4,
    tripped: false,
    dayPnlQuote: -3.2,
    breakerBots: 2,
    breakerBudget: 1800,
    botCount: 2,
  },
  notes: [],
  // The sample DCA bot's 7 closed cycles (realized 18.42 USDT), the newest today.
  pnl: {
    totals: { cycles: 7, netQuote: 18.42, todayCycles: 1, todayQuote: 2.31 },
    recent: [
      {
        botId: "sb_sampledcabot",
        botName: SAMPLE_DCA_CONFIG.name,
        kind: "dca",
        market: "futures",
        symbol: "ETHUSDT",
        side: "long",
        seq: 7,
        openedAt: now - 9 * HOUR,
        closedAt: now - 2 * HOUR,
        exitReason: "tp",
        pnlQuote: 2.31,
        pnlPctBudget: 0.23,
        archived: false,
      },
    ],
  },
  loadError: null,
  busyId: null,
  refresh: noop,
  act: () => Promise.resolve(null),
  rearmBreaker: () => Promise.resolve(null),
};

/** The whole desk context the shell pages read, built from the samples above. */
export const SAMPLE_DESK_CONTEXT: DeskContextValue = {
  membership: SAMPLE_MEMBERSHIP,
  vault: SAMPLE_VAULT,
  risk: SAMPLE_RISK,
  desk: SAMPLE_DESK,
  pnl: SAMPLE_PNL,
  strategy: SAMPLE_STRATEGY,
  catalog: { exchanges: SAMPLE_EXCHANGES, error: null, retry: () => undefined },
  health: {
    exchanges: [{ id: "binance", name: "Binance", level: "ok", latencyMs: 41 }],
    signal: { level: "ok", connected: true, lastSignalSecs: 12, latencyMs: 130 },
    // No BTC price in the samples: the status bar shows "Price unavailable".
    btc: { trend: "neutral", strength: 0, phase: "", price: 0, changePct: 0, sparkline: [] },
    vault: "unlocked",
    membership: "active",
    generatedAt: now,
  },
  feed: {
    signals: SAMPLE_SIGNALS,
    health: { connected: true, phase: "live", lastSignalSecs: 12 },
    loaded: true,
    updatedAt: now,
    reconnect: noop,
  },
  endpoints: {
    apiBase: "http://localhost:8080",
    relayUrl: "ws://localhost:8080",
    deskId: "desk-sample",
    binanceFuturesBase: "http://localhost:8080",
    binanceIsProduction: true,
  },
  version: "0.3.0",
  keyedExchanges: SAMPLE_EXCHANGES,
  accountExchanges: ["binance"],
  accountExchange: "binance",
};
