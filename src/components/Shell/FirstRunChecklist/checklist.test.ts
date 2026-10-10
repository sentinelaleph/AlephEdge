import { describe, expect, it } from "vitest";
import type { StrategyBotView } from "@/lib/ipc/strategy/strategy";
import { checklistState, FIRST_PRESET_PATH } from "./checklist";

const NO_SIGNAL = { futures: null, spot: null, pump: null, futuresRunning: false, spotRunning: false, pumpRunning: false };

const bot = (over: Partial<StrategyBotView> = {}): StrategyBotView =>
  ({
    id: "sb_1",
    kind: "dca",
    runState: "armed",
    acceptingNewCycles: true,
    openCycle: null,
    cyclesDone: 0,
    ...over,
  }) as StrategyBotView;

const keys = (s: ReturnType<typeof checklistState>) => s.steps.map((st) => st.key);

describe("first-run checklist", () => {
  it("a DCA-only desk with a running bot is done and can be hidden", () => {
    // Audit 2026-10-08: a user running DCA Long Classic saw "2 of 4" forever.
    const s = checklistState(NO_SIGNAL, [bot()], false);
    expect(s.allDone).toBe(true);
    expect(s.botConfigured).toBe(true);
    expect(keys(s)).toEqual(["signedIn", "botConfigured", "botRunning"]);
  });

  it("a bot holding an open cycle counts as running after Pause", () => {
    const s = checklistState(NO_SIGNAL, [bot({ acceptingNewCycles: false, runState: "stopped", openCycle: {} as StrategyBotView["openCycle"] })], false);
    expect(s.steps.find((st) => st.key === "botRunning")?.done).toBe(true);
  });

  it("an empty desk points at the template that passed its test", () => {
    const s = checklistState(NO_SIGNAL, [], false);
    expect(s.allDone).toBe(false);
    expect(s.botConfigured).toBe(false);
    const configure = s.steps.find((st) => st.key === "botConfigured");
    expect(configure?.to).toBe(FIRST_PRESET_PATH);
    expect(configure?.to).toContain("preset=dca_long_classic");
    expect(configure?.hint).toBe("firstPreset");
    expect(s.steps.find((st) => st.key === "botRunning")?.hint).toBeUndefined();
  });

  it("a stopped DCA bot links its run step to the DCA list", () => {
    const s = checklistState(NO_SIGNAL, [bot({ acceptingNewCycles: false, runState: "stopped" })], true);
    const run = s.steps.find((st) => st.key === "botRunning");
    expect(run?.done).toBe(false);
    expect(run?.to).toBe("/bots/dca");
    expect(s.doneCount).toBe(2);
  });

  it("the stream step appears once a signal bot needs it", () => {
    const signal = { ...NO_SIGNAL, futures: {} as never, futuresRunning: true };
    const waiting = checklistState(signal, [], false);
    expect(keys(waiting)).toEqual(["signedIn", "streamConnected", "botConfigured", "botRunning"]);
    expect(waiting.allDone).toBe(false);
    expect(checklistState(signal, [], true).allDone).toBe(true);
    expect(waiting.steps.find((st) => st.key === "botRunning")?.to).toBe("/bots/signal");
  });

  it("nothing is counted before the strategy list loads", () => {
    expect(checklistState(NO_SIGNAL, null, true).botConfigured).toBe(false);
  });
});
