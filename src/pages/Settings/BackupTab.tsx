import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { Icon } from "@/app/AppShell/icons";
import { Button } from "@/components/ui/Button/Button";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { Panel } from "@/components/ui/Panel/Panel";
import i18n, { localeForLanguage } from "@/i18n";
import { localizeError } from "@/lib/errorText";
import { formatNumber } from "@/lib/format";
import { errorMessage } from "@/lib/ipc/bridge";
import {
  backupList,
  backupNow,
  backupOpenDir,
  settingsExport,
  settingsImport,
  type BackupEntry,
  type ImportReport,
  type ImportRow,
} from "@/lib/ipc/app/backup";
import { strategyErrorText } from "@/lib/strategyText";

/** "1.2 MB" style, binary units. */
export function sizeText(bytes: number, locale: string): string {
  const units = ["B", "KB", "MB", "GB"];
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${formatNumber(v, locale, { maximumFractionDigits: i === 0 ? 0 : 1 })} ${units[i]}`;
}

/** A refusal code of an imported bot, in the user's language. */
function rowCodeText(t: TFunction, row: ImportRow, strategy: boolean): string {
  const code = row.code ?? "";
  const head = code.split("|")[0];
  if (strategy && i18n.exists(`strategy.errors.${head}`)) return strategyErrorText(t, code);
  return localizeError(code);
}

function exportFileName(now: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `aleph-edge-bots-${now.getFullYear()}${p(now.getMonth() + 1)}${p(now.getDate())}-${p(now.getHours())}${p(now.getMinutes())}.json`;
}

function download(name: string, text: string) {
  const url = URL.createObjectURL(new Blob([text], { type: "application/json" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}

/**
 * Settings, Backup: the backups taken before each update (and on demand),
 * the bot settings file, and what an update keeps. Every row is read from
 * the data folder; nothing is shown that Rust did not write.
 */
export function BackupTab() {
  const { t } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const [list, setList] = useState<BackupEntry[] | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const [busy, setBusy] = useState<"backup" | "export" | "import" | null>(null);
  const [backupError, setBackupError] = useState<string | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [exported, setExported] = useState<string | null>(null);
  const [report, setReport] = useState<ImportReport | null>(null);
  const fileInput = useRef<HTMLInputElement | null>(null);

  const load = useCallback(async () => {
    try {
      setList(await backupList());
      setListError(null);
    } catch (e) {
      setListError(errorMessage(e, t("settings.backup.listFailed")));
    }
  }, [t]);

  useEffect(() => {
    void load();
  }, [load]);

  const backUp = async () => {
    setBusy("backup");
    setBackupError(null);
    try {
      await backupNow();
      await load();
    } catch (e) {
      setBackupError(errorMessage(e, t("settings.backup.failed")));
    } finally {
      setBusy(null);
    }
  };

  const doExport = async () => {
    setBusy("export");
    setFileError(null);
    setExported(null);
    try {
      const name = exportFileName(new Date());
      download(name, await settingsExport());
      setExported(name);
    } catch (e) {
      setFileError(errorMessage(e, t("settings.backup.exportFailed")));
    } finally {
      setBusy(null);
    }
  };

  const doImport = async (file: File) => {
    setBusy("import");
    setFileError(null);
    setReport(null);
    try {
      setReport(await settingsImport(await file.text()));
    } catch (e) {
      setFileError(errorMessage(e, t("settings.backup.importFailed")));
    } finally {
      setBusy(null);
      if (fileInput.current) fileInput.current.value = "";
    }
  };

  const columns: DataColumn<BackupEntry>[] = [
    { id: "at", header: t("settings.backup.col.taken"), cell: (b) => new Date(b.atMs).toLocaleString(locale), keep: true },
    {
      id: "reason",
      header: t("settings.backup.col.reason"),
      cell: (b) =>
        b.kind === "update"
          ? t("settings.backup.reasonUpdate", { from: b.fromVersion ?? "?", to: b.toVersion ?? "?" })
          : t("settings.backup.reasonManual"),
      wrap: true,
    },
    { id: "size", header: t("settings.backup.col.size"), cell: (b) => sizeText(b.sizeBytes, locale), numeric: true },
  ];

  type Row = ImportRow & { key: string; strategy: boolean };
  const rows: Row[] = report
    ? [
        ...report.signalBots.map((r, i) => ({ ...r, key: `s${i}`, strategy: false })),
        ...report.strategyBots.map((r, i) => ({ ...r, key: `b${i}`, strategy: true })),
      ]
    : [];
  const botLabel = (r: Row) => {
    if (!r.kind) return t("settings.backup.unreadableEntry");
    if (!r.strategy) return t("settings.backup.signalBot", { kind: t(`bots.kind.${r.kind}`, { defaultValue: r.kind }) });
    const kind = r.kind === "grid" ? "Grid" : "DCA";
    return [kind, r.name, r.symbol].filter(Boolean).join(" · ");
  };
  const importColumns: DataColumn<Row>[] = [
    { id: "bot", header: t("settings.backup.col.bot"), cell: botLabel, keep: true, wrap: true },
    {
      id: "result",
      header: t("settings.backup.col.result"),
      cell: (r) =>
        r.ok
          ? t(r.strategy ? "settings.backup.created" : "settings.backup.replaced")
          : `${t("settings.backup.refused")}: ${rowCodeText(t, r, r.strategy)}`,
      tone: (r) => (r.ok ? undefined : "down"),
      wrap: true,
    },
  ];
  const created = report ? report.strategyBots.filter((r) => r.ok).length + report.signalBots.filter((r) => r.ok).length : 0;

  return (
    <>
      <Panel
        title={t("settings.backup.listTitle")}
        aside={
          <div className="ae-toolbar">
            <Button variant="secondary" size="sm" onClick={() => void backupOpenDir().catch(() => undefined)}>
              <Icon name="folder" size={12} />
              {t("settings.backup.openFolder")}
            </Button>
            <Button size="sm" disabled={busy === "backup"} onClick={() => void backUp()}>
              {busy === "backup" ? t("settings.backup.running") : t("settings.backup.now")}
            </Button>
          </div>
        }
      >
        {listError ? (
          <EmptyState tone="error" title={listError} />
        ) : (
          <DataTable
            label={t("settings.backup.listTitle")}
            columns={columns}
            rows={list ?? []}
            rowKey={(b) => b.name}
            loading={list === null}
            bare
            compact
            empty={<EmptyState title={t("settings.backup.empty")} detail={t("settings.backup.emptyDetail")} />}
          />
        )}
        {backupError ? (
          <p className="ae-error" role="alert">
            {backupError}
          </p>
        ) : null}
        <p className="ae-subtle">{t("settings.backup.contents")}</p>
      </Panel>

      <Panel title={t("settings.backup.fileTitle")}>
        <p className="ae-muted">{t("settings.backup.fileHolds")}</p>
        <p className="ae-subtle">{t("settings.backup.importRule")}</p>
        <div className="ae-toolbar">
          <Button variant="secondary" size="sm" disabled={busy !== null} onClick={() => void doExport()}>
            {busy === "export" ? t("settings.backup.exporting") : t("settings.backup.export")}
          </Button>
          <Button variant="secondary" size="sm" disabled={busy !== null} onClick={() => fileInput.current?.click()}>
            {busy === "import" ? t("settings.backup.importing") : t("settings.backup.import")}
          </Button>
          <input
            ref={fileInput}
            type="file"
            accept="application/json,.json"
            hidden
            aria-label={t("settings.backup.import")}
            onChange={(e) => {
              const f = e.currentTarget.files?.[0];
              if (f) void doImport(f);
            }}
          />
        </div>
        {exported ? (
          <p className="ae-subtle" role="status">
            {t("settings.backup.exported", { name: exported })}
          </p>
        ) : null}
        {fileError ? (
          <p className="ae-error" role="alert">
            {fileError}
          </p>
        ) : null}
        {report ? (
          <>
            <p className="ae-subtle" role="status">
              {t("settings.backup.importSummary", { ok: created, total: rows.length })}
            </p>
            <DataTable
              label={t("settings.backup.importResult")}
              columns={importColumns}
              rows={rows}
              rowKey={(r) => r.key}
              bare
              compact
              empty={<EmptyState title={t("settings.backup.importEmpty")} />}
            />
          </>
        ) : null}
      </Panel>

      <Panel title={t("settings.backup.promiseTitle")}>
        <ul className="ae-setlist">
          {(["keeps", "backup", "liveBlocks", "signal", "strategy"] as const).map((k) => (
            <li key={k}>{t(`settings.backup.promise.${k}`)}</li>
          ))}
        </ul>
      </Panel>
    </>
  );
}
