# Pine-lite engine copy

`src/` is a byte-for-byte copy (line endings aside) of the website's
Pine-lite engine, so the Pine bot and ribqa.com/chart run the same code
(docs/aleph-edge-pine-lite-spec.md section 4).

- Source: the website's frontend source, `sentinel-vds-base/frontend/src/`
  (a sibling checkout, not public and not git-tracked, so there is no
  commit id: the file hashes are the version).
  - `lib/pine/**` (the engine, contract in
    `sentinel-vds-base/docs/PINE_LITE_SPEC.md`)
  - `lib/ta/core.ts` (imported by the engine), `lib/ta/overlays.ts`,
    `lib/ta/oscillators.ts`, `lib/indicators.ts` (imported by the engine
    tests only)
- Copied: 31 Oct 2026; re-synced 2 Nov 2026 (host `barIndexBase`). Hashes: `SOURCE.sha256` (sha256 of each file with
  CRLF turned into LF, `sha256sum` format). Every hash there equals the
  website file at copy time.
- Tests: `sentinel-vds-base/frontend/src/tests/lib/pine/**` copied to
  `src/__tests__/pine/` with `fixtures/reference.json`. One change: the
  "no eval" safety test in `api.test.ts` scans `pine-engine/src/lib/pine`
  instead of `src/lib/pine`. `fixtures/gen_reference.py` (the Python
  generator of reference.json) is not copied.

The `@/lib/pine`, `@/lib/ta/*` and `@/lib/indicators` imports resolve to
this copy through `vitest.config.ts` and `tsconfig.json` paths. The copy is
typechecked by `pine-engine/tsconfig.json` (the website compiles it without
`noUnusedLocals` / `noUnusedParameters`). Files are not split to this
repo's 300-line rule: an edit would break the hash check.

## Checks

- `src/__tests__/pine/source.test.ts`: the copy equals `SOURCE.sha256`.
- `npm run pine:sync`: also compares with the website engine when
  `../sentinel-vds-base/frontend/src/lib` exists (skips otherwise) and
  lists engine files the website added.
- `src/__tests__/pine/bundle.test.ts`: `src-tauri/src/pine/engine.bundle.js`
  is a fresh build of `host.ts` + the copy, contains no eval / Function /
  dynamic import, and `src-tauri/src/pine/fixtures/parity.json` was
  generated from it.

## Re-sync after a website engine change

1. Copy the files listed above over `src/` (same layout).
2. `node scripts/pine-sync-check.cjs --write` (new SOURCE.sha256).
3. Copy changed engine tests into `src/__tests__/pine/` (keep the
   api.test.ts path change).
4. `npm run pine:bundle && npm run pine:fixtures`.
5. `npx vitest run src/__tests__/pine`, then
   `cargo test --lib pine` in src-tauri (QuickJS parity).
