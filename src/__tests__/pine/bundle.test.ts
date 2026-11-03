/**
 * src-tauri/src/pine/engine.bundle.js is what the Rust runner executes. It
 * must be a fresh build of pine-engine/ (`npm run pine:bundle`) and carry no
 * dynamic code paths (spec section 3: no eval, no Function). The Rust parity
 * fixtures (src-tauri/src/pine/fixtures/parity.json) must come from it.
 */
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);
const bundler = require("../../../scripts/pine-bundle.cjs") as { buildBundle(): Promise<string>; OUT: string };
const fixtures = require("../../../scripts/pine-fixtures.cjs") as { buildFixtures(): string; OUT: string };

const committed = () => readFileSync(bundler.OUT, "utf8").replace(/\r\n/g, "\n");

describe("pine engine bundle", () => {
  it("equals a fresh build (run `npm run pine:bundle` after changing pine-engine/)", async () => {
    expect(committed() === (await bundler.buildBundle())).toBe(true);
  }, 30_000);

  it("parity fixtures for the Rust runner come from this bundle (`npm run pine:fixtures`)", () => {
    const committedFixtures = readFileSync(fixtures.OUT, "utf8").replace(/\r\n/g, "\n");
    expect(committedFixtures === fixtures.buildFixtures()).toBe(true);
  }, 30_000);

  it("has no eval, Function constructor or dynamic import", () => {
    const text = committed();
    expect(/\beval\s*\(/.test(text)).toBe(false);
    expect(/new\s+Function\b/.test(text)).toBe(false);
    expect(/\bFunction\s*\(/.test(text)).toBe(false);
    expect(/\bimport\s*\(/.test(text)).toBe(false);
  });
});
