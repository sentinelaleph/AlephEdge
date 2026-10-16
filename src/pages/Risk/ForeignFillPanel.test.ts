import { describe, expect, it } from "vitest";
import type { ForeignFillStatus } from "@/lib/ipc/risk/foreignFill";
import { foreignFillState } from "./ForeignFillPanel";

const base: ForeignFillStatus = { liveBuild: true, enabled: true, watching: true, lastCheckMs: 1, lastError: null, alarms: [] };
const fill = { symbol: "XUSDT", tradeId: 1, orderId: 2, side: "BUY", qty: 1, price: 1, timeMs: 1, detectedMs: 2 };

describe("foreign-trade watch state", () => {
  it("an alarm wins over every other state, even with the watch switched off", () => {
    expect(foreignFillState({ ...base, alarms: [fill] })).toEqual({ status: "error", key: "alarm" });
    expect(foreignFillState({ ...base, enabled: false, alarms: [fill] }).key).toBe("alarm");
  });

  it("off, watching, and switched on but not checking (key locked) read apart", () => {
    expect(foreignFillState({ ...base, enabled: false }).key).toBe("off");
    expect(foreignFillState(base)).toEqual({ status: "ok", key: "watching" });
    expect(foreignFillState({ ...base, watching: false })).toEqual({ status: "needsSetup", key: "idle" });
  });
});
