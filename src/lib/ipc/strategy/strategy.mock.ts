/**
 * DEV-ONLY browser preview for the strategy IPC (no Rust in `npm run dev`).
 * Reached through `devMock`, so the released bundle drops this module. The
 * preview math is a rough stand-in for the Rust preview (no liquidation
 * model, no fees) so the form is reviewable; it is not the authority and it
 * never ships. Every bot here starts empty: no sample history is invented.
 */

import type {
  CloseAllReport,
  CycleRow,
  EquityRow,
  ExportPaths,
  FillRow,
  OrderRow,
  Preset,
  PreviewDto,
  StrategyBotView,
  StrategyConfig,
  StrategyDetail,
  StrategyKind,
  StrategyPnl,
  StrategyNote,
  StrategyRiskView,
  StrategyStats,
} from "./strategy";

const REF_PRICE: Record<string, number> = { BTCUSDT: 63000, ETHUSDT: 2500, SOLUSDT: 150, BNBUSDT: 560 };
const bots = new Map<string, { cfg: StrategyConfig; view: StrategyBotView }>();
let riskState: StrategyRiskView = {
  portfolioDdPct: 15,
  portfolioDdDefaultPct: 15,
  budgetCapPct: 40,
  levelBudgetCapPct: 40,
  maxLeverage: 5,
  balance: 10000,
  reservedBudget: 0,
  pnlQuote: 0,
  peakPnlQuote: 0,
  tripped: false,
  dayPnlQuote: 0,
  breakerBots: 0,
  breakerBudget: 0,
  botCount: 0,
};

const ok = <T>(v: T) => Promise.resolve(v);
const fail = (code: string) => Promise.reject(code);

function newId(): string {
  const a = "abcdefghijklmnopqrstuvwxyz234567";
  let s = "sb_";
  for (let i = 0; i < 12; i += 1) s += a[Math.floor(Math.random() * 32)];
  return s;
}

export function defaults(kind: StrategyKind, exchangeId: string, symbol: string): Promise<StrategyConfig> {
  const common = {
    schemaVersion: 1,
    exchangeId,
    market: "futures" as const,
    symbol,
    budget: 1000,
    leverage: 1,
    start: { type: "immediately" as const },
    restart: { cooldownMin: 0, maxCycles: null, afterStop: true, afterLiquidation: false, priceBand: null, endAtMs: null },
    maxDrawdownPct: 25,
    pauseOnBtcBreak: true,
    portfolioBreaker: true,
    presetId: null,
  };
  if (kind === "dca") {
    return ok({
      ...common,
      name: "DCA",
      side: "long",
      params: {
        kind: "dca",
        baseOrder: null,
        safetyOrder: null,
        baseWeight: 1,
        safetyWeight: 1,
        maxSo: 8,
        soStepPct: 2.5,
        stepScale: 1.3,
        volumeScale: 1.4,
        tpPct: 2,
        trailingPct: null,
        slPct: null,
        maxDurationMin: null,
      },
    });
  }
  return ok({
    ...common,
    name: "Grid",
    side: "neutral",
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
  });
}

function check(cfg: StrategyConfig): { code: string; field: string } | null {
  const name = cfg.name.trim();
  if (!name || name.length > 40) return { code: "nameInvalid", field: "name" };
  if (!/^[A-Z0-9]{1,16}USDT$/.test(cfg.symbol)) return { code: "symbolInvalid", field: "symbol" };
  if (!(cfg.budget > 0)) return { code: "budgetInvalid", field: "budget" };
  if (cfg.market === "spot" && cfg.side !== "long") return { code: "spotShortRefused", field: "side" };
  if (cfg.leverage > riskState.maxLeverage) return { code: "leverageAboveCeiling", field: "leverage" };
  const p = cfg.params;
  if (p.kind === "dca") {
    if (cfg.side === "neutral") return { code: "neutralDcaRefused", field: "side" };
    if (p.tpPct <= 0) return { code: "tpInvalid", field: "tpPct" };
    if (p.stepScale < 1 || p.stepScale > 10) return { code: "stepScaleInvalid", field: "stepScale" };
    if (p.volumeScale < 1 || p.volumeScale > 10) return { code: "volumeScaleInvalid", field: "volumeScale" };
    if (p.maxSo > 25) return { code: "maxSoAboveCap", field: "maxSo" };
  } else {
    if (p.nGrids < 2 || p.nGrids > 100) return { code: "gridLevelsInvalid", field: "nGrids" };
    if (p.trailingUp && cfg.side !== "long") return { code: "trailingUpLongOnly", field: "trailingUp" };
  }
  return null;
}

