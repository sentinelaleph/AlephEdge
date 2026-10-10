import { describe, expect, it } from "vitest";
import { createActivityThrottle, TOUCH_INTERVAL_MS } from "./vaultActivity";

describe("vault activity throttle", () => {
  it("reports the first input, then at most once per minute", () => {
    const due = createActivityThrottle();
    expect(due(1_000)).toBe(true);
    // A burst of clicks and keys inside the minute: no further IPC calls.
    for (let t = 1_100; t < 1_000 + TOUCH_INTERVAL_MS; t += 250) expect(due(t)).toBe(false);
    expect(due(1_000 + TOUCH_INTERVAL_MS)).toBe(true);
    expect(due(1_000 + TOUCH_INTERVAL_MS + 1)).toBe(false);
  });

  it("is per desk mount: a new throttle reports at once", () => {
    const a = createActivityThrottle(10);
    a(0);
    expect(createActivityThrottle(10)(1)).toBe(true);
  });
});
