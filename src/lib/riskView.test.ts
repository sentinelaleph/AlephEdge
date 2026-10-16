import { describe, expect, it } from "vitest";
import { SAMPLE_SNAPSHOT } from "@/lib/ipc/riskview/riskview.mock";
import type { RiskViewSnapshot } from "@/lib/ipc/riskview/riskview";
import { activeBooks, crossedChecks, lossPct, scenarioAt, worstLevel } from "./riskView";

describe("risk view readers", () => {
  it("picks the scenario and turns its result into a loss share", () => {
    const sc = scenarioAt(SAMPLE_SNAPSHOT.paper, -20);
    expect(sc?.btcMovePct).toBe(-20);
    expect(lossPct(sc)).toBeCloseTo(25.0, 1);
    // A gain is no loss; a missing figure stays missing.
    expect(lossPct(scenarioAt(SAMPLE_SNAPSHOT.paper, 10))).toBe(0);
    expect(lossPct(scenarioAt(SAMPLE_SNAPSHOT.real, -20))).toBeNull();
    expect(scenarioAt(SAMPLE_SNAPSHOT.paper, -50)).toBeNull();
  });

  it("lists only books with exposures", () => {
    expect(activeBooks(SAMPLE_SNAPSHOT)).toEqual(["paper"]);
  });

  it("alerts on danger checks only, real money first", () => {
    const real = { ...SAMPLE_SNAPSHOT.paper, live: true };
    const s: RiskViewSnapshot = { ...SAMPLE_SNAPSHOT, real };
    const crossed = crossedChecks(s);
    expect(crossed.map((c) => `${c.book}:${c.check.key}`)).toEqual([
      "real:sameDirection",
      "real:singleCoin",
      "paper:sameDirection",
      "paper:singleCoin",
    ]);
    expect(crossedChecks(null)).toEqual([]);
    expect(worstLevel(s)).toBe("danger");
    expect(worstLevel({ ...SAMPLE_SNAPSHOT, paper: { ...SAMPLE_SNAPSHOT.paper, checks: [] } })).toBeNull();
  });
});
