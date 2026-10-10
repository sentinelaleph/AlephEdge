import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { runUpdateCheck, runUpdateInstall, useUpdateState } from "@/app/update";
import { Button } from "@/components/ui/Button/Button";
import { FactList, Panel, type Fact } from "@/components/ui/Panel/Panel";
import { StatusChip } from "@/components/ui/StatusChip/StatusChip";
import { localeForLanguage } from "@/i18n";
import { NO_VALUE } from "@/lib/format";

/** Settings, About: the running version, a manual check and the install. */
export function UpdatePanel() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { version } = useDeskContext();
  const u = useUpdateState();
  const v = u.available?.version ?? null;
  // The installed version is known before any check: the update state only
  // learns it from the first feed read.
  const installed = u.current ?? version;
  const pct = u.progress?.total ? Math.min(100, Math.round((u.progress.downloaded / u.progress.total) * 100)) : null;

  const chip =
    u.status === "available" || u.status === "installing" ? (
      <StatusChip status="needsSetup" label={t("update.availableChip", { version: v })} />
    ) : u.status === "upToDate" ? (
      <StatusChip status="ok" label={t("update.upToDate")} />
    ) : u.status === "error" ? (
      <StatusChip status="error" label={t("update.checkFailed")} />
    ) : u.status === "off" ? (
      <StatusChip status="idle" label={t("update.offTestnet")} />
    ) : null;

  const rows: Fact[] = [
    { label: t("update.current"), value: installed ?? NO_VALUE },
    { label: t("update.latest"), value: v ?? (u.status === "upToDate" ? installed ?? NO_VALUE : NO_VALUE) },
  ];
  if (u.available?.dateMs) {
    rows.push({ label: t("update.published"), value: new Date(u.available.dateMs).toLocaleDateString(locale) });
  }
  rows.push(
    { label: t("update.checkedAt"), value: u.checkedAt ? new Date(u.checkedAt).toLocaleString(locale) : t("update.notChecked") },
    { label: t("update.schedule"), value: t("update.scheduleValue") },
  );

  return (
    <Panel title={t("update.title")} aside={chip}>
      <FactList rows={rows} />
      {u.available?.notes ? (
        <div className="ae-setnotes">
          <h3 className="ae-setnotes__title">{t("update.notes")}</h3>
          <p className="ae-muted">{u.available.notes}</p>
        </div>
      ) : null}
      {u.error ? (
        <p className="ae-error" role="alert">
          {u.error}
        </p>
      ) : null}
      {u.status === "installing" ? (
        <p className="ae-subtle" role="status">
          {pct !== null ? t("update.downloading", { pct }) : t("update.downloadingNoSize")}
        </p>
      ) : null}
      <div className="ae-toolbar">
        <Button variant="secondary" size="sm" disabled={u.status === "checking" || u.status === "installing"} onClick={() => void runUpdateCheck()}>
          {u.status === "checking" ? t("update.checking") : t("update.check")}
        </Button>
        {v && (u.status === "available" || u.status === "installing") ? (
          <Button size="sm" disabled={u.status === "installing"} onClick={() => void runUpdateInstall()}>
            {u.status === "installing" ? t("update.installing") : t("update.install")}
          </Button>
        ) : null}
      </div>
      <p className="ae-subtle">
        {u.status === "off" ? t("update.offTestnetNote") : t("update.signedNote")} {v ? t("update.restartNote") : null}
      </p>
    </Panel>
  );
}