/** Rough stand-in for validate::min_budget (5 USDT smallest order). */
function minBudget(cfg: StrategyConfig): number | null {
  const lev = Math.max(1, cfg.leverage);
  const p = cfg.params;
  if (p.kind === "dca") {
    const raw = [p.baseOrder ?? p.baseWeight ?? 1];
    for (let k = 1; k <= p.maxSo; k += 1) raw.push((p.safetyOrder ?? p.safetyWeight ?? 1) * p.volumeScale ** (k - 1));
    const sum = raw.reduce((a, b) => a + b, 0);
    if (p.baseOrder !== null) return Math.min(...raw) >= 5 ? sum / lev : null;
    return Math.ceil(((5 * sum) / (Math.min(...raw) * lev)) * 100) / 100;
  }
  const price = REF_PRICE[cfg.symbol] ?? 100;
  const lower = p.range.type === "absolute" ? p.range.lower : price * (1 - p.range.lowerPct / 100);
  const upper = p.range.type === "absolute" ? p.range.upper : price * (1 + p.range.upperPct / 100);
  const n = Math.max(1, p.nGrids);
  const levels = Array.from({ length: n + 1 }, (_, i) => (p.spacing === "geom" ? lower * (upper / lower) ** (i / n) : lower + ((upper - lower) * i) / n));
  return Math.ceil(((5 * levels.slice(1).reduce((a, b) => a + b, 0)) / (lev * levels[0])) * 100) / 100;
}

export function validate(cfg: StrategyConfig): Promise<{ ok: boolean; error: { code: string; field: string } | null; minBudget: number | null }> {
  const error = check(cfg);
  return ok({ ok: error === null, error, minBudget: minBudget(cfg) });
}

