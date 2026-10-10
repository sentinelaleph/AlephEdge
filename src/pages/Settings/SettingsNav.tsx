import { useTranslation } from "react-i18next";
import { ALERT_CATEGORIES, useAlertPrefs } from "@/app/alerts";
import { Icon, type IconName } from "@/app/AppShell/icons";
import { useDeskContext } from "@/app/DeskProvider";
import { useUpdateState } from "@/app/update";
import { LANGUAGES } from "@/i18n";
import type { LinkStateView } from "@/lib/ipc/link/link";
import { useTheme } from "@/theme/ThemeProvider";
import { SETTINGS_TABS, type SettingsTab } from "./settingsData";

const ICONS: Record<SettingsTab, IconName> = {
  general: "sliders",
  notifications: "bell",
  devices: "phone",
  keys: "key",
  about: "info",
};

/**
 * The left panel: one row per section with what it holds and its current
 * state, so the state of every section reads without opening it.
 */
export function SettingsNav({
  tab,
  onSelect,
  link,
}: {
  tab: SettingsTab;
  onSelect: (tab: SettingsTab) => void;
  link: LinkStateView | null;
}) {
  const { t, i18n } = useTranslation();
  const theme = useTheme();
  const { prefs } = useAlertPrefs();
  const { vault, version } = useDeskContext();
  const update = useUpdateState();

  const language = LANGUAGES.find((l) => l.code === i18n.resolvedLanguage)?.endonym ?? i18n.resolvedLanguage;
  const on = ALERT_CATEGORIES.filter((c) => prefs[c]).length;
  const updateReady = update.available && (update.status === "available" || update.status === "installing");

  const status: Record<SettingsTab, { text: string | null; attention?: boolean }> = {
    general: { text: `${language} · ${t(`settings.themeOption.${theme.mode}`)}` },
    notifications: {
      text: t("settings.navStatus.alerts", { on, total: ALERT_CATEGORIES.length }),
      attention: on < ALERT_CATEGORIES.length,
    },
    devices: { text: link ? t(`link.state.${link.state}`) : null },
    keys: { text: t("settings.navStatus.keys", { count: vault.credentials.length }) },
    about: updateReady
      ? { text: t("settings.navStatus.updateAvailable", { version: update.available?.version }), attention: true }
      : { text: version ? `v${version}` : null },
  };

  return (
    <nav className="ae-setnav" aria-label={t("settings.sectionsLabel")}>
      <ul>
        {SETTINGS_TABS.map((id) => {
          const s = status[id];
          return (
            <li key={id}>
              <button
                type="button"
                className="ae-setnav__item"
                aria-current={tab === id ? "page" : undefined}
                onClick={() => onSelect(id)}
              >
                <span className="ae-setnav__icon" aria-hidden="true">
                  <Icon name={ICONS[id]} size={16} />
                </span>
                <span className="ae-setnav__text">
                  <span className="ae-setnav__label">{t(`settings.tabs.${id}`)}</span>
                  <span className="ae-setnav__hint">{t(`settings.navHint.${id}`)}</span>
                  {s.text ? (
                    <span className="ae-setnav__status" data-attention={s.attention || undefined}>
                      {s.text}
                    </span>
                  ) : null}
                </span>
              </button>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
