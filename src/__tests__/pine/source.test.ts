/**
 * The engine copy in pine-engine/src must equal SOURCE.sha256 (the website
 * engine it was copied from, SOURCE.md). A local edit to the copy fails
 * here; a website change is caught by `npm run pine:sync` on a PC that has
 * the website checkout next to this repo.
 */
import { createRequire } from "node:module";
import { describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);
const sync = require("../../../scripts/pine-sync-check.cjs") as {
  readList(): Map<string, string>;
  copyHashes(): Map<string, string>;
  diff(expected: Map<string, string>, actual: Map<string, string>, what: string): string[];
};

describe("pine engine copy", () => {
  it("matches SOURCE.sha256 file by file", () => {
    const listed = sync.readList();
    expect(listed.size).toBeGreaterThan(30);
    expect(sync.diff(listed, sync.copyHashes(), "pine-engine/src")).toEqual([]);
  });
});
