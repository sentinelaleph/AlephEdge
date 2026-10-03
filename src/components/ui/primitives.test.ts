import { describe, expect, it } from "vitest";
import { foldOrder, maxPriority, splitColumns } from "./DataTable/columns";
import { nextRowIndex, rowActivationKey } from "./DataTable/rowKeys";
import { levelsFor, partsWidth, pickLevel } from "./PageShell/actionLevel";
import { balancedColumns } from "./KpiGrid/balance";
import { STATUS_TONE, statusLabelKey, statusTone, type StatusKind } from "./StatusChip/statusTone";
import en from "@/i18n/locales/en/en.json";

describe("DataTable column priorities", () => {
  const cols = [
    { id: "name" },
    { id: "state", priority: 1 as const },
    { id: "market", priority: 2 as const },
    { id: "cycles", priority: 3 as const },
  ];

  it("shows everything before the first measurement", () => {
    expect(maxPriority(null)).toBe(3);
    expect(splitColumns(cols, null).hidden).toEqual([]);
  });

  it("folds priority 3, then 2, as the container narrows", () => {
    expect(splitColumns(cols, 1000).hidden.map((c) => c.id)).toEqual([]);
    expect(splitColumns(cols, 800).hidden.map((c) => c.id)).toEqual(["cycles"]);
    expect(splitColumns(cols, 500).hidden.map((c) => c.id)).toEqual(["market", "cycles"]);
    expect(splitColumns(cols, 500).visible.map((c) => c.id)).toEqual(["name", "state"]);
  });

  it("honours custom breakpoints", () => {
    expect(maxPriority(700, { p2: 400, p3: 700 })).toBe(3);
    expect(maxPriority(699, { p2: 400, p3: 700 })).toBe(2);
  });

  it("detailOnly columns always live in the expander, at any width", () => {
    const withDetail = [...cols, { id: "details", detailOnly: true }];
    expect(splitColumns(withDetail, null).hidden.map((c) => c.id)).toEqual(["details"]);
    expect(splitColumns(withDetail, 5000).visible.map((c) => c.id)).toEqual(["name", "state", "market", "cycles"]);
    expect(splitColumns(withDetail, 500).hidden.map((c) => c.id)).toEqual(["market", "cycles", "details"]);
  });

  it("fit: folds lowest priority first, rightmost first, never the first or a keep column", () => {
    const wide = [
      { id: "time" },
      { id: "symbol", keep: true },
      { id: "side" },
      { id: "entry" },
      { id: "tp2", priority: 2 as const },
      { id: "state" },
    ];
    expect(foldOrder(wide).map((c) => c.id)).toEqual(["tp2", "state", "entry", "side"]);
    const one = splitColumns(wide, 2000, undefined, 1);
    expect(one.visible.map((c) => c.id)).toEqual(["time", "symbol", "side", "entry", "state"]);
    expect(one.hidden.map((c) => c.id)).toEqual(["tp2"]);
    const three = splitColumns(wide, 2000, undefined, 3);
    expect(three.visible.map((c) => c.id)).toEqual(["time", "symbol", "side"]);
    // Hidden keep the table's own order in the expander.
    expect(three.hidden.map((c) => c.id)).toEqual(["entry", "tp2", "state"]);
    expect(three.canFold).toBe(true);
    const all = splitColumns(wide, 2000, undefined, 99);
    expect(all.visible.map((c) => c.id)).toEqual(["time", "symbol"]);
    expect(all.canFold).toBe(false);
  });

  it("fit folds only what the priorities left visible", () => {
    // At 500 px priority 2/3 are already hidden; fold 1 takes the rightmost priority-1 column.
    expect(splitColumns(cols, 500, undefined, 1).visible.map((c) => c.id)).toEqual(["name"]);
    expect(splitColumns(cols, 500, undefined, 1).hidden.map((c) => c.id)).toEqual(["state", "market", "cycles"]);
  });
});

