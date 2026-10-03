import { motion } from "framer-motion";
import { useTheme, type ThemeMode } from "@/theme/ThemeProvider";
import { SPRING } from "@/lib/motion";
import { useTranslation } from "react-i18next";
import "./ThemeToggle.css";

const MODES: { mode: ThemeMode; labelKey: string; icon: JSX.Element }[] = [
  { mode: "light", labelKey: "theme.light", icon: <SunIcon /> },
  { mode: "dark", labelKey: "theme.dark", icon: <MoonIcon /> },
  { mode: "contrast", labelKey: "theme.contrast", icon: <ContrastIcon /> },
  { mode: "system", labelKey: "theme.system", icon: <SystemIcon /> },
];

/**
 * Segmented theme switch. The active segment is marked by a single pill that
 * physically slides between options via a shared-layout spring (layoutId) —
 * the "Opus thinking toggle" feel: alive, interruptible, disciplined bounce.
 * Icons cross-fade; the pill is the one moving element.
 */
export function ThemeToggle() {
  const { mode, setMode } = useTheme();
  const { t } = useTranslation();

  return (
    <div className="ae-theme" role="radiogroup" aria-label={t("theme.toggle")}>
      {MODES.map((m) => {
        const active = mode === m.mode;
        return (
          <button
            key={m.mode}
            type="button"
            role="radio"
            aria-checked={active}
            aria-label={t(m.labelKey)}
            title={t(m.labelKey)}
            className={`ae-theme__seg${active ? " is-active" : ""}`}
            onClick={() => setMode(m.mode)}
          >
            {active && (
              <motion.span
                layoutId="ae-theme-pill"
                className="ae-theme__pill"
                transition={SPRING.toggle}
              />
            )}
            <span className="ae-theme__icon">{m.icon}</span>
          </button>
        );
      })}
    </div>
  );
}

function SunIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <circle cx="12" cy="12" r="4.2" stroke="currentColor" strokeWidth="1.8" />
      <g stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
        <path d="M12 2.6v2.4M12 19v2.4M4.6 4.6l1.7 1.7M17.7 17.7l1.7 1.7M2.6 12h2.4M19 12h2.4M4.6 19.4l1.7-1.7M17.7 6.3l1.7-1.7" />
      </g>
    </svg>
  );
}
function MoonIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M20 14.2A8 8 0 1 1 9.8 4a6.4 6.4 0 0 0 10.2 10.2Z"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinejoin="round"
      />
    </svg>
  );
}
function ContrastIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <circle cx="12" cy="12" r="9" stroke="currentColor" strokeWidth="1.8" />
      <path d="M12 3a9 9 0 0 0 0 18Z" fill="currentColor" />
    </svg>
  );
}
function SystemIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <rect x="3" y="4.5" width="18" height="12" rx="2" stroke="currentColor" strokeWidth="1.8" />
      <path d="M9 20h6M12 16.5V20" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
    </svg>
  );
}
