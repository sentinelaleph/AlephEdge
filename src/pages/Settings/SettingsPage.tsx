import { useTranslation } from "react-i18next";
import { useQueryParam } from "@/app/router/router";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { AboutTab } from "./AboutTab";
import { DevicesTab } from "./DevicesTab";
import { ExchangeKeysTab } from "./ExchangeKeysTab";
import { GeneralTab } from "./GeneralTab";
import { NotificationsTab } from "./NotificationsTab";
import { isSettingsTab, useAppInfo, useLinkStateView, type SettingsTab } from "./settingsData";
import { SettingsNav } from "./SettingsNav";
import { SettingsSummary } from "./SettingsSummary";
import "./SettingsPage.css";

/**
 * #/settings: this installation's preferences, in the Sentinel page shape.
 * Left: the sections, each with its current state. Center: the open section
 * (`?tab=`, so deep links like /settings?tab=keys keep working). Right: the
 * installation at a glance and where to get help.
 */
export function SettingsPage() {
  const { t } = useTranslation();
  const [tabParam, setTab] = useQueryParam("tab");
  const tab: SettingsTab = isSettingsTab(tabParam) ? tabParam : "general";
  const info = useAppInfo();
  const link = useLinkStateView();

  return (
    <PageShell
      title={t("nav.settings")}
      leftLabel={t("settings.sectionsLabel")}
      left={<SettingsNav tab={tab} link={link} onSelect={(id) => setTab(id === "general" ? null : id)} />}
      right={<SettingsSummary info={info} link={link} />}
    >
      <div className="ae-settings" data-tab={tab}>
        <header className="ae-settings__head">
          <h2 className="ae-settings__title">{t(`settings.tabs.${tab}`)}</h2>
          <p className="ae-settings__hint">{t(`settings.navHint.${tab}`)}</p>
        </header>
        {tab === "general" ? (
          <GeneralTab />
        ) : tab === "notifications" ? (
          <NotificationsTab />
        ) : tab === "devices" ? (
          <DevicesTab />
        ) : tab === "keys" ? (
          <ExchangeKeysTab />
        ) : (
          <AboutTab info={info} />
        )}
      </div>
    </PageShell>
  );
}
