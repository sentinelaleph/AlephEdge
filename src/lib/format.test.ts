import { describe, expect, it } from "vitest";
import {
  formatAge,
  formatDuration,
  formatLatency,
  formatPercent,
  formatPnl,
  formatPrice,
  formatSignedPercent,
  formatSignedUsdt,
  formatTableTime,
  formatUsdt,
  pnlTone,
  pnlToneAttr,
  priceDecimals,
  roundTo,
} from "./format";

const en = "en-US";

describe("pnl sign and tone", () => {
  it("exact zero and -0 are neutral and unsigned", () => {
    expect(formatSignedUsdt(0, en)).toBe("0.00 USDT");
    expect(formatSignedUsdt(-0, en)).toBe("0.00 USDT");
    expect(pnlTone(0)).toBe("flat");
    expect(pnlTone(-0)).toBe("flat");
  });

  it("a value that rounds to zero at the shown precision is zero", () => {
    expect(formatSignedUsdt(-0.004, en)).toBe("0.00 USDT");
    expect(formatSignedPercent(-0.001, en)).toBe("0.00%");
    expect(pnlTone(-0.004)).toBe("flat");
    expect(pnlTone(0.004, 2)).toBe("flat");
    // ...but not at a finer precision.
    expect(pnlTone(-0.004, 3)).toBe("down");
    expect(pnlToneAttr(0.001)).toBeUndefined();
  });

  it("signs and colours real moves", () => {
    expect(formatSignedUsdt(12.4, en)).toBe("+12.40 USDT");
    expect(formatSignedUsdt(-0.04, en)).toBe("-0.04 USDT");
    expect(formatSignedPercent(1.7249, en)).toBe("+1.72%");
    expect(formatSignedPercent(-0.81, en)).toBe("-0.81%");
    expect(pnlTone(0.005)).toBe("up");
    expect(pnlToneAttr(-1)).toBe("down");
  });

  it("formatPnl returns text and tone from one rounding", () => {
    expect(formatPnl(-0.0049, en)).toEqual({ text: "0.00 USDT", tone: "flat" });
    expect(formatPnl(1.977, en, { unit: "percent" })).toEqual({ text: "+1.98%", tone: "up" });
    expect(formatPnl(-3, en, { unit: "none", digits: 0 })).toEqual({ text: "-3", tone: "down" });
    expect(formatPnl(Number.NaN, en).tone).toBe("flat");
  });

  it("roundTo never returns -0", () => {
    expect(Object.is(roundTo(-0.0001, 2), 0)).toBe(true);
  });

  it("follows the locale's marks", () => {
    expect(formatSignedUsdt(-1234.5, "tr-TR")).toBe("-1.234,50 USDT");
  });
});

describe("amounts and percent", () => {
  it("USDT amounts with fixed decimals", () => {
    expect(formatUsdt(1000, en)).toBe("1,000.00 USDT");
    expect(formatUsdt(500, en, 0)).toBe("500 USDT");
    expect(formatUsdt(Number.NaN, en)).toBe("—");
  });

  it("percent with fixed decimals", () => {
    expect(formatPercent(54.8, en)).toBe("54.80%");
    expect(formatPercent(2, en, 0)).toBe("2%");
    expect(formatPercent(-0.001, en)).toBe("0.00%");
  });
});

describe("prices keep their significant digits", () => {
  it("magnitude bands", () => {
    expect(formatPrice(86188.4, en)).toBe("86,188");
    expect(formatPrice(14.44, en)).toBe("14.44");
    expect(formatPrice(0.0511, en)).toBe("0.05110");
    expect(formatPrice(0.5, en)).toBe("0.5000");
  });

  it("tiny prices do not collapse (PEPE entry / TP / SL stay distinct)", () => {
    const entry = formatPrice(0.0000049123, en);
    const tp1 = formatPrice(0.0000051234, en);
    const sl = formatPrice(0.0000046789, en);
    expect(entry).toBe("0.000004912");
    expect(tp1).toBe("0.000005123");
    expect(sl).toBe("0.000004679");
    expect(new Set([entry, tp1, sl]).size).toBe(3);
  });

  it("decimals for at least 4 significant digits below 1", () => {
    expect(priceDecimals(0.000908)).toBe(7);
    expect(priceDecimals(0.99)).toBe(4);
    expect(priceDecimals(1.5)).toBe(2);
    expect(priceDecimals(2500)).toBe(0);
  });
});

describe("time and latency", () => {
  it("age uses the largest whole unit", () => {
    expect(formatAge(45, en)).toBe("45 sec");
    expect(formatAge(111, en)).toBe("1 min");
    expect(formatAge(3599, en)).toBe("59 min");
    expect(formatAge(3600, en)).toBe("1 hr");
    expect(formatAge(2 * 86_400 + 5, en)).toBe("2 days");
    expect(formatAge(-3, en)).toBe("0 sec");
  });

  it("latency in ms, seconds from 1000", () => {
    expect(formatLatency(74, en)).toBe("74 ms");
    expect(formatLatency(1234, en)).toBe("1.2 sec");
  });

  it("table times show the date when not today", () => {
    const now = new Date(2026, 9, 2, 12, 0);
    const today = new Date(2026, 9, 2, 10, 55);
    const yesterday = new Date(2026, 9, 1, 22, 51);
    const lastYear = new Date(2025, 11, 31, 9, 5);
    expect(formatTableTime(today, en, now)).toBe("10:55 AM");
    expect(formatTableTime(yesterday, en, now)).toBe("Oct 1, 10:51 PM");
    expect(formatTableTime(lastYear, en, now)).toBe("Dec 31, 2025, 09:05 AM");
    expect(formatTableTime("not a date", en, now)).toBe("—");
  });
});

describe("formatDuration speaks the user's language when given a locale", () => {
  const ms = (73 * 60 + 45) * 60_000;
  it("keeps the compact form without a locale", () => {
    expect(formatDuration(ms)).toBe("73h45m");
  });
  it("uses the language's own short units", () => {
    const tr = formatDuration(ms, "tr-TR");
    const en = formatDuration(ms, "en-US");
    expect(tr).not.toContain("h45m");
    expect(tr).toMatch(/73/);
    expect(tr).toMatch(/45/);
    expect(en).toMatch(/73 hr/);
    expect(formatDuration(-5, "tr-TR")).toMatch(/0/);
  });
});
