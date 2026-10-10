import { useTranslation } from "react-i18next";
import { ALERT_CATEGORIES, deriveAlerts, useAlertPrefs, useNow, useVaultAutoLockedAt, type AlertCategory } from "@/app/alerts";
import { useDeskContext } from "@/app/DeskProvider";
import { Button } from "@/components/ui/Button/Button";
import { Panel } from "@/components/ui/Panel/Panel";

type Severity = "danger" | "warn" | "info";

/** Grouped by what the user would do about it; each category's usual severity (alerts.ts). */
const GROUPS: { id: "safety" | "bots" | "market"; items: { category: AlertCategory; severity: Severity }[] }[] = [
  {
    id: "safety",
    items: [
      { category: "killSwitch", severity: "danger" },
      { category: "stopLoss", severity: "warn" },
      { category: "vaultAutoLock", severity: "info" },
    ],
  },
  {
    id: "bots",
    items: [
      { category: "botError", severity: "danger" },
      { category: "budget", severity: "warn" },
      { category: "dealClosed", severity: "info" },
    ],
  },
  {
    id: "market",
    items: [
      { category: "streamDown", severity: "warn" },
      { category: "regime", severity: "warn" },
    ],
  },
];

/**
 * Settings, Notifications: which alert categories the bell shows. Each row
 * says when the alert fires and how many are active right now; turning one
 * off hides the notice only, never the bots' reaction.
 */
export function NotificationsTab() {
  const { t } = useTranslation();
  const ctx = useDeskContext();
  const lockedAt = useVaultAutoLockedAt();
  const now = useNow();
  const { prefs, setPref, storageBlocked } = useAlertPrefs();

  const active = new Map<AlertCategory, number>();
  for (const a of deriveAlerts(ctx, t, lockedAt, now)) active.set(a.category, (active.get(a.category) ?? 0) + 1);
  const allOn = ALERT_CATEGORIES.every((c) => prefs[c]);

  return (
    <>
      <Panel
        title={t("settings.inAppAlerts")}
        aside={
          <Button variant="secondary" size="sm" disabled={allOn} onClick={() => ALERT_CATEGORIES.forEach((c) => setPref(c, true))}>
            {t("settings.notify.allOn")}
          </Button>
        }
      >
        <p className="ae-muted">{t("settings.notify.lede")}</p>
        {storageBlocked ? <p className="ae-error">{t("settings.storageBlocked")}</p> : null}
      </Panel>

      {GROUPS.map((g) => (
        <Panel key={g.id} title={t(`settings.notify.group.${g.id}`)}>
          <ul className="ae-notify">
            {g.items.map(({ category, severity }) => {
              const on = prefs[category];
              const count = active.get(category) ?? 0;
              const id = `notify-${category}`;
              return (
                <li key={category} className="ae-notify__row" data-off={on ? undefined : true}>
                  <span className="ae-notify__sev" data-severity={severity}>
                    {t(`settings.notify.severity.${severity}`)}
                  </span>
                  <span className="ae-notify__text">
                    <label htmlFor={id} className="ae-notify__name">
                      {t(`alerts.category.${category}`)}
                    </label>
                    <span className="ae-notify__desc" id={`${id}-desc`}>
                      {t(`settings.notify.desc.${category}`)}
                    </span>
                    {on ? null : <span className="ae-notify__off">{t("settings.notify.offNote")}</span>}
                  </span>
                  <span className="ae-notify__count" data-active={count > 0 || undefined}>
                    {t("settings.notify.activeNow", { count })}
                  </span>
                  <input
                    id={id}
                    type="checkbox"
                    role="switch"
                    className="ae-switch"
                    aria-describedby={`${id}-desc`}
                    checked={on}
                    onChange={(e) => setPref(category, e.target.checked)}
                  />
                </li>
              );
            })}
          </ul>
        </Panel>
      ))}
    </>
  );
}
