import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { DUR, EASE } from "./motion";

// Read from disk: Vitest's CSS handling turns a `?raw` CSS import into "".
const TOKENS_CSS = "src/styles/tokens.css";
const CSS = readFileSync(new URL("../styles/tokens.css", import.meta.url), "utf8");

const kebab = (s: string) => s.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);

/** The first (base) value of a custom property; later blocks such as reduced-motion override it. */
function token(name: string): string {
  const m = new RegExp(`--${name}:\\s*([^;]+);`).exec(CSS);
  expect(m, `--${name} not found in ${TOKENS_CSS}`).not.toBeNull();
  return m![1].trim();
}

describe("motion presets match tokens.css", () => {
  it.each(Object.entries(DUR))("DUR.%s equals --dur-*", (key, seconds) => {
    const css = token(`dur-${kebab(key)}`);
    expect(css).toMatch(/^\d+ms$/);
    expect(seconds * 1000).toBeCloseTo(Number.parseFloat(css), 9);
  });

  it.each(Object.entries(EASE))("EASE.%s equals --ease-*", (key, curve) => {
    const m = /^cubic-bezier\(([^)]*)\)$/.exec(token(`ease-${kebab(key)}`));
    expect(m).not.toBeNull();
    expect(m![1].split(",").map((v) => Number(v.trim()))).toEqual([...curve]);
  });
});