export function preview(cfg: StrategyConfig): Promise<PreviewDto> {
  const price = REF_PRICE[cfg.symbol] ?? 100;
  const lev = Math.max(1, cfg.leverage);
  const warnings: string[] = [];
  if (cfg.leverage > 1) warnings.push("leveraged");
  if (cfg.maxDrawdownPct === null) warnings.push("noDrawdownStop");
  const error = check(cfg);
  const costs =
    cfg.market === "futures"
      ? { maker: 0.0002, taker: 0.0005, slippage: 0.0002, mmr: 0.005, funding: true }
      : { maker: 0.001, taker: 0.001, slippage: 0.0002, mmr: 0, funding: false };
  const p = cfg.params;
  if (p.kind === "dca") {
    if (p.slPct === null) warnings.push(cfg.maxDrawdownPct !== null ? "ddStopOnly" : "noStopLoss");
    const s = cfg.side === "short" ? -1 : 1;
    const devs: number[] = [0];
    let gap = p.soStepPct;
    for (let k = 1; k <= p.maxSo; k += 1) {
      devs.push(devs[k - 1] + gap);
      gap *= p.stepScale;
    }
    const raw = devs.map((_, k) => (k === 0 ? p.baseOrder ?? p.baseWeight ?? 1 : (p.safetyOrder ?? p.safetyWeight ?? 1) * p.volumeScale ** (k - 1)));
    const weightForm = p.baseOrder === null;
    const scale = weightForm ? (cfg.budget * lev) / raw.reduce((a, b) => a + b, 0) : 1;
    let cumN = 0;
    let cumQ = 0;
    const rungs = devs.map((d, k) => {
      const px = price * (1 - (s * d) / 100);
      const notional = raw[k] * scale;
      cumN += notional;
      cumQ += notional / px;
      const avg = cumN / cumQ;
      return { index: k, deviationPct: d, price: px, notional, qty: notional / px, cumNotional: cumN, cumQty: cumQ, avgEntry: avg, tpPrice: avg * (1 + (s * p.tpPct) / 100), liqPrice: null };
    });
    const worst: [number, "sl" | "ddStop"][] = [];
    if (p.slPct !== null) worst.push([(cumN * p.slPct) / 100, "sl"]);
    if (cfg.maxDrawdownPct !== null) worst.push([(cfg.maxDrawdownPct / 100) * cfg.budget, "ddStop"]);
    worst.sort((a, b) => a[0] - b[0]);
    return ok({
      kind: "dca",
      referencePrice: price,
      requiredCapital: cumN / lev,
      minBudget: minBudget(cfg),
      costs,
      dca: {
        rungs,
        lastSoPrice: p.maxSo > 0 ? rungs[rungs.length - 1].price : null,
        maxCoveragePct: devs[devs.length - 1],
        liqPrice: null,
        liqDistancePct: null,
        totalNotional: cumN,
        worstLossQuote: worst[0]?.[0] ?? null,
        worstLossBasis: worst[0]?.[1] ?? "none",
        suggestedSlPct: Math.min(90, Math.round(devs[devs.length - 1] + 5)),
      },
      grid: null,
      warnings,
      error,
    });
  }
  if (p.stopOutPct === null) warnings.push(cfg.maxDrawdownPct !== null ? "ddStopOnly" : "noStopOut");
  const lower = p.range.type === "absolute" ? p.range.lower : price * (1 - p.range.lowerPct / 100);
  const upper = p.range.type === "absolute" ? p.range.upper : price * (1 + p.range.upperPct / 100);
  const n = Math.max(1, p.nGrids);
  const levels = Array.from({ length: n + 1 }, (_, i) =>
    p.spacing === "geom" ? lower * (upper / lower) ** (i / n) : lower + ((upper - lower) * i) / n,
  );
  const q = (cfg.budget * lev) / levels.reduce((a, b) => a + b, 0);
  const rows = levels.map((l, i) => ({ index: i, price: l, side: (l < price ? "buy" : l > price ? "sell" : "none") as "buy" | "sell" | "none", qty: q, notional: q * l }));
  const buys = rows.filter((r) => r.side === "buy").length;
  const sells = rows.filter((r) => r.side === "sell").length;
  const step = (levels[1] / levels[0] - 1) * 100;
  const stepTop = (levels[n] / levels[n - 1] - 1) * 100;
  const initialBaseQty = cfg.side === "long" ? q * sells : cfg.side === "short" ? q * buys : 0;
  return ok({
    kind: "grid",
    referencePrice: price,
    requiredCapital: cfg.budget,
    minBudget: minBudget(cfg),
    costs,
    dca: null,
    grid: {
      lower,
      upper,
      levels: rows,
      qtyPerLevel: q,
      profitPerGridMinPct: Math.min(step, stepTop) - 0.04,
      profitPerGridMaxPct: Math.max(step, stepTop) - 0.04,
      buyOrders: buys,
      sellOrders: sells,
      initialBaseQty,
      initialQuote: initialBaseQty * price,
      stopLower: p.stopOutPct !== null ? lower * (1 - p.stopOutPct / 100) : null,
      stopUpper: p.stopOutPct !== null && !p.trailingUp ? upper * (1 + p.stopOutPct / 100) : null,
      liqPriceBottom: null,
      liqPriceTop: null,
    },
    warnings,
    error,
  });
}

function makeView(id: string, cfg: StrategyConfig): StrategyBotView {
  return {
    id,
    kind: cfg.params.kind,
    name: cfg.name.trim(),
    exchangeId: cfg.exchangeId,
    symbol: cfg.symbol,
    side: cfg.side,
    market: cfg.market,
    leverage: cfg.leverage,
    paper: true,
    runState: "stopped",
    acceptingNewCycles: false,
    pauseReason: null,
    budget: cfg.budget,
    realizedQuote: 0,
    realizedPct: 0,
    mtmPct: 0,
    equity: cfg.budget,
    drawdownPct: 0,
    maxDrawdownPct: 0,
    cyclesDone: 0,
    deadReason: null,
    createdAt: Date.now(),
    presetId: cfg.presetId,
    markPrice: null,
    markTs: null,
    openCycle: null,
    lastNote: null,
    utilisationInPosition: 0,
    utilisationCommitted: 0,
  };
}

function reserved(): number {
  return [...bots.values()].filter((b) => b.view.acceptingNewCycles).reduce((s, b) => s + b.cfg.budget, 0);
}

/** Screenshot harness only: registers sample bots (labelled "Sample data" there). */
export function seed(entries: { cfg: StrategyConfig; view: StrategyBotView }[]) {
  for (const e of entries) bots.set(e.view.id, e);
}

export const list = () => ok([...bots.values()].map((b) => b.view));

