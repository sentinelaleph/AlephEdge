import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en/en.json";

/**
 * Every static `t("a.b.c")` / `i18n.t('a.b')` / t(`a.b`) call in src must
 * name a string in en.json (the fallback language): a missing key renders as
 * the raw key in every language. Dynamic keys (`t(\`x.${y}\`)`, `t(key)`) are
 * out of reach of a scan and are not checked here.
 */
const SOURCES = import.meta.glob(["/src/**/*.{ts,tsx}", "!/src/**/*.test.{ts,tsx}", "!/src/__tests__/**"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const CALL = /(?<![\w$])t\(\s*(?:"([^"\\\n]+)"|'([^'\\\n]+)'|`([^`$\\\n]+)`)/g;

function lookup(key: string): unknown {
  return key.split(".").reduce<unknown>((o, k) => (o && typeof o === "object" ? (o as Record<string, unknown>)[k] : undefined), en);
}

function exists(key: string): boolean {
  if (typeof lookup(key) === "string") return true;
  // i18next plural forms: t("x", { count }) reads x_one / x_other.
  return typeof lookup(`${key}_other`) === "string" || typeof lookup(`${key}_one`) === "string";
}

function staticKeys(): { file: string; key: string; line: number }[] {
  const out: { file: string; key: string; line: number }[] = [];
  for (const [file, src] of Object.entries(SOURCES)) {
    for (const m of src.matchAll(CALL)) {
      const key = m[1] ?? m[2] ?? m[3];
      // Only dotted identifiers are i18n keys; t("x") on something else is not.
      if (!/^[A-Za-z0-9_-]+(?:\.[A-Za-z0-9_-]+)+$/.test(key)) continue;
      const line = src.slice(0, m.index).split("\n").length;
      out.push({ file, key, line });
    }
  }
  return out;
}

describe("i18n keys used in src", () => {
  const found = staticKeys();

  it("the scan sees the source tree", () => {
    expect(Object.keys(SOURCES).length).toBeGreaterThan(50);
    expect(found.length).toBeGreaterThan(500);
  });

  it("every static t(\"…\") key exists in en.json", () => {
    const missing = found.filter((f) => !exists(f.key)).map((f) => `${f.file}:${f.line}  ${f.key}`);
    expect(missing, `missing in en.json:\n${missing.join("\n")}`).toEqual([]);
  });
});
