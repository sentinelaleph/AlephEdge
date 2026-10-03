import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { useTranslation } from "react-i18next";
import { LANGUAGES } from "@/i18n";
import { DUR, EASE } from "@/lib/motion";
import "./LanguageSelect.css";

/**
 * Language picker. Lists each language by its endonym (its own name in its own
 * script) so a user finds their language without reading English. The popover
 * scales in from the trigger's top-right (origin-aware, ease-out, <300ms) —
 * not from center (emil-design-eng).
 */
export function LanguageSelect() {
  const { i18n, t } = useTranslation();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  const current =
    LANGUAGES.find((l) => l.code === i18n.resolvedLanguage) ??
    LANGUAGES.find((l) => i18n.resolvedLanguage?.startsWith(l.code)) ??
    LANGUAGES[0];

  // Close on outside click / Escape.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const choose = (code: string) => {
    void i18n.changeLanguage(code);
    setOpen(false);
  };

  return (
    <div className="ae-lang" ref={rootRef}>
      <button
        type="button"
        className="ae-lang__trigger"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={t("lang.label")}
        onClick={() => setOpen((v) => !v)}
      >
        <GlobeIcon />
        <span className="ae-lang__current">{current.endonym}</span>
        <ChevronIcon open={open} />
      </button>

      <AnimatePresence>
        {open && (
          <motion.ul
            className="ae-lang__menu"
            role="listbox"
            aria-label={t("lang.label")}
            initial={{ opacity: 0, scale: 0.96, y: -4 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.97, y: -2 }}
            transition={{ duration: DUR.pop, ease: EASE.out }}
          >
            {LANGUAGES.map((l) => {
              const active = l.code === current.code;
              return (
                <li key={l.code} role="option" aria-selected={active}>
                  <button
                    type="button"
                    className={`ae-lang__item${active ? " is-active" : ""}`}
                    onClick={() => choose(l.code)}
                  >
                    <span>{l.endonym}</span>
                    {active && <CheckIcon />}
                  </button>
                </li>
              );
            })}
          </motion.ul>
        )}
      </AnimatePresence>
    </div>
  );
}

function GlobeIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <circle cx="12" cy="12" r="9" stroke="currentColor" strokeWidth="1.7" />
      <path
        d="M3 12h18M12 3c2.5 2.5 3.5 6 3.5 9s-1 6.5-3.5 9c-2.5-2.5-3.5-6-3.5-9S9.5 5.5 12 3Z"
        stroke="currentColor"
        strokeWidth="1.7"
      />
    </svg>
  );
}
function ChevronIcon({ open }: { open: boolean }) {
  return (
    <svg
      width="12"
      height="12"
      viewBox="0 0 24 24"
      fill="none"
      aria-hidden="true"
      className="ae-lang__chev"
      style={{ transform: open ? "rotate(180deg)" : "none" }}
    >
      <path d="M6 9l6 6 6-6" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}
function CheckIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path d="M5 12.5l4.5 4.5L19 7" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}
