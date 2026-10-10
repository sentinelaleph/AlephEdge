import { useTranslation } from "react-i18next";
import { ALERT_CATEGORIES, useAlertPrefs } from "@/app/alerts";
import { Icon } from "@/app/AppShell/icons";
import { useDeskContext } from "@/app/DeskProvider";
import { Link } from "@/app/router/router";
import { useUpdateState, type UpdateState } from "@/app/update";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { isTestnetBuild, openHelpLink, type AppInfo } from "@/lib/ipc/app/about";
import type { LinkStateView } from "@/lib/ipc/link/link";
import { idleDurationKey } from "@/lib/ipc/vault/vault";
import { NO_VALUE } from "@/lib/format";
import type { TFunction } from "i18next";
import "./SettingsPage.css";

export function updateStatusText(t: TFunction, u: UpdateState): string {
  switch (u.status) {
    case "available":
    case "installing":
      return t("update.availableChip", { version: u.available?.version });
    case "upToDate":
      return t("update.upToDate");
    case "checking":
      return t("update.checking");
    case "error":
      return t("update.checkFailed");
    case "off":
      return t("update.offTestnet");
    default:
      return t("settings.summary.notChecked");
  }
}

export function buildText(t: TFunction, info: AppInfo | null): string {
  if (!info) return NO_VALUE;
  const build = t(info.liveBuild ? "settings.about.liveBuild" : "settings.about.paperBuild");
  return isTestnetBuild(info) ? `${build} · ${t("settings.about.testnetBuild")}` : build;
}

/** The right panel: this installation at a glance, and where to get help. */
export function SettingsSummary({ info, link }: { info: AppInfo | null; link: LinkStateView | null }) {
  const { t } = useTranslation();
  const { version, endpoints, vault } = useDeskContext();
  const update = useUpdateState();
  const { prefs } = useAlertPrefs();
  const idle = idleDurationKey(vault.status.idleTimeoutMinutes);
  const updateReady = update.status === "available" || update.status === "installing";

  return (
    <>
      <Panel title={t("settings.summary.title")}>
        <FactList
          rows={[
            { label: t("settings.summary.version"), value: version ?? NO_VALUE },
            { label: t("settings.summary.updates"), value: updateStatusText(t, update), tone: updateReady ? "warn" : undefined },
            { label: t("settings.summary.build"), value: buildText(t, info) },
            {
              label: t("settings.summary.orders"),
              value: endpoints ? t(endpoints.binanceIsProduction ? "accounts.production" : "app.testnetBadge") : NO_VALUE,
              tone: endpoints && !endpoints.binanceIsProduction ? "warn" : undefined,
            },
            {
              label: t("settings.summary.vault"),
              value: `${t("settings.summary.vaultUnlocked")} · ${t(idle.key, { count: idle.count })}`,
            },
            { label: t("settings.summary.keys"), value: vault.credentials.length },
            { label: t("settings.summary.phone"), value: link ? t(`link.state.${link.state}`) : NO_VALUE },
            {
              label: t("settings.summary.alerts"),
              value: `${ALERT_CATEGORIES.filter((c) => prefs[c]).length} / ${ALERT_CATEGORIES.length}`,
            },
          ]}
        />
      </Panel>
      <HelpPanel />
    </>
  );
}

/** Where to get help: the in-app Guide and FAQ, the web user guide, support. */
export function HelpPanel() {
  const { t } = useTranslation();
  return (
    <Panel title={t("settings.help.title")}>
      <ul className="ae-sethelp">
        <li>
          <Link to="/guide" className="ae-link">
            {t("settings.help.guide")}
          </Link>
        </li>
        <li>
          <Link to="/faq?s=settings" className="ae-link">
            {t("settings.help.faq")}
          </Link>
        </li>
        <li>
          <button type="button" className="ae-link ae-sethelp__ext" onClick={() => void openHelpLink("userGuide").catch(() => undefined)}>
            {t("settings.help.userGuide")}
            <Icon name="external" size={12} />
          </button>
        </li>
        <li>
          <button type="button" className="ae-link ae-sethelp__ext" onClick={() => void openHelpLink("support").catch(() => undefined)}>
            {t("settings.help.support")}
            <Icon name="external" size={12} />
          </button>
        </li>
      </ul>
    </Panel>
  );
}
