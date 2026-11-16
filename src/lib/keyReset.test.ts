import { describe, expect, it } from "vitest";
import { isKeyResetHeld } from "./keyReset";

describe("a live row opened before a vault reset is held", () => {
  it("holds live rows opened at or before the reset, nothing else", () => {
    expect(isKeyResetHeld({ live: true, openedAt: 1_000 }, 2_000)).toBe(true);
    expect(isKeyResetHeld({ live: true, openedAt: 2_000 }, 2_000)).toBe(true);
    expect(isKeyResetHeld({ live: true, openedAt: 2_001 }, 2_000)).toBe(false);
    expect(isKeyResetHeld({ live: false, openedAt: 1_000 }, 2_000)).toBe(false);
    expect(isKeyResetHeld({ live: true, openedAt: 1_000 }, 0)).toBe(false);
    expect(isKeyResetHeld({ live: true, openedAt: 1_000 }, undefined)).toBe(false);
  });
});
