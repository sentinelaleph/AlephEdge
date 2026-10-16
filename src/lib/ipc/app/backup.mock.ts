/**
 * DEV-ONLY browser preview for backups and the settings file. Reached through
 * `devMock`, so the released bundle drops this module. There is no data
 * folder in a browser: the list starts empty, "Back up now" records an empty
 * entry for this page load only, and an import reads the file but saves no
 * bot (each one is reported as not saved in the preview).
 */

import type { BackupEntry, ImportReport, ImportRow } from "./backup";

const FORMAT = "aleph-edge-bot-settings";
let entries: BackupEntry[] = [];

function stamp(ms: number): string {
  return new Date(ms).toISOString().replace(/[-:T]/g, "").slice(0, 14);
}

export function list(): BackupEntry[] {
  return [...entries].sort((a, b) => b.atMs - a.atMs);
}

export function now(): BackupEntry {
  const at = Math.floor(Date.now() / 1000) * 1000;
  const name = `manual-${stamp(at)}`;
  if (entries.some((e) => e.name === name)) throw "backupExists";
  const e: BackupEntry = { name, kind: "manual", fromVersion: null, toVersion: null, atMs: at, sizeBytes: 0, files: [] };
  entries = [e, ...entries].slice(0, 5);
  return e;
}

export function exportJson(): string {
  return JSON.stringify(
    { format: FORMAT, formatVersion: 1, appVersion: "dev", exportedAtMs: Date.now(), signalBots: [], strategyBots: [] },
    null,
    2,
  );
}

type Entry = { kind?: unknown; name?: unknown; symbol?: unknown; params?: { kind?: unknown } };

export function importJson(json: string): ImportReport {
  let raw: { format?: unknown; formatVersion?: unknown; signalBots?: Entry[]; strategyBots?: Entry[] };
  try {
    raw = JSON.parse(json);
  } catch {
    throw "importUnreadable";
  }
  if (raw?.format !== FORMAT) throw "importNotSettings";
  if (typeof raw.formatVersion === "number" && raw.formatVersion > 1) throw `importNewerFormat|${raw.formatVersion}`;
  const str = (v: unknown) => (typeof v === "string" ? v : null);
  const row = (kind: string | null, name: string | null, symbol: string | null): ImportRow => ({
    kind,
    name,
    symbol,
    ok: false,
    code: "importPreview",
    id: null,
  });
  return {
    signalBots: (raw.signalBots ?? []).map((b) => row(str(b.kind), null, null)),
    strategyBots: (raw.strategyBots ?? []).map((b) => row(str(b.params?.kind), str(b.name), str(b.symbol))),
  };
}

/** Tests only. */
export function reset(): void {
  entries = [];
}