describe("DataTable row keyboard", () => {
  it("Enter and Space activate; other keys do not", () => {
    expect(rowActivationKey("Enter")).toBe(true);
    expect(rowActivationKey(" ")).toBe(true);
    expect(rowActivationKey("a")).toBe(false);
    expect(rowActivationKey("Tab")).toBe(false);
  });

  it("arrows, Home and End move within the rows and stop at the ends", () => {
    expect(nextRowIndex("ArrowDown", 0, 3)).toBe(1);
    expect(nextRowIndex("ArrowDown", 2, 3)).toBe(2);
    expect(nextRowIndex("ArrowUp", 0, 3)).toBe(0);
    expect(nextRowIndex("ArrowUp", 2, 3)).toBe(1);
    expect(nextRowIndex("Home", 2, 3)).toBe(0);
    expect(nextRowIndex("End", 0, 3)).toBe(2);
    expect(nextRowIndex("Enter", 0, 3)).toBeNull();
    expect(nextRowIndex("ArrowDown", 0, 0)).toBeNull();
  });
});

describe("PageShell header action levels", () => {
  const parts = { secondary: 300, primary: 90, more: 30, gap: 8 };

  it("keeps everything inline while the title has its natural width", () => {
    expect(pickLevel(800, 0, 200, parts)).toBe(0);
  });

  it("moves the secondary actions into More first, then the primary", () => {
    // level 0 needs 200 + 308 + 98 = 606; level 1 needs 200 + 38 + 98 = 336; level 2 needs 238.
    expect(partsWidth(0, parts)).toBe(406);
    expect(partsWidth(1, parts)).toBe(136);
    expect(partsWidth(2, parts)).toBe(38);
    expect(pickLevel(605, 0, 200, parts)).toBe(1);
    expect(pickLevel(336, 0, 200, parts)).toBe(1);
    expect(pickLevel(335, 0, 200, parts)).toBe(2);
    // Nothing fits (a phone): the highest level, the title may wrap.
    expect(pickLevel(100, 0, 200, parts)).toBe(2);
  });

  it("counts the panel toggle as fixed width", () => {
    expect(pickLevel(606, 0, 200, parts)).toBe(0);
    expect(pickLevel(606, 40, 200, parts)).toBe(1);
  });

  it("skips levels a page cannot use", () => {
    expect(levelsFor({ secondary: 0, primary: 90 })).toEqual([0, 2]);
    expect(levelsFor({ secondary: 120, primary: 0 })).toEqual([0, 1]);
    expect(pickLevel(150, 0, 200, { secondary: 0, primary: 90, more: 30, gap: 8 })).toBe(2);
    expect(pickLevel(150, 0, 200, { secondary: 120, primary: 0, more: 30, gap: 8 })).toBe(1);
  });
});

describe("KpiGrid balanced columns", () => {
  it("no orphan row: 6 tiles in room for 4 become 3 + 3", () => {
    // 4 x 160 + 3 x 8 = 664 fits 4.
    expect(balancedColumns(6, 700, 160, 8)).toBe(3);
  });

  it("5 tiles in room for 4 become 3 + 2, 7 in room for 4 become 4 + 3", () => {
    expect(balancedColumns(5, 700, 160, 8)).toBe(3);
    expect(balancedColumns(7, 700, 160, 8)).toBe(4);
  });

  it("one row when everything fits; one column when nothing does", () => {
    expect(balancedColumns(4, 2000, 160, 8)).toBe(4);
    expect(balancedColumns(3, 100, 160, 8)).toBe(1);
    expect(balancedColumns(0, 700, 160, 8)).toBe(1);
  });
});

describe("StatusChip tone map", () => {
  it("not configured / needs setup are warnings, never positive", () => {
    expect(statusTone("notConfigured")).toBe("warning");
    expect(statusTone("needsSetup")).toBe("warning");
  });

  it("fixed tones for run states", () => {
    expect(statusTone("running")).toBe("positive");
    expect(statusTone("stopped")).toBe("neutral");
    expect(statusTone("idle")).toBe("neutral");
    expect(statusTone("error")).toBe("negative");
    expect(statusTone("dead")).toBe("negative");
  });

  it("every state has a sentence-case English label", () => {
    const status = (en as unknown as { status: Record<string, string> }).status;
    for (const kind of Object.keys(STATUS_TONE) as StatusKind[]) {
      const key = statusLabelKey(kind).split(".")[1];
      const label = status[key];
      expect(label, kind).toBeTypeOf("string");
      // Sentence case: not ALL CAPS beyond an acronym like "OK".
      if (label.length > 2) expect(label, kind).not.toBe(label.toUpperCase());
    }
  });
});
