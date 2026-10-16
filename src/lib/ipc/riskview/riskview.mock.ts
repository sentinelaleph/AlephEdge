/**
 * DEV-ONLY sample data for the portfolio risk view in the browser preview
 * (`npm run dev`, the shot harness). Reached through `devMock`, so the
 * released bundle drops this module. Every number is invented to make the
 * panels readable, and every response says `sample: true`: the page labels
 * it "Sample data" so it is never read as a real book.
 */

import type {
  BookResult,
  BookStress,
  Contribution,
  ReplayReport,
  RiskViewSnapshot,
  WhatIf,
  WhatIfCandidate,
} from "./riskview";

const now = Date.now();

const c = (id: string, symbol: string, label: string, source: Contribution["source"], pnl: number, exit: string | null, basis?: "history" | "beta"): Contribution => ({
  id,
  symbol,
  label,
  source,
  pnl,
  exit,
  ...(basis ? { basis } : {}),
});

const book = (pnl: number, capital: number, share: number | null, top: Contribution[], excluded = 0, estimated = 0): BookResult => ({
  pnl,
  pctOfCapital: (pnl / capital) * 100,
  btcSharePct: share,
  top,
  included: 4,
  excluded,
  estimated,
});

const CAPITAL = 1650;

const paper: BookStress = {
  live: false,
  exposures: 4,
  capital: CAPITAL,
  scenarios: [
    {
      btcMovePct: -10,
      book: book(-148.2, CAPITAL, 71, [
        c("sb:sb_sampledcabot", "SOLUSDT", "DCA SOL classic", "dca", -96.4, null),
        c("pos:futures:s1", "SOLUSDT", "futures", "signal", -31.5, "sl"),
        c("pos:spot:u1", "ETHUSDT", "spot", "manual", -14.1, null),
        c("sb:sb_samplegridbt", "BTCUSDT", "Grid BTC range", "grid", -6.2, null),
      ]),
    },
    {
      btcMovePct: -20,
      book: book(-412.6, CAPITAL, 74, [
        c("sb:sb_sampledcabot", "SOLUSDT", "DCA SOL classic", "dca", -301.8, null),
        c("sb:sb_samplegridbt", "BTCUSDT", "Grid BTC range", "grid", -51.4, "stop"),
        c("pos:futures:s1", "SOLUSDT", "futures", "signal", -31.5, "sl"),
        c("pos:spot:u1", "ETHUSDT", "spot", "manual", -27.9, "sl"),
      ]),
    },
    {
      btcMovePct: -30,
      book: book(-498.0, CAPITAL, 76, [
        c("sb:sb_sampledcabot", "SOLUSDT", "DCA SOL classic", "dca", -387.2, null),
        c("sb:sb_samplegridbt", "BTCUSDT", "Grid BTC range", "grid", -51.4, "stop"),
        c("pos:futures:s1", "SOLUSDT", "futures", "signal", -31.5, "sl"),
        c("pos:spot:u1", "ETHUSDT", "spot", "manual", -27.9, "sl"),
      ]),
    },
    {
      btcMovePct: 10,
      book: { ...book(96.3, CAPITAL, null, []), top: [] },
    },
  ],
  concentration: { sameDirectionPct: 87.9, dominant: "long", topCoin: "SOL", topCoinPct: 69.7, avgBeta: 1.38, withBeta: 4 },
  checks: [
    { key: "sameDirection", value: 87.9, threshold: 80, level: "danger", sinceMs: now - 42 * 60_000 },
    { key: "singleCoin", value: 69.7, threshold: 40, level: "danger", sinceMs: now - 42 * 60_000 },
    { key: "stress20", value: 25.0, threshold: 25, level: "warn", sinceMs: null },
  ],
};

const emptyBook = (live: boolean): BookStress => ({
  live,
  exposures: 0,
  capital: 0,
  scenarios: [-10, -20, -30, 10].map((m) => ({
    btcMovePct: m,
    book: { pnl: 0, pctOfCapital: null, btcSharePct: null, top: [], included: 0, excluded: 0, estimated: 0 },
  })),
  concentration: { sameDirectionPct: null, dominant: null, topCoin: null, topCoinPct: null, avgBeta: null, withBeta: 0 },
  checks: [],
});

export const SAMPLE_SNAPSHOT: RiskViewSnapshot = {
  computedAt: now - 90_000,
  sample: true,
  betaDays: 30,
  exposures: [
    { id: "sb:sb_sampledcabot", source: "dca", label: "DCA SOL classic", symbol: "SOLUSDT", market: "futures", live: false, direction: "long", capital: 900, mark: 152.4, stop: null, liq: 98.2, beta: { beta: 1.62, raw: 1.62, r2: 0.71, samples: 718, clamped: false }, betaError: null },
    { id: "pos:futures:s1", source: "signal", label: "futures", symbol: "SOLUSDT", market: "futures", live: false, direction: "long", capital: 250, mark: 152.4, stop: 146.1, liq: 124.8, beta: { beta: 1.62, raw: 1.62, r2: 0.71, samples: 718, clamped: false }, betaError: null },
    { id: "pos:spot:u1", source: "manual", label: "spot", symbol: "ETHUSDT", market: "spot", live: false, direction: "long", capital: 300, mark: 3120.5, stop: 2840, liq: null, beta: { beta: 1.21, raw: 1.21, r2: 0.82, samples: 719, clamped: false }, betaError: null },
    { id: "sb:sb_samplegridbt", source: "grid", label: "Grid BTC range", symbol: "BTCUSDT", market: "futures", live: false, direction: "neutral", capital: 200, mark: 61250, stop: 57800, liq: null, beta: { beta: 1, raw: 1, r2: 1, samples: 0, clamped: false }, betaError: null },
  ],
  paper,
  real: emptyBook(true),
  thresholds: { sameDirectionPct: 80, singleCoinPct: 40, stress20Pct: 25 },
  unpriced: 0,
};

