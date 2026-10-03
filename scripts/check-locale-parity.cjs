#!/usr/bin/env node
/**
 * Verifies every locale under src/i18n/locales/<lang>/<lang>.json has the
 * exact same key set as en.json (the reference), and that every key carries
 * the same {{param}} set as its English source. Exits non-zero and prints
 * missing/extra keys and param drift per locale. `npm run check:i18n`.
 */
const fs = require("fs");
const path = require("path");

const localesDir = path.join(__dirname, "..", "src", "i18n", "locales");
const LANGS = ["en", "tr", "es", "hi", "id", "pt-BR", "ru", "vi"];

function flatten(obj, prefix = "", out = {}) {
  for (const [k, v] of Object.entries(obj)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (v && typeof v === "object" && !Array.isArray(v)) {
      flatten(v, key, out);
    } else {
      out[key] = v;
    }
  }
  return out;
}

/** The sorted {{param}} names a string interpolates. */
function params(value) {
  return [...String(value).matchAll(/\{\{\s*(\w+)\s*\}\}/g)]
    .map((m) => m[1])
    .sort()
    .join(",");
}

const keysByLang = {};
const valuesByLang = {};
for (const lang of LANGS) {
  const file = path.join(localesDir, lang, `${lang}.json`);
  const json = JSON.parse(fs.readFileSync(file, "utf8"));
  valuesByLang[lang] = flatten(json);
  keysByLang[lang] = new Set(Object.keys(valuesByLang[lang]));
}

const reference = keysByLang.en;
let failed = false;

for (const lang of LANGS) {
  if (lang === "en") continue;
  const keys = keysByLang[lang];
  const missing = [...reference].filter((k) => !keys.has(k));
  const extra = [...keys].filter((k) => !reference.has(k));
  const drift = [...reference].filter(
    (k) => keys.has(k) && params(valuesByLang[lang][k]) !== params(valuesByLang.en[k]),
  );

  if (missing.length === 0 && extra.length === 0 && drift.length === 0) {
    console.log(`OK   ${lang}: ${keys.size} keys, 0 missing, 0 extra, 0 param drift`);
  } else {
    failed = true;
    console.log(
      `FAIL ${lang}: ${keys.size} keys, ${missing.length} missing, ${extra.length} extra, ${drift.length} param drift`,
    );
    if (missing.length) console.log(`  missing: ${missing.join(", ")}`);
    if (extra.length) console.log(`  extra:   ${extra.join(", ")}`);
    if (drift.length) console.log(`  params:  ${drift.join(", ")}`);
  }
}

console.log(`\nReference (en): ${reference.size} keys`);
process.exit(failed ? 1 : 0);
