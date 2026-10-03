import { useTranslation } from "react-i18next";
import { runUpdateCheck, runUpdateInstall, useUpdateState } from "@/app/update";
import { Button } from "@/components/ui/Button/Button";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { StatusChip } from "@/components/ui/StatusChip/StatusChip";
import { localeForLanguage } from "@/i18n";
import { NO_VALUE } from "@/lib/format";

/** Settings, About: the running version, a manual check and the install. */
export function UpdatePanel() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const u = useUpdateState();
  const v = u.available?.version ?? null;
  const pct = u.progress?.total ? Math.min(100, Math.round((u.progress.downloaded / u.progress.total) * 100)) : null;

  const chip =
    u.status === "available" || u.status === "installing" ? (
      <StatusChip status="needsSetup" label={t("update.availableChip", { version: v })} />
    ) : u.status === "upToDate" ? (
      <StatusChip status="ok" label={t("update.upToDate")} />
    ) : u.status === "error" ? (
      <StatusChip status="error" label={t("update.checkFailed")} />
    ) : null;

  return (
    <Panel title={t("update.title")} aside={chip}>
      <FactList
        rows={[
          { label: t("update.current"), value: u.current ?? NO_VALUE },
          { label: t("update.latest"), value: v ?? (u.status === "upToDate" ? u.current ?? NO_VALUE : NO_VALUE) },
          {
            label: t("update.checkedAt"),
            value: u.checkedAt ? new Date(u.checkedAt).toLocaleString(locale) : t("update.notChecked"),
          },
        ]}
      />
      {u.available?.notes ? <p className="ae-muted">{u.available.notes}</p> : null}
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
      <p className="ae-subtle">{t("update.signedNote")}</p>
    </Panel>
  );
}
