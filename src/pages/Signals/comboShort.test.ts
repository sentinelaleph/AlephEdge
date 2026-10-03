import { describe, expect, it } from "vitest";
import { comboShort } from "./SignalsPage";

describe("comboShort", () => {
  it("keeps one or two sources whole", () => {
    expect(comboShort("stophunt_snap")).toBe("stophunt_snap");
    expect(comboShort("ob + fvg")).toBe("ob + fvg");
  });
  it("never cuts a source name: two names and +N", () => {
    expect(comboShort("ob + fvg + choch + obv")).toBe("ob + fvg +2");
  });
});
