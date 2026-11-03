/**
 * The DCA / Grid strategy form and the Pine bot screens show no English to a
 * user of another language: every user-visible string goes through t(),
 * every key they use exists in all 8 locales, and their labels and help
 * texts are translated (not copies of the English) apart from trading
 * loanwords and code names.
 */
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { PINE_CODES } from "@/lib/ipc/pine/pine";

const DIR = path.join(__dirname);
const LOCALES = path.join(__dirname, "..", "..", "..", "i18n", "locales");
const LANGS = ["en", "tr", "es", "hi", "id", "pt-BR", "ru", "vi"];

const read = (dir: string) =>
  fs
    .readdirSync(dir)
    .filter((f) => /\.tsx?$/.test(f) && !f.includes(".test."))
    .map((f) => ({ file: f, text: fs.readFileSync(path.join(dir, f), "utf8") }));
const sources = read(DIR).filter((s) => s.file.endsWith(".tsx"));
const pineSources = read(path.join(DIR, "..", "pine"));

type Tree = { [k: string]: string | Tree };
function flatten(o: Tree, prefix = "", out: Record<string, string> = {}): Record<string, string> {
  for (const [k, v] of Object.entries(o)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (typeof v === "string") out[key] = v;
    else flatten(v, key, out);
  }
  return out;
}
const locale = (lang: string) => flatten(JSON.parse(fs.readFileSync(path.join(LOCALES, lang, `${lang}.json`), "utf8")) as Tree);

/** Code-only lines (comments dropped) so a comment never counts as UI text. */
function code(text: string): string {
  return text
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .split("\n")
    .map((l) => l.replace(/^\s*\/\/.*$/, ""))
    .join("\n");
}

/** User-visible English written into the source instead of t(). */
function hardCodedEnglish(files: { file: string; text: string }[]): string[] {
  const found: string[] = [];
  for (const { file, text } of files) {
    const c = code(text);
    // JSX text between tags: >Some words< (not `=> Promise<`).
    for (const m of c.matchAll(/(?<![=\w])>\s*([A-Za-z][A-Za-z ,.'!?:-]{2,})\s*</g)) {
      found.push(`${file}: text "${m[1].trim()}"`);
    }
    // String-literal props a user reads (a word with lower case: a symbol
    // example like "BTCUSDT" is not English).
    for (const m of c.matchAll(/\b(label|hint|title|placeholder|aria-label|alt|invalidText)="([^"]*[a-z]{2,}[^"]*)"/g)) {
      found.push(`${file}: ${m[1]}="${m[2]}"`);
    }
  }
  return found;
}

describe("strategy form i18n", () => {
  it("has no hard-coded user-visible English", () => {
    expect(hardCodedEnglish(sources)).toEqual([]);
  });

  it("uses only strategy keys that exist in every locale", () => {
    const keys = new Set<string>();
    for (const { text } of sources) {
      for (const m of text.matchAll(/\bt\(\s*"(strategy\.[\w.]+)"/g)) keys.add(m[1]);
    }
    expect(keys.size).toBeGreaterThan(50);
    for (const lang of LANGS) {
      const loc = locale(lang);
      const missing = [...keys].filter((k) => !(k in loc));
      expect(missing, lang).toEqual([]);
    }
  });

  it("translates the form's labels and help texts", () => {
    const en = locale("en");
    // Words traders read untranslated in these languages.
    const loanwords = new Set(["Bot", "Grid", "Long", "Short", "Take profit", "Stop loss", "Drawdown", "Funding", "Maker", "Taker", "Limit", "Preset", "Margin", "Log", "Simulator", "Neutral", "Base", "Experimental", "Order", "LIVE", "Slippage"]);
    const formKeys = Object.keys(en).filter((k) => /^strategy\.(form|field|hint|warnings|budgetFloor)\./.test(k));
    for (const lang of LANGS.filter((l) => l !== "en")) {
      const loc = locale(lang);
      const english = formKeys.filter((k) => loc[k] === en[k] && /[a-z]{3}/i.test(en[k]) && !loanwords.has(en[k]) && !/^Minimum \{\{min\}\} USDT\.$/.test(en[k]));
      expect(english, lang).toEqual([]);
    }
  });

  it("carries the stop-loss help for longs and shorts and the live cost line", () => {
    for (const lang of LANGS) {
      const loc = locale(lang);
      for (const k of ["strategy.hint.slLong", "strategy.hint.slShort", "strategy.hint.slLine", "strategy.hint.slLineRelative"]) {
        expect(loc[k], `${lang} ${k}`).toBeTruthy();
      }
      expect(loc["strategy.hint.slLine"]).toMatch(/\{\{price\}\}.*\{\{loss\}\}.*\{\{pct\}\}/s);
      expect(loc["strategy.hint.slLineRelative"]).toMatch(/\{\{rel\}\}/);
    }
    expect(locale("en")["strategy.hint.slLong"]).toContain("about $35");
    expect(locale("en")["strategy.hint.slShort"]).toContain("buys back");
  });
});

describe("Pine bot screens i18n", () => {
  it("has no hard-coded user-visible English", () => {
    expect(pineSources.length).toBeGreaterThan(5);
    expect(hardCodedEnglish(pineSources.filter((s) => s.file.endsWith(".tsx")))).toEqual([]);
  });

  it("uses only keys that exist in every locale", () => {
    const keys = new Set<string>();
    for (const { text } of pineSources) {
      for (const m of text.matchAll(/\bt\(\s*"((?:botsPine|botsTv|bots|settings|errors|page|nav)\.[\w.]+)"/g)) keys.add(m[1]);
      for (const m of text.matchAll(/\bk\(\s*"([\w.]+)"\s*\)/g)) keys.add(`botsPine.${m[1]}`);
      for (const m of text.matchAll(/\bcol\(\s*"([\w.]+)"/g)) keys.add(`botsPine.col.${m[1]}`);
    }
    expect(keys.size).toBeGreaterThan(40);
    for (const lang of LANGS) {
      const loc = locale(lang);
      expect([...keys].filter((k) => !(k in loc)), lang).toEqual([]);
    }
  });

  it("translates every Pine code and every Pine screen string", () => {
    const en = locale("en");
    // Code names and trading loanwords read the same in every language.
    const loan = new Set(["Pine", "Pine-lite", "Script", "Stop", "Take", "profit", "Slippage", "Fill", "Pair", "Input", "alert", "alertcondition", "plotshape", "ms", "Timeframe", "KB"]);
    const same = (v: string) => (v.replace(/\{\{\w+\}\}/g, "").match(/[A-Za-z-]+/g) ?? []).every((w) => loan.has(w));
    const keys = [...PINE_CODES.map((c) => `errors.${c}`), ...Object.keys(en).filter((k) => k.startsWith("botsPine."))];
    for (const k of keys) expect(en[k], k).toBeTruthy();
    for (const lang of LANGS.filter((l) => l !== "en")) {
      const loc = locale(lang);
      const english = keys.filter((k) => loc[k] === en[k] && /[a-z]{3}/i.test(en[k]) && !same(en[k]));
      expect(english, lang).toEqual([]);
    }
  });
});
