import { describe, expect, it } from "vitest";
import {
  DEFAULT_RISK_PCT,
  estimateRiskSize,
  EXAMPLE_STOP_FRACS,
  FUNDING_COST_FRAC,
  FUTURES_FEE_RATE,
  MAX_RISK_PCT,
  maxLossEffect,
  MIN_RISK_PCT,
  ROUND_TRIP_COST_FRAC,
  ROUND_TRIP_FEE_FRAC,
  SPOT_FEE_RATE,
} from "./sizing";
import { customPctValid, MAX_CUSTOM_TP_PCT, MIN_CUSTOM_TP_PCT, signalTpDistances, targetLabel } from "./takeProfit";
import { capitalAboveCap, DEFAULT_CAPITAL, defaultCapitalWithin, MAX_BOT_POSITIONS } from "./botLimits";
import type { Signal } from "@/lib/ipc/signal/signal";

/** Rust sources the TS constants are kept in lockstep with by hand. */
const RUST = import.meta.glob(["/src-tauri/src/bot/engine/sizing.rs", "/src-tauri/src/bot/model.rs", "/src-tauri/src/bot/commands.rs"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

function rustConst(file: string, name: string): number {
  const src = RUST[file];
  expect(src, `${file} not found`).toBeTypeOf("string");
  const m = new RegExp(`pub const ${name}: \\w+ = ([0-9_.]+);`).exec(src);
  expect(m, `${name} not found in ${file}`).not.toBeNull();
  return Number(m![1].replace(/_/g, ""));
}

const SIZING_RS = "/src-tauri/src/bot/engine/sizing.rs";
const MODEL_RS = "/src-tauri/src/bot/model.rs";
const COMMANDS_RS = "/src-tauri/src/bot/commands.rs";

/** A private `const NAME: f64 = …;` (rustConst reads only `pub const`). */
function rustPrivateConst(file: string, name: string): number {
  const m = new RegExp(`const ${name}: \\w+ = ([0-9_.]+);`).exec(RUST[file]);
  expect(m, `${name} not found in ${file}`).not.toBeNull();
  return Number(m![1].replace(/_/g, ""));
}

describe("constants match the Rust side", () => {
  it("sizing.rs", () => {
    expect(ROUND_TRIP_COST_FRAC).toBeCloseTo(rustConst(SIZING_RS, "ROUND_TRIP_COST_FRAC"), 12);
    expect(MIN_RISK_PCT).toBe(rustConst(SIZING_RS, "MIN_RISK_PCT"));
    expect(MAX_RISK_PCT).toBe(rustConst(SIZING_RS, "MAX_RISK_PCT"));
    expect(DEFAULT_RISK_PCT).toBe(rustConst(SIZING_RS, "DEFAULT_RISK_PCT"));
  });

  it("model.rs", () => {
    expect(MIN_CUSTOM_TP_PCT).toBe(rustConst(MODEL_RS, "MIN_CUSTOM_TP_PCT"));
    expect(MAX_CUSTOM_TP_PCT).toBe(rustConst(MODEL_RS, "MAX_CUSTOM_TP_PCT"));
    expect(MAX_BOT_POSITIONS).toBe(rustConst(MODEL_RS, "MAX_BOT_POSITIONS"));
    // 0.05% taker per side = the 0.10% round trip the example charges.
    expect(ROUND_TRIP_FEE_FRAC).toBeCloseTo(2 * rustConst(MODEL_RS, "FUTURES_FEE_RATE"), 12);
    expect(FUTURES_FEE_RATE).toBe(rustConst(MODEL_RS, "FUTURES_FEE_RATE"));
    expect(SPOT_FEE_RATE).toBe(rustConst(MODEL_RS, "SPOT_FEE_RATE"));
  });

  it("commands.rs", () => {
    expect(DEFAULT_CAPITAL).toBe(rustPrivateConst(COMMANDS_RS, "DEFAULT_CAPITAL"));
  });

  it("the cost is fees + funding", () => {
    expect(ROUND_TRIP_COST_FRAC).toBeCloseTo(ROUND_TRIP_FEE_FRAC + FUNDING_COST_FRAC, 12);
    expect(ROUND_TRIP_COST_FRAC).toBeCloseTo(0.0012, 12);
  });
});

// Mirrors src-tauri/src/bot/engine/sizing_tests.rs.
describe("estimateRiskSize (mirror of size_position, risk mode)", () => {
  it("narrow and wide stops lose the same share of capital (1% of 1000 at x20)", () => {
    for (const stop of [0.03, 0.07]) {
      const s = estimateRiskSize(1_000, 20, 1, stop);
      expect(s.riskCapped).toBe(false);
      expect(s.costUsdt).toBeCloseTo(10, 9);
      expect(s.costPctOfCapital).toBeCloseTo(1, 9);
      expect(s.effectiveLeverage).toBeCloseTo(s.notional / 1_000, 12);
    }
    const narrow = estimateRiskSize(1_000, 20, 1, 0.03);
    const wide = estimateRiskSize(1_000, 20, 1, 0.07);
    expect(narrow.notional).toBeGreaterThan(2 * wide.notional);
    expect(wide.effectiveLeverage).toBeLessThan(1.5);
  });

  it("the leverage cap binds and says so (5% risk, 0.5% stop, x3)", () => {
    const s = estimateRiskSize(1_000, 3, 5, 0.005);
    expect(s.riskCapped).toBe(true);
    expect(s.notional).toBe(3_000);
    expect(s.effectiveLeverage).toBe(3);
    expect(s.costUsdt).toBeLessThan(50);
    expect(s.costPctOfCapital).toBeLessThan(5);
  });

  it("exact arithmetic: notional = risk / (stop + cost)", () => {
    const s = estimateRiskSize(500, 10, 2, 0.04);
    expect(s.notional).toBeCloseTo(10 / (0.04 + 0.0012), 9);
  });

  it("a zero stop or zero capital sizes nothing instead of dividing by zero", () => {
    expect(estimateRiskSize(1_000, 5, 1, 0)).toMatchObject({ notional: 0, riskCapped: false, effectiveLeverage: 0 });
    expect(estimateRiskSize(0, 5, 1, 0.03)).toMatchObject({ notional: 0, effectiveLeverage: 0, costPctOfCapital: 0 });
  });

  it("clamps the declared risk like size_position does", () => {
    expect(estimateRiskSize(1_000, 50, 12, 0.03).costPctOfCapital).toBeCloseTo(MAX_RISK_PCT, 9);
    expect(estimateRiskSize(1_000, 50, 0.01, 0.03).costPctOfCapital).toBeCloseTo(MIN_RISK_PCT, 9);
  });

  it("the worked example uses the ledger's 3% and 7% stops", () => {
    expect([...EXAMPLE_STOP_FRACS]).toEqual([0.03, 0.07]);
  });
});

describe("takeProfit helpers", () => {
  it("custom percent bounds are inclusive and finite", () => {
    expect(customPctValid(MIN_CUSTOM_TP_PCT)).toBe(true);
    expect(customPctValid(MAX_CUSTOM_TP_PCT)).toBe(true);
    expect(customPctValid(40)).toBe(true);
    expect(customPctValid(0.09)).toBe(false);
    expect(customPctValid(1000.01)).toBe(false);
    expect(customPctValid(Number.NaN)).toBe(false);
    expect(customPctValid(Number.POSITIVE_INFINITY)).toBe(false);
  });

  it("signal TP distances are % from entry, both directions, skipping junk", () => {
    const sig = (entry: number, tp: number[]) => ({ entry, tp }) as unknown as Signal;
    const long = signalTpDistances(sig(100, [104, 107, 117]));
    expect(long.map((d) => +d.toFixed(9))).toEqual([4, 7, 17]);
    const short = signalTpDistances(sig(200, [190, 180]));
    expect(short.map((d) => +d.toFixed(9))).toEqual([5, 10]);
    expect(signalTpDistances(sig(100, [0, Number.NaN, 110]))).toEqual([10]);
    expect(signalTpDistances(sig(0, [110]))).toEqual([]);
  });

  it("target labels", () => {
    expect(targetLabel(null, "Custom")).toBe("TP1");
    expect(targetLabel(undefined, "Custom")).toBe("TP1");
    expect(targetLabel("", "Custom")).toBe("TP1");
    expect(targetLabel("tp3", "Custom")).toBe("TP3");
    expect(targetLabel("custom:40", "Özel")).toBe("Özel +40%");
  });
});

// Mirrors pnl.rs `breaches_cap` (audit 2026-10-08, signal_bots 4): one number
// whose meaning depends on the sizing mode, now stated under the field.
describe("maxLossEffect", () => {
  const futures = { feeRate: FUTURES_FEE_RATE, riskPct: 1 };
  it("fixed sizing: the price move that reaches the cap", () => {
    const e = maxLossEffect({ ...futures, sizing: "fixed", capPct: 2, leverage: 3 });
    expect(e?.kind).toBe("fixed");
    expect(e?.kind === "fixed" && e.movePct).toBeCloseTo(2 / 3 - 0.1, 9);
    const spot = maxLossEffect({ sizing: "fixed", capPct: 2, leverage: 1, riskPct: 1, feeRate: SPOT_FEE_RATE });
    expect(spot?.kind === "fixed" && spot.movePct).toBeCloseTo(1.8, 9);
  });
  it("fixed sizing: fees alone can reach a small cap at high leverage", () => {
    expect(maxLossEffect({ ...futures, sizing: "fixed", capPct: 1, leverage: 20 })).toEqual({ kind: "immediate" });
  });
  it("risk sizing: a cap at or above the risk never fires, below it fires before the stop", () => {
    expect(maxLossEffect({ ...futures, sizing: "risk", capPct: 2, leverage: 3, riskPct: 1 })).toEqual({ kind: "riskInert" });
    expect(maxLossEffect({ ...futures, sizing: "risk", capPct: 1, leverage: 3, riskPct: 1 })).toEqual({ kind: "riskInert" });
    expect(maxLossEffect({ ...futures, sizing: "risk", capPct: 1, leverage: 3, riskPct: 2 })).toEqual({ kind: "riskActive", stopShare: 0.5 });
  });
  it("empty or nonsense is off, as in Rust", () => {
    expect(maxLossEffect({ ...futures, sizing: "fixed", capPct: 0, leverage: 3 })).toBeNull();
    expect(maxLossEffect({ ...futures, sizing: "fixed", capPct: Number.NaN, leverage: 3 })).toBeNull();
  });
});

// Mirrors bot/commands.rs `check_capital_cap` / `default_capital` (audit
// 2026-10-08, signal_bots 1): the fresh-install default must fit its cap.
describe("capital per position against the risk level's cap", () => {
  it("Cautious with a 1000 USDT balance caps a position at 20 USDT", () => {
    expect(defaultCapitalWithin(20)).toBe(20);
    expect(capitalAboveCap(100, 20)).toBe(true);
    expect(capitalAboveCap(20, 20)).toBe(false);
  });
  it("keeps 100 when the cap allows it, never exceeds the cap", () => {
    expect(defaultCapitalWithin(200)).toBe(100);
    expect(defaultCapitalWithin(60.4)).toBe(60);
    expect(defaultCapitalWithin(0.456)).toBe(0.45);
  });
  it("an unknown cap is not a refusal", () => {
    expect(capitalAboveCap(1_000_000, null)).toBe(false);
    expect(defaultCapitalWithin(null)).toBe(DEFAULT_CAPITAL);
  });
});
