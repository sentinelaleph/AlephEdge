/**
 * Local backups and the bot settings file (src-tauri/src/app/backup.rs,
 * app/settings_transfer.rs). Rust writes the backups and checks every
 * imported bot; the webview lists, asks and saves the export as a file.
 */

import { devMock, inTauri, invoke } from "../bridge";

export interface BackupEntry {
  /** Folder name under `<data dir>/backups`. */
  name: string;
  kind: "update" | "manual";
  fromVersion: string | null;
  toVersion: string | null;
  /** When it was taken, UNIX ms (UTC, from the folder name). */
  atMs: number;
  sizeBytes: number;
  files: string[];
}

/** One imported bot's outcome. */
export interface ImportRow {
  /** Signal bots: futures / spot / pump. DCA / Grid: dca / grid. Null: unreadable entry. */
  kind: string | null;
  name: string | null;
  symbol: string | null;
  ok: boolean;
  /** Refusal code (`errors.*` or `strategy.errors.*`). */
  code: string | null;
  id: string | null;
}

export interface ImportReport {
  signalBots: ImportRow[];
  strategyBots: ImportRow[];
}

type Mock = typeof import("./backup.mock");
const mock = <T>(run: (m: Mock) => Promise<T>) => devMock(() => import("./backup.mock"), run);

function call<T>(cmd: string, args: Record<string, unknown> | undefined, run: (m: Mock) => Promise<T>): Promise<T> {
  return inTauri() ? invoke<T>(cmd, args) : mock(run);
}

export const backupList = () => call<BackupEntry[]>("backup_list", undefined, async (m) => m.list());

export const backupNow = () => call<BackupEntry>("backup_now", undefined, async (m) => m.now());

export const backupOpenDir = () => call<void>("backup_open_dir", undefined, async () => undefined);

/** The settings file as JSON text. */
export const settingsExport = () => call<string>("settings_export", undefined, async (m) => m.exportJson());

export const settingsImport = (json: string) => call<ImportReport>("settings_import", { json }, async (m) => m.importJson(json));
