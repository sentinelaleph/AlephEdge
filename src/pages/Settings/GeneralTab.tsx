import { useTranslation } from "react-i18next";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { LanguageSelect } from "@/components/ui/LanguageSelect/LanguageSelect";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { Section } from "@/components/ui/Section/Section";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatSignedPercent } from "@/lib/format";
import { prefersReducedMotion } from "@/lib/motion";
import { useTheme, type ThemeMode } from "@/theme/ThemeProvider";

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

function timeZone(): string | null {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone ?? null;
  } catch {
    return null;
  }
}

/** Settings, General: language and formats, theme, keyboard, motion. */
export function GeneralTab() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const theme = useTheme();
  const tz = timeZone();
  const reduced = prefersReducedMotion();

  const shortcutColumns: DataColumn<Shortcut>[] = [
    {
      id: "keys",
      header: t("settings.shortcutKeys"),
      width: "1%",
      cell: (s) => (
        <span className="ae-setkeys">
          {s.keys.map((k, i) => (
            <span key={k}>
              {i > 0 ? <span className="ae-setkeys__plus">+</span> : null}
              <kbd className="ae-kbd">{k}</kbd>
            </span>
          ))}
        </span>
      ),
    },
    { id: "action", header: t("settings.shortcutAction"), cell: (s) => t(`settings.shortcut.${s.action}`) },
  ];

  return (
    <>
      <div className="ae-setgrid">
        <Panel title={t("settings.language")}>
          <div>
            <LanguageSelect />
          </div>
          <FactList
            rows={[
              { label: t("settings.numberPreview"), value: formatNumber(1234567.89, locale, { minimumFractionDigits: 2 }) },
              { label: t("settings.amountPreview"), value: `${formatNumber(1250.5, locale, { minimumFractionDigits: 2 })} USDT` },
              { label: t("settings.percentPreview"), value: formatSignedPercent(-1.25, locale) },
              { label: t("settings.datePreview"), value: new Date().toLocaleString(locale) },
              ...(tz ? [{ label: t("settings.timeZone"), value: tz }] : []),
            ]}
          />
          <p className="ae-subtle">{t("settings.languageHint")}</p>
        </Panel>

        <Panel title={t("settings.theme")}>
          <div className="ae-themecards" role="group" aria-label={t("settings.theme")}>
            {THEMES.map((m) => (
              <button
                key={m}
                type="button"
                aria-pressed={theme.mode === m}
                className="ae-themecard"
                onClick={() => theme.setMode(m)}
              >
                <span className="ae-themecard__preview" data-preview={m} aria-hidden="true">
                  <span className="ae-themecard__bar" />
                  <span className="ae-themecard__line" />
                  <span className="ae-themecard__line ae-themecard__line--short" />
                  <span className="ae-themecard__accent" />
                </span>
                <span className="ae-themecard__name">{t(`settings.themeOption.${m}`)}</span>
                <span className="ae-themecard__hint">
                  {m === "system"
                    ? t("settings.themeHint.system", { current: t(`settings.themeOption.${theme.resolved}`) })
                    : t(`settings.themeHint.${m}`)}
                </span>
              </button>
            ))}
          </div>
        </Panel>
      </div>

      <Section title={t("settings.shortcuts")}>
        <DataTable label={t("settings.shortcuts")} columns={shortcutColumns} rows={SHORTCUTS} rowKey={(s) => s.action} compact />
        <p className="ae-subtle">{t("settings.shortcutsHint")}</p>
      </Section>

      <Panel title={t("settings.motion")}>
        <p className="ae-muted">{t(reduced ? "settings.motionReduced" : "settings.motionFull")}</p>
        <p className="ae-subtle">{t("settings.motionHint")}</p>
      </Panel>
    </>
  );
}
