#!/usr/bin/env node
/**
 * Pine-lite engine copy check (docs/aleph-edge-pine-lite-spec.md section 4).
 *
 *   node scripts/pine-sync-check.cjs          compare pine-engine/src with the
 *                                             website engine when it exists
 *   node scripts/pine-sync-check.cjs --write  rewrite pine-engine/SOURCE.sha256
 *                                             from the copy (after a re-sync)
 *
 * The website engine is looked up at ../sentinel-vds-base/frontend/src/lib
 * (a sibling checkout of the website source); elsewhere (CI, a fresh clone)
 * the comparison is skipped with a message. The copy itself is always
 * checked against SOURCE.sha256 by src/__tests__/pine/source.test.ts.
 *
 * Hashes are over the text with CRLF turned into LF: git's autocrlf checks
 * the copy out with CRLF on Windows while the website keeps LF, and the
 * same engine must hash the same on both.
 */
const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");

const ROOT = path.resolve(__dirname, "..");
const COPY = path.join(ROOT, "pine-engine", "src");
const LIST = path.join(ROOT, "pine-engine", "SOURCE.sha256");
const WEBSITE = path.resolve(ROOT, "..", "sentinel-vds-base", "frontend", "src");

/** Relative paths (forward slashes) of every file under `dir`, sorted. */
function listFiles(dir) {
  const out = [];
  const walk = (d) => {
    for (const f of fs.readdirSync(d)) {
      const p = path.join(d, f);
      if (fs.statSync(p).isDirectory()) walk(p);
      else out.push(path.relative(dir, p).split(path.sep).join("/"));
    }
  };
  walk(dir);
  return out.sort();
}

function hashFile(file) {
  const text = fs.readFileSync(file, "utf8").replace(/\r\n/g, "\n");
  return crypto.createHash("sha256").update(text, "utf8").digest("hex");
}

/** SOURCE.sha256 as a Map of relative path -> hash. */
function readList() {
  const map = new Map();
  for (const line of fs.readFileSync(LIST, "utf8").split(/\r?\n/)) {
    const m = /^([0-9a-f]{64}) {2}(.+)$/.exec(line.trim());
    if (m) map.set(m[2], m[1]);
  }
  return map;
}

/** Hashes of the copy, keyed like SOURCE.sha256. */
function copyHashes() {
  return new Map(listFiles(COPY).map((f) => [f, hashFile(path.join(COPY, f))]));
}

/** Differences between two path -> hash maps, as readable lines. */
function diff(expected, actual, what) {
  const lines = [];
  for (const [f, h] of expected) {
    if (!actual.has(f)) lines.push(`missing in ${what}: ${f}`);
    else if (actual.get(f) !== h) lines.push(`differs in ${what}: ${f}`);
  }
  for (const f of actual.keys()) if (!expected.has(f)) lines.push(`extra in ${what}: ${f}`);
  return lines;
}

function writeList() {
  const body = [...copyHashes()].map(([f, h]) => `${h}  ${f}`).join("\n") + "\n";
  fs.writeFileSync(LIST, body);
  console.log(`wrote ${path.relative(ROOT, LIST)}`);
}

function main() {
  if (process.argv.includes("--write")) return writeList();
  const listed = readList();
  const local = diff(listed, copyHashes(), "pine-engine/src (vs SOURCE.sha256)");
  if (!fs.existsSync(path.join(WEBSITE, "lib", "pine"))) {
    console.log(`website engine not found at ${WEBSITE}; skipped the website comparison`);
    return finish(local);
  }
  const website = new Map([...listed.keys()].filter((f) => fs.existsSync(path.join(WEBSITE, f))).map((f) => [f, hashFile(path.join(WEBSITE, f))]));
  const remote = diff(listed, website, "website (vs SOURCE.sha256)");
  const newPine = listFiles(path.join(WEBSITE, "lib", "pine")).map((f) => `lib/pine/${f}`).filter((f) => !listed.has(f));
  return finish([...local, ...remote, ...newPine.map((f) => `new in website, not copied: ${f}`)]);
}

function finish(problems) {
  if (problems.length === 0) {
    console.log("pine engine copy matches SOURCE.sha256" + (fs.existsSync(WEBSITE) ? " and the website engine" : ""));
    return;
  }
  for (const p of problems) console.error(p);
  console.error("re-copy the engine (SOURCE.md), then run with --write");
  process.exitCode = 1;
}

module.exports = { listFiles, hashFile, readList, copyHashes, diff, COPY, LIST };

if (require.main === module) main();
