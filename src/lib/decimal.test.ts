import { afterAll, describe, expect, it } from "vitest";
import i18n, { LANGUAGES } from "@/i18n";
import { formatDecimalInput, parseDecimal } from "./decimal";

async function inLanguage(code: string) {
  await i18n.changeLanguage(code);
}

afterAll(async () => {
  await i18n.changeLanguage("en");
});

describe("parseDecimal: English", () => {
  it("reads en grouping and either decimal mark", async () => {
    await inLanguage("en");
    expect(parseDecimal("1,000.5")).toBe(1000.5);
    expect(parseDecimal("1,000")).toBe(1000);
    expect(parseDecimal("1,000,000.25")).toBe(1_000_000.25);
    expect(parseDecimal("1.5")).toBe(1.5);
    expect(parseDecimal("1,5")).toBe(1.5);
    // "." is not en grouping: one dot is always a decimal mark.
    expect(parseDecimal("1.000")).toBe(1);
  });

  it("en-GB resolves to en and still reads a comma decimal", async () => {
    await inLanguage("en-GB");
    expect(i18n.resolvedLanguage).toBe("en");
    expect(parseDecimal("1,5")).toBe(1.5);
    expect(parseDecimal("1,000.5")).toBe(1000.5);
  });
});

describe("parseDecimal: Turkish", () => {
  it("reads tr grouping and either decimal mark", async () => {
    await inLanguage("tr");
    expect(parseDecimal("1.000,5")).toBe(1000.5);
    expect(parseDecimal("1.000")).toBe(1000);
    expect(parseDecimal("1.000.000,25")).toBe(1_000_000.25);
    expect(parseDecimal("1,5")).toBe(1.5);
    expect(parseDecimal("1.5")).toBe(1.5);
    // "," is not tr grouping: one comma is always a decimal mark.
    expect(parseDecimal("1,000")).toBe(1);
  });
});

describe("parseDecimal: Hindi (Indian 2-digit grouping)", () => {
  it("reads lakh/crore grouping and western grouping, rejects stray separators", async () => {
    await inLanguage("hi");
    expect(parseDecimal("10,00,000.5")).toBe(1_000_000.5);
    expect(parseDecimal("1,00,00,000")).toBe(10_000_000);
    expect(parseDecimal("-1,00,000")).toBe(-100_000);
    expect(parseDecimal("1,000,000.5")).toBe(1_000_000.5);
    expect(parseDecimal("1,0,00,000")).toBeNaN();
    expect(parseDecimal("10,00,000.5.5")).toBeNaN();
  });

  it("Indian grouping is not accepted in en", async () => {
    await inLanguage("en");
    expect(parseDecimal("10,00,000")).toBeNaN();
  });
});

describe("parseDecimal: rejects what is not a plain decimal", () => {
  it.each(["", "   ", "1e3", "1E3", "0x10", "abc", "1..5", "1,,5", "1.5.5", "1,5,5", "--1", "+1", "1-", "-", ".", ",", "Infinity", "NaN", "1 2a"])(
    "%j is NaN",
    async (text) => {
      await inLanguage("en");
      expect(parseDecimal(text)).toBeNaN();
    },
  );

  it("mixed marks in the wrong order are not a number in en", async () => {
    await inLanguage("en");
    expect(parseDecimal("1.000,5")).toBeNaN();
  });

  it("mixed marks in the wrong order are not a number in tr", async () => {
    await inLanguage("tr");
    expect(parseDecimal("1,000.5")).toBeNaN();
  });

  it("a misplaced group (not exactly three digits) is not a number", async () => {
    await inLanguage("en");
    expect(parseDecimal("1,00,0.5")).toBeNaN();
    expect(parseDecimal("1,0000.5")).toBeNaN();
    // Not grouping (a leading zero), so the comma is the decimal mark.
    expect(parseDecimal("01,000")).toBe(1);
  });
});

describe("parseDecimal: signs and whitespace", () => {
  it("negatives", async () => {
    await inLanguage("en");
    expect(parseDecimal("-1.5")).toBe(-1.5);
    expect(parseDecimal("-1,5")).toBe(-1.5);
    expect(parseDecimal("-1,000.5")).toBe(-1000.5);
    expect(parseDecimal("-.5")).toBe(-0.5);
    await inLanguage("tr");
    expect(parseDecimal("-1.000,5")).toBe(-1000.5);
  });

  it("leading/trailing and inner spaces, tabs, no-break spaces", async () => {
    await inLanguage("en");
    expect(parseDecimal("  1.5  ")).toBe(1.5);
    expect(parseDecimal("\t2,5\n")).toBe(2.5);
    expect(parseDecimal("1 000.5")).toBe(1000.5);
    expect(parseDecimal("1 000.5")).toBe(1000.5);
    expect(parseDecimal("1 000.5")).toBe(1000.5);
  });

  it("bare fractions and a trailing mark", async () => {
    await inLanguage("en");
    expect(parseDecimal(".5")).toBe(0.5);
    expect(parseDecimal(",5")).toBe(0.5);
    expect(parseDecimal("1.")).toBe(1);
    expect(parseDecimal("0")).toBe(0);
  });
});