export function detail(id: string): Promise<StrategyDetail> {
  const b = bots.get(id);
  return b ? ok({ view: b.view, config: b.cfg, openOrders: [] }) : fail("botUnknown");
}

export function create(cfg: StrategyConfig): Promise<StrategyBotView> {
  const e = check(cfg);
  if (e) return fail(`${e.code}|${e.field}`);
  if (cfg.budget > (riskState.balance * riskState.budgetCapPct) / 100) return fail("budgetCapReached|budget");
  const id = newId();
  const view = makeView(id, cfg);
  bots.set(id, { cfg, view });
  return ok(view);
}

export function update(id: string, cfg: StrategyConfig): Promise<StrategyBotView> {
  const b = bots.get(id);
  if (!b) return fail("botUnknown");
  const e = check(cfg);
  if (e) return fail(`${e.code}|${e.field}`);
  const view = { ...makeView(id, cfg), runState: b.view.runState, acceptingNewCycles: b.view.acceptingNewCycles, createdAt: b.view.createdAt };
  bots.set(id, { cfg, view });
  return ok(view);
}

function setRun(id: string, on: boolean): Promise<StrategyBotView> {
  const b = bots.get(id);
  if (!b) return fail("botUnknown");
  if (on && reserved() + b.cfg.budget > (riskState.balance * riskState.budgetCapPct) / 100) return fail("budgetCapReached");
  b.view = { ...b.view, runState: on ? "armed" : "stopped", acceptingNewCycles: on };
  return ok(b.view);
}

export const start = (id: string) => setRun(id, true);
export const stop = (id: string) => setRun(id, false);
export const close = (id: string) => setRun(id, false);
export const closeAll = (): Promise<CloseAllReport> => ok({ closed: 0, unpriced: 0 });

export function archive(id: string): Promise<void> {
  const b = bots.get(id);
  if (!b) return fail("botUnknown");
  if (b.view.acceptingNewCycles) return fail("stopFirst");
  bots.delete(id);
  return ok(undefined);
}

export const cycles = (_id: string): Promise<CycleRow[]> => ok([]);
export const pnl = (): Promise<StrategyPnl> =>
  ok({ totals: { cycles: 0, netQuote: 0, todayCycles: 0, todayQuote: 0 }, recent: [] });
export const orders = (_id: string): Promise<OrderRow[]> => ok([]);
export const fills = (_id: string): Promise<FillRow[]> => ok([]);
export const equity = (_id: string): Promise<EquityRow[]> => ok([]);

export function stats(id?: string): Promise<StrategyStats> {
  return ok({
    bots: id ? 1 : bots.size,
    cycles: 0,
    winRate: null,
    meanCycleReturnPct: null,
    medianCycleReturnPct: null,
    returnPerDayOfCapitalPct: null,
    totalPnlQuote: 0,
    totalPnlPct: 0,
    maxDrawdownPct: 0,
    exits: {},
    feesPctOfBudget: 0,
    fundingPctOfBudget: 0,
    safetyOrdersFilledMean: null,
    gridClosingFillsPerCycle: null,
    utilisationInPosition: 0,
    utilisationCommitted: 0,
  });
}

export const exportCsv = (): Promise<ExportPaths> =>
  ok({ cyclesPath: "strategy-cycles-export.csv", fillsPath: "strategy-fills-export.csv" });

export function risk(): Promise<StrategyRiskView> {
  const covered = [...bots.values()].filter((b) => b.cfg.portfolioBreaker);
  return ok({
    ...riskState,
    reservedBudget: reserved(),
    breakerBots: covered.length,
    breakerBudget: covered.reduce((s, b) => s + b.cfg.budget, 0),
    botCount: bots.size,
  });
}

export function riskSet(args: { portfolioDdPct?: number; budgetCapPct?: number; rearm?: boolean }): Promise<StrategyRiskView> {
  riskState = {
    ...riskState,
    portfolioDdPct: args.portfolioDdPct ?? riskState.portfolioDdPct,
    budgetCapPct: args.budgetCapPct ?? riskState.budgetCapPct,
    tripped: args.rearm ? false : riskState.tripped,
  };
  return risk();
}

export const notes = (): Promise<StrategyNote[]> => ok([]);

