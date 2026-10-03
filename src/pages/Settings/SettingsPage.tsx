import { useTranslation } from "react-i18next";
import { ALERT_CATEGORIES, useAlertPrefs } from "@/app/alerts";
import { useDeskContext } from "@/app/DeskProvider";
import { useQueryParam } from "@/app/router/router";
import { PairingPanel } from "@/components/Link/PairingPanel/PairingPanel";
import { ExchangeKeysTab } from "./ExchangeKeysTab";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { LanguageSelect } from "@/components/ui/LanguageSelect/LanguageSelect";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { Section } from "@/components/ui/Section/Section";
import { SegmentedControl } from "@/components/ui/SegmentedControl/SegmentedControl";
import { Tabs } from "@/components/ui/Tabs/Tabs";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatSignedPercent } from "@/lib/format";
import { prefersReducedMotion } from "@/lib/motion";
import { useTheme, type ThemeMode } from "@/theme/ThemeProvider";

const TABS = ["general", "notifications", "devices", "keys", "about"] as const;
type Tab = (typeof TABS)[number];
const THEMES: ThemeMode[] = ["light", "dark", "contrast", "system"];

/** Keyboard shortcuts, as wired in AppShell, Tabs and the sidebar. */
interface Shortcut {
  keys: string[];
  action: string;
}

const SHORTCUTS: Shortcut[] = [
  { keys: ["Ctrl", "1…9"], action: "jump" },
  { keys: ["Ctrl", ","], action: "settings" },
  { keys: ["N"], action: "newBot" },
  { keys: ["/"], action: "search" },
  { keys: ["Ctrl", "B"], action: "sidebar" },
  { keys: ["Alt", "←/→"], action: "history" },
  { keys: ["Ctrl", "Shift", "←/→"], action: "tabs" },
  { keys: ["Esc"], action: "close" },
  { keys: ["↑/↓"], action: "navList" },
];

/** #/settings: device preferences. */
export function SettingsPage() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const [tabParam, setTab] = useQueryParam("tab");
  const tab: Tab = (TABS as readonly string[]).includes(tabParam ?? "") ? (tabParam as Tab) : "general";
  const theme = useTheme();
  const { prefs, setPref, storageBlocked } = useAlertPrefs();
  const { version, endpoints } = useDeskContext();

  const shortcutColumns: DataColumn<Shortcut>[] = [
    {
      id: "keys",
      header: t("settings.shortcutKeys"),
      width: "1%",
      cell: (s) => (
        <span>
          {s.keys.map((k, i) => (
            <span key={k}>
              {i > 0 ? " + " : null}
              <kbd className="ae-kbd">{k}</kbd>
            </span>
          ))}
        </span>
      ),
    },
    { id: "action", header: t("settings.shortcutAction"), cell: (s) => t(`settings.shortcut.${s.action}`) },
  ];

  return (
    <PageShell title={t("nav.settings")} single>
      <Tabs
        label={t("nav.settings")}
        value={tab}
        onChange={(v) => setTab(v === "general" ? null : v)}
        tabs={TABS.map((id) => ({ id, label: t(`settings.tabs.${id}`) }))}
      >
        {tab === "general" ? (
          <>
            <Panel title={t("settings.language")}>
              <div>
                <LanguageSelect />
              </div>
              <FactList
                rows={[
                  { label: t("settings.numberPreview"), value: formatNumber(1234567.89, locale, { minimumFractionDigits: 2 }) },
                  { label: t("settings.percentPreview"), value: formatSignedPercent(-1.25, locale) },
                  { label: t("settings.datePreview"), value: new Date().toLocaleString(locale) },
                ]}
              />
            </Panel>
            <Panel title={t("settings.theme")}>
              <SegmentedControl
                label={t("settings.theme")}
                value={theme.mode}
                onChange={(m) => theme.setMode(m)}
                wrap
                options={THEMES.map((m) => ({ value: m, label: t(`settings.themeOption.${m}`) }))}
              />
            </Panel>
            <Section title={t("settings.shortcuts")}>
              <DataTable
                label={t("settings.shortcuts")}
                columns={shortcutColumns}
                rows={SHORTCUTS}
                rowKey={(s) => s.action}
                compact
              />
            </Section>
            <Panel title={t("settings.motion")}>
              <p className="ae-muted">{t(prefersReducedMotion() ? "settings.motionReduced" : "settings.motionFull")}</p>
            </Panel>
          </>
        ) : tab === "notifications" ? (
          <Panel title={t("settings.inAppAlerts")}>
            {ALERT_CATEGORIES.map((c) => (
              <label key={c} className="ae-switchrow">
                <span>{t(`alerts.category.${c}`)}</span>
                <input type="checkbox" checked={prefs[c]} onChange={(e) => setPref(c, e.target.checked)} />
              </label>
            ))}
            {storageBlocked ? <p className="ae-error">{t("settings.storageBlocked")}</p> : null}
          </Panel>
        ) : tab === "devices" ? (
          <PairingPanel />
        ) : tab === "keys" ? (
          <ExchangeKeysTab />
        ) : (
          <Panel title={t("settings.tabs.about")}>
            <FactList
              rows={[
                { label: t("settings.version"), value: version ?? "—" },
                { label: t("accounts.apiBase"), value: endpoints?.apiBase ?? "—" },
                { label: t("settings.relay"), value: endpoints?.relayUrl ?? "—" },
                { label: t("settings.deskId"), value: endpoints?.deskId ?? "—" },
              ]}
            />
          </Panel>
        )}
      </Tabs>
    </PageShell>
  );
}