/** What each app language groups with, as Intl writes 1,000,000.5 there. */
const GROUPED_MILLION: Record<string, string> = {
  en: "1,000,000.5",
  tr: "1.000.000,5",
  hi: "10,00,000.5",
  vi: "1.000.000,5",
  id: "1.000.000,5",
  ru: "1 000 000,5",
  "pt-BR": "1.000.000,5",
  es: "1.000.000,5",
};

const KNOWN_UNRESOLVED = new Set<string>([]);

describe("all 8 app languages", () => {
  it("the table covers exactly the app's languages", () => {
    expect(Object.keys(GROUPED_MILLION).sort()).toEqual(LANGUAGES.map((l) => l.code).sort());
  });

  describe.each(LANGUAGES.map((l) => l.code))("%s", (code) => {
    // KNOWN BUG (src/i18n/index.ts): `nonExplicitSupportedLngs: true` checks
    // "pt" against supportedLngs, which lists only "pt-BR", so pt-BR resolves
    // to English and the parser uses en-US grouping ("1.000,5" -> NaN). Drop
    // pt-BR from this set once that is fixed; `it.fails` will go red to say so.
    const known = KNOWN_UNRESOLVED.has(code) ? it.fails : it;
    known("reads the language's own Intl output back", async () => {
      await inLanguage(code);
      expect(i18n.resolvedLanguage).toBe(code);
      const locale = LANGUAGES.find((l) => l.code === code)!.locale;
      for (const n of [1000.5, 1_000_000.5, 1234567.25, -98765.125, 12.5]) {
        const shown = new Intl.NumberFormat(locale, { maximumFractionDigits: 10 }).format(n);
        expect(parseDecimal(shown), `${code}: ${JSON.stringify(shown)}`).toBe(n);
      }
      expect(parseDecimal(GROUPED_MILLION[code])).toBe(1_000_000.5);
    });

    it("reads a comma or dot decimal with no grouping", async () => {
      await inLanguage(code);
      expect(parseDecimal("1,5")).toBe(1.5);
      expect(parseDecimal("1.5")).toBe(1.5);
      expect(parseDecimal("0,25")).toBe(0.25);
      expect(parseDecimal("1e3")).toBeNaN();
      expect(parseDecimal("")).toBeNaN();
    });

    it("formatDecimalInput round-trips", async () => {
      await inLanguage(code);
      for (const n of [0, 1, 1.5, 1.125, -2.75, 1000, 1234567.891, 0.0001, 0.1 + 0.2, 123456789.123, 1e-7, 2.5e-9, 1e21, -3.4e-8]) {
        const text = formatDecimalInput(n);
        expect(text, `${code} ${n}`).not.toMatch(/e/i);
        expect(parseDecimal(text), `${code}: ${n} -> ${JSON.stringify(text)}`).toBe(n);
      }
    });
  });
});

describe("formatDecimalInput", () => {
  it("writes the language's decimal mark, never grouping", async () => {
    await inLanguage("en");
    expect(formatDecimalInput(1234.5)).toBe("1234.5");
    await inLanguage("tr");
    expect(formatDecimalInput(1234.5)).toBe("1234,5");
    expect(formatDecimalInput(1.125)).toBe("1,125");
  });

  it("empty for null, undefined and non-finite", async () => {
    await inLanguage("en");
    expect(formatDecimalInput(null)).toBe("");
    expect(formatDecimalInput(undefined)).toBe("");
    expect(formatDecimalInput(Number.NaN)).toBe("");
    expect(formatDecimalInput(Number.POSITIVE_INFINITY)).toBe("");
  });

  it("tiny and huge numbers are written out, not in exponent form", async () => {
    await inLanguage("en");
    expect(formatDecimalInput(1.23e-7)).toBe("0.000000123");
    expect(formatDecimalInput(-5e-7)).toBe("-0.0000005");
    expect(formatDecimalInput(1e21)).toBe("1000000000000000000000");
    expect(formatDecimalInput(1.5e22)).toBe("15000000000000000000000");
    await inLanguage("tr");
    expect(formatDecimalInput(1.23e-7)).toBe("0,000000123");
  });
});