const replayBook = (pnl: number, top: Contribution[]): BookResult => ({ ...book(pnl, CAPITAL, 68, top) });

export const SAMPLE_REPLAY: ReplayReport = {
  computedAt: now,
  sample: true,
  windows: [
    {
      id: "oct2025",
      startMs: Date.UTC(2025, 9, 10),
      endMs: Date.UTC(2025, 9, 12),
      status: "ok",
      error: null,
      btcMovePct: -7.4,
      btcLowPct: -14.2,
      paper: replayBook(-356.1, [
        c("sb:sb_sampledcabot", "SOLUSDT", "DCA SOL classic", "dca", -248.9, null, "history"),
        c("pos:futures:s1", "SOLUSDT", "futures", "signal", -31.5, "sl", "history"),
      ]),
      real: null,
    },
    {
      id: "ftx2022",
      startMs: Date.UTC(2022, 10, 8),
      endMs: Date.UTC(2022, 10, 11),
      status: "ok",
      error: null,
      btcMovePct: -16.8,
      btcLowPct: -24.9,
      paper: replayBook(-470.3, [
        c("sb:sb_sampledcabot", "SOLUSDT", "DCA SOL classic", "dca", -366.0, null, "history"),
        c("sb:sb_samplegridbt", "BTCUSDT", "Grid BTC range", "grid", -51.4, "stop", "history"),
      ]),
      real: null,
    },
    {
      id: "luna2022",
      startMs: Date.UTC(2022, 4, 9),
      endMs: Date.UTC(2022, 4, 13),
      status: "ok",
      error: null,
      btcMovePct: -12.1,
      btcLowPct: -21.3,
      paper: replayBook(-401.7, [c("sb:sb_sampledcabot", "SOLUSDT", "DCA SOL classic", "dca", -309.4, null, "history")]),
      real: null,
    },
    {
      id: "may2021",
      startMs: Date.UTC(2021, 4, 19),
      endMs: Date.UTC(2021, 4, 20),
      status: "ok",
      error: null,
      btcMovePct: -11.6,
      btcLowPct: -29.4,
      paper: replayBook(-288.0, [c("pos:spot:u1", "ETHUSDT", "spot", "manual", -27.9, "sl", "history")]),
      real: null,
    },
    {
      id: "mar2020",
      startMs: Date.UTC(2020, 2, 12),
      endMs: Date.UTC(2020, 2, 13),
      status: "ok",
      error: null,
      btcMovePct: -37.2,
      btcLowPct: -41.1,
      // SOL had no Binance market in March 2020: both SOL exposures follow beta x BTC.
      paper: {
        ...replayBook(-512.4, [
          c("sb:sb_sampledcabot", "SOLUSDT", "DCA SOL classic", "dca", -401.0, null, "beta"),
          c("sb:sb_samplegridbt", "BTCUSDT", "Grid BTC range", "grid", -51.4, "stop", "history"),
          c("pos:futures:s1", "SOLUSDT", "futures", "signal", -31.5, "sl", "beta"),
          c("pos:spot:u1", "ETHUSDT", "spot", "manual", -27.9, "sl", "history"),
        ]),
        estimated: 2,
      },
      real: null,
    },
  ],
};

export function snapshot(): Promise<RiskViewSnapshot> {
  return Promise.resolve({ ...SAMPLE_SNAPSHOT, computedAt: Date.now() - 90_000 });
}

export function replay(): Promise<ReplayReport> {
  return new Promise((resolve) => setTimeout(() => resolve({ ...SAMPLE_REPLAY, computedAt: Date.now() }), 400));
}

export function whatIf(candidate: WhatIfCandidate): Promise<WhatIf> {
  const before = { pnl: -412.6, pctOfCapital: -25.0, capital: CAPITAL, included: 4, excluded: 0 };
  const budget = candidate.type === "strategy" ? candidate.config.budget : candidate.trade.capital;
  const candPnl = -0.31 * budget;
  const capital = CAPITAL + budget;
  return Promise.resolve({
    btcMovePct: -20,
    live: false,
    before,
    after: { pnl: before.pnl + candPnl, pctOfCapital: ((before.pnl + candPnl) / capital) * 100, capital, included: 5, excluded: 0 },
    candidatePnl: candPnl,
    candidateBeta: { beta: 1.45, raw: 1.45, r2: 0.66, samples: 717, clamped: false },
    error: null,
    sample: true,
  });
}
