import { beforeEach, describe, expect, it } from "vitest";
import { resetRunStore, runStore } from "./backtestRunStore";

describe("backtest run store", () => {
  beforeEach(resetRunStore);

  it("holds one run at a time and survives a page remount", () => {
    expect(runStore.begin("rt-a")).toBe(true);
    expect(runStore.begin("rt-b")).toBe(false);
    // the page is left and reopened: the pending run is still shown
    const unmount = runStore.mount();
    unmount();
    expect(runStore.pageMounted()).toBe(false);
    expect(runStore.get().flight?.token).toBe("rt-a");
    runStore.progress("rt-a", { runToken: "rt-a", done: 2, total: 5 });
    runStore.progress("rt-b", { runToken: "rt-b", done: 9, total: 9 });
    expect(runStore.get().flight?.progress).toEqual({ runToken: "rt-a", done: 2, total: 5 });
    runStore.end("rt-b", "historyNetwork");
    expect(runStore.get().flight?.token).toBe("rt-a");
    runStore.end("rt-a", "historyEmpty");
    expect(runStore.get()).toEqual({ flight: null, error: "historyEmpty", finished: 1 });
    expect(runStore.begin("rt-c")).toBe(true);
    expect(runStore.get().error).toBeNull();
  });
});