/** Same figures as presets.rs (the Rust copy is the source). */
export function presets(): Promise<Preset[]> {
  return classicPreset().then((c) => [
    c,
    // One failed template so the dev preview shows the verdict and the ask.
    {
      ...c,
      id: "dca_short_classic",
      verdict: "failed",
      failReasons: ["negativeSplit", "ciIncludesZero", "monthsNotAllPositive", "liquidations"],
      situation: "bearMajors",
      config: { ...c.config, name: "DCA Short Classic", side: "short", presetId: "dca_short_classic" },
      history: {
        ...c.history,
        splits: [
          { split: "train", meanPerBotMonthPct: 1.7, ci95LowPct: 0.9, ci95HighPct: 2.4, monthsPositive: 5, months: 7, oneBotReturnPerDayPct: null },
          { split: "valid", meanPerBotMonthPct: 4.75, ci95LowPct: 2.1, ci95HighPct: 7.0, monthsPositive: 6, months: 7, oneBotReturnPerDayPct: null },
          { split: "test", meanPerBotMonthPct: -8.61, ci95LowPct: -20.47, ci95HighPct: 0.87, monthsPositive: 3, months: 8, oneBotReturnPerDayPct: null },
        ],
        testMeanAtTaker010Pct: null,
        oneBotWorstDrawdownPct: null,
        oneBotMedianDrawdownPct: null,
        testWorstBotDrawdownPct: -100.4,
        liquidations: 4,
      },
    },
  ]);
}

function classicPreset(): Promise<Preset> {
  return defaults("dca", "binance", "").then((base) => (
    {
      id: "dca_long_classic",
      verdict: "presetReady",
      failReasons: [],
      situation: "bullMajors",
      config: {
        ...base,
        name: "DCA Long Classic",
        pauseOnBtcBreak: false,
        portfolioBreaker: false,
        maxDrawdownPct: null,
        presetId: "dca_long_classic",
        params: {
          kind: "dca",
          baseOrder: null,
          safetyOrder: null,
          baseWeight: 1,
          safetyWeight: 1,
          maxSo: 8,
          soStepPct: 2.5,
          stepScale: 1.3,
          volumeScale: 1.4,
          tpPct: 2,
          trailingPct: null,
          slPct: null,
          maxDurationMin: null,
        },
      },
      capitalModel: "botPerSymbolMonth",
      universe: {
        rule: "topUsdtPerpsByQuoteVolume",
        rankFrom: 1,
        topN: 5,
        volumeWindowDays: 90,
        reselect: "monthly",
        exclude: ["USDCUSDT", "BTCDOMUSDT", "FDUSDUSDT", "TUSDUSDT", "BUSDUSDT", "USDPUSDT", "DEFIUSDT"],
      },
      history: {
        label: "historicalSimulation",
        simulator: "botsim",
        barInterval: "1h",
        makerFeePct: 0.02,
        takerFeePct: 0.05,
        slippagePct: 0.02,
        fillRule: "conservativeIntrabar",
        splits: [
          { split: "train", meanPerBotMonthPct: 2.93, ci95LowPct: 2.34, ci95HighPct: 3.44, monthsPositive: 7, months: 7, oneBotReturnPerDayPct: 0.054 },
          { split: "valid", meanPerBotMonthPct: 1.86, ci95LowPct: 0.82, ci95HighPct: 2.96, monthsPositive: 7, months: 7, oneBotReturnPerDayPct: 0.022 },
          { split: "test", meanPerBotMonthPct: 1.90, ci95LowPct: 1.38, ci95HighPct: 2.44, monthsPositive: 8, months: 8, oneBotReturnPerDayPct: 0.038 },
        ],
        testBots: 40,
        testShareBotsPositive: 1,
        testWorstBotDrawdownPct: -20.7,
        testMeanAtTaker010Pct: 1.95,
        testSignTestP: 0.0039,
        bonferroniBar: 0.00625,
        oneBotWorstDrawdownPct: -46.8,
        oneBotMedianDrawdownPct: -16.6,
        maxDealDays: 329,
        openDealsAtDataEnd: 5,
        liquidations: 0,
        stoppedDeals: 0,
        recheck: {
          runOn: "2026-10-09",
          testWindowEnd: "2026-09-27",
          partialMonth: "2026-10",
          partialMonthDays: 8.5,
          partialMonthMeanPct: -0.44,
          nextRead: "2026-11-01",
        },
      },
    }) as Preset);
}
