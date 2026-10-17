#!/usr/bin/env node
/**
 * Fails when the release bundle in dist/ carries a dev mock or the dev
 * screenshot harness. Run after `vite build` (`npm run build` does it;
 * `npm run build:check` runs it alone on an existing dist/).
 *
 * Two independent checks:
 *  - file names: a chunk named after a mock (`x.mock-HASH.js`) or the harness;
 *  - contents: the side-effect marker the `aleph:strip-dev-mocks` plugin in
 *    vite.config.ts puts into every mock / src/dev module during a build, plus
 *    leftover references to `.mock` modules or the harness.
 */
const fs = require("fs");
const path = require("path");

const MARKER = "__ALEPH_DEV_MOCK__"; // keep in sync with vite.config.ts
const dist = path.resolve(process.argv[2] || path.join(__dirname, "..", "dist"));

const BAD_NAME = /(^|[\\/._-])mock([._-]|$)|ShotHarness/i;
const BAD_CONTENT = [
  { what: "dev mock marker", re: new RegExp(MARKER) },
  { what: "reference to a .mock module", re: /\.mock(?:\.[cm]?[jt]sx?)?["'`]|\.mock-[\w-]+\.js/ },
  { what: "dev screenshot harness", re: /ShotHarness|dev\/shot\// },
];
const TEXT = /\.(?:[cm]?js|html|css|json|map|txt|svg)$/i;

function walk(dir, out = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full, out);
    else out.push(full);
  }
  return out;
}

if (!fs.existsSync(dist)) {
  console.error(`check-no-dev-mocks: ${dist} does not exist; run vite build first.`);
  process.exit(2);
}

const problems = [];
const files = walk(dist);
for (const file of files) {
  const rel = path.relative(dist, file).split(path.sep).join("/");
  if (BAD_NAME.test(rel)) problems.push(`${rel}: file name looks like a dev mock`);
  if (!TEXT.test(file)) continue;
  const text = fs.readFileSync(file, "utf8");
  for (const { what, re } of BAD_CONTENT) {
    if (re.test(text)) problems.push(`${rel}: contains ${what}`);
  }
}

if (problems.length) {
  console.error(`check-no-dev-mocks: ${problems.length} problem(s) in ${dist}`);
  for (const p of problems) console.error(`  ${p}`);
  process.exit(1);
}
console.log(`check-no-dev-mocks: ${files.length} files in dist/, no dev mocks.`);
