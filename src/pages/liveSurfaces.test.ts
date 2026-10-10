import { describe, expect, it } from "vitest";
import { modeKey } from "@/pages/Bots/BotNewPage";
import { riskSections } from "@/pages/Risk/RiskPage";

// The public build places no real orders: it must not advertise a LIVE
// opt-in or audit a real-money setup it cannot have (audit 2026-10-08,
// ux_global 4). A live build keeps both.
describe("paper build surfaces", () => {
  it("the bot type cards say paper only, without a LIVE opt-in", () => {
    expect(modeKey(false)).toBe("botCreate.mode.paperOnly");
    expect(modeKey(true)).toBe("botCreate.mode.signal");
  });

  it("Risk & safety has no Live readiness section in the paper build", () => {
    expect(riskSections(false)).not.toContain("preflight");
    expect(riskSections(false)).toContain("live");
    expect(riskSections(true)[0]).toBe("preflight");
  });
});
