import { useTranslation } from "react-i18next";
import { dismissVersion, runUpdateInstall, useUpdateState } from "@/app/update";
import { Button } from "@/components/ui/Button/Button";
import "./UpdateBanner.css";

/**
 * A thin strip under the header when the signed feed offers a newer version.
 * "Install and restart" downloads, verifies and installs it; "Later" hides
 * the strip for that version only. Settings, About keeps the manual check.
 */
export function UpdateBanner() {
  const { t } = useTranslation();
  const u = useUpdateState();
  const v = u.available?.version;
  if (!v || (u.status !== "available" && u.status !== "installing") || u.dismissed === v) return null;
  const installing = u.status === "installing";
  const pct = u.progress?.total ? Math.min(100, Math.round((u.progress.downloaded / u.progress.total) * 100)) : null;
  return (
    <div className="ae-update" role="status">
      <span className="ae-update__text">
        <strong>{t("update.available", { version: v })}</strong>
        <span className="ae-update__sub">{installing ? (pct !== null ? t("update.downloading", { pct }) : t("update.downloadingNoSize")) : t("update.restartNote")}</span>
        {u.error ? <span className="ae-error">{u.error}</span> : null}
      </span>
      <span className="ae-update__actions">
        {!installing ? (
          <Button variant="ghost" size="xs" onClick={() => dismissVersion(v)}>
            {t("update.later")}
          </Button>
        ) : null}
        <Button size="xs" disabled={installing} onClick={() => void runUpdateInstall()}>
          {installing ? t("update.installing") : t("update.install")}
        </Button>
      </span>
    </div>
  );
}
