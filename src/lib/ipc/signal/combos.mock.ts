import type { ComboEntry } from "./combos";

/**
 * DEV-ONLY browser preview, reached through `devMock` (absent from the
 * released bundle): the real 2026-08-12 payload from ribqa.com, verbatim.
 * Using live values rather than round invented ones keeps the live/backtest
 * disagreement visible while reviewing the UI in `npm run dev` — a mock with
 * tidy matching numbers would hide the exact thing this panel exists to show.
 */
export const MOCK: ComboEntry[] = [
  {
    id: "stophunt_snap",
    name: "Stophunt Snap",
    description:
      "Price breaches a resting liquidity pool and closes back inside range, with momentum confirming the reversal.",
    sequenceHuman: "Stophunt breach + close-back → momentum/pump agreement",
    status: "best",
    live: { n: 73, winRate: 0.8082, avgPnl: 0.9312 },
    backtest: { n: 94, winRate: 0.5851, avgPnl: 0 },
  },
  {
    id: "breaker_flip",
    name: "Breaker Flip",
    description:
      "A failed order block flips into a breaker block and is retested in agreement with the prevailing market structure.",
    sequenceHuman: "Breaker (fail → flip → retest) + structure agreement",
    status: "candidate",
    live: { n: 18, winRate: 0.6667, avgPnl: 0.5082 },
    backtest: { n: 21, winRate: 0.4762, avgPnl: 0 },
  },
  {
    id: "liquidity_reversal",
    name: "Liquidity Reversal",
    description:
      "A liquidity sweep is immediately followed by an opposite-direction displacement that flips market structure, then price retests a still-active order block or fair value gap.",
    sequenceHuman: "Sweep → Displacement → CHoCH/MSB → fresh OB/FVG retest",
    status: "candidate",
    live: { n: 0, winRate: 0, avgPnl: 0 },
    backtest: { n: 3, winRate: 0.3333, avgPnl: 0 },
  },
  {
    id: "structure_retest",
    name: "Structure Retest",
    description:
      "A higher-timeframe-confirmed break of market structure is followed by a retest of the break level before continuation.",
    sequenceHuman: "HTF-confirmed MSB → retest of break level",
    status: "candidate",
    live: { n: 1, winRate: 0, avgPnl: -7.0293 },
    backtest: { n: 0, winRate: 0, avgPnl: 0 },
  },
  {
    id: "fvg_continuation",
    name: "FVG Continuation",
    description:
      "A gap-leaving displacement is followed by a same-direction fair value gap that price re-enters while the gap is still active or only partially filled.",
    sequenceHuman: "Displacement (leaves gap) → active/partial-fill FVG retest",
    status: "candidate",
    live: { n: 1, winRate: 1, avgPnl: 1.4757 },
    backtest: { n: 3, winRate: 1, avgPnl: 0 },
  },
];
