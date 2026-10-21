import { describe, expect, it } from "vitest";
import { MODE_LINE_KEY } from "@/pages/Bots/BotNewPage";
import { riskSections } from "@/pages/Risk/RiskPage";

// One build (0.3.19): every install can make a Live bot, so no surface says
// "paper build", and Live readiness is always audited right under the summary.
describe("one build surfaces", () => {
  it("the bot type cards say Paper by default, Live per bot", () => {
    expect(MODE_LINE_KEY).toBe("botCreate.mode.liveBuildLine");
  });

  it("Risk & safety audits the live setup right under the risk summary, before any setting", () => {
    const s = riskSections();
    expect(s.slice(0, 2)).toEqual(["summary", "preflight"]);
    expect(s).toContain("live");
    expect(s.indexOf("preflight")).toBeLessThan(s.indexOf("level"));
  });
});
