import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { alertTarget, type DeskAlert } from "@/app/alerts";
import { Link } from "@/app/router/router";
import { localeForLanguage } from "@/i18n";
import { Icon } from "./icons";

interface AlertsDrawerProps {
  open: boolean;
  alerts: DeskAlert[];
  /** Keys of the alerts that are new or changed since the drawer was last opened. */
  fresh?: ReadonlySet<string>;
  onClose: () => void;
}

/** Right drawer listing current alerts. Esc or the scrim closes it; focus stays inside. */
export function AlertsDrawer({ open, alerts, fresh, onClose }: AlertsDrawerProps) {
  const { t } = useTranslation();
  const boxRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  useEffect(() => {
    if (!open) return;
    const prev = document.activeElement as HTMLElement | null;
    requestAnimationFrame(() => boxRef.current?.querySelector<HTMLElement>("button")?.focus());
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeRef.current();
      else if (e.key === "Tab" && boxRef.current) {
        const items = boxRef.current.querySelectorAll<HTMLElement>("a, button");
        if (items.length === 0) return;
        const first = items[0];
        const last = items[items.length - 1];
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    };
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
      prev?.focus?.();
    };
  }, [open]);

  return (
    <div className="ae-alerts" data-open={open || undefined} aria-hidden={!open}>
      <button type="button" className="ae-alerts__scrim" tabIndex={-1} aria-label={t("alerts.close")} onClick={onClose} />
      <div ref={boxRef} className="ae-alerts__box" role="dialog" aria-modal="true" aria-label={t("alerts.title")}>
        <header className="ae-alerts__head">
          <h2>{t("alerts.title")}</h2>
          <button type="button" className="ae-header__icon" aria-label={t("alerts.close")} onClick={onClose} tabIndex={open ? 0 : -1}>
            <Icon name="close" />
          </button>
        </header>
        {open ? <AlertsList alerts={alerts} fresh={fresh} onNavigate={onClose} /> : null}
        <Link to="/settings?tab=notifications" className="ae-alerts__prefs" onClick={onClose} tabIndex={open ? 0 : -1}>
          {t("alerts.preferences")}
        </Link>
      </div>
    </div>
  );
}

/**
 * The alert rows: shared by the drawer and the dashboard's right panel. A
 * row with a page to open is one link; new or changed rows come first,
 * marked New (owner, 2 Nov 2026: the badge counted a change the list hid).
 */
export function AlertsList({ alerts, fresh, onNavigate }: { alerts: DeskAlert[]; fresh?: ReadonlySet<string>; onNavigate?: () => void }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  if (alerts.length === 0) return <p className="ae-alerts__empty">{t("alerts.none")}</p>;
  const isNew = (a: DeskAlert) => fresh?.has(a.key) === true;
  const rows = [...alerts].sort((a, b) => Number(isNew(b)) - Number(isNew(a)));
  return (
    <ul className="ae-alerts__list">
      {rows.map((a) => {
        const target = alertTarget(a);
        const body = (
          <>
            <span className="ae-alerts__cat">
              {t(`alerts.category.${a.category}`)}
              {isNew(a) ? <span className="ae-alerts__new">{t("alerts.new")}</span> : null}
            </span>
            <span className="ae-alerts__text">
              {a.title}
              {a.count && a.count > 1 && a.category !== "stopLoss" && a.category !== "dealClosed" ? (
                <span className="ae-alerts__count tabular"> ×{a.count}</span>
              ) : null}
            </span>
            {a.detail ? <span className="ae-alerts__detail">{a.detail}</span> : null}
            <span className="ae-alerts__meta">
              {a.at ? (
                <span className="tabular">{new Date(a.at).toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" })}</span>
              ) : null}
              {target ? <span className="ae-alerts__go">{t(target.labelKey)} ›</span> : null}
            </span>
          </>
        );
        return (
          <li key={a.key} data-new={isNew(a) || undefined}>
            {target ? (
              <Link to={target.to} onClick={onNavigate} className="ae-alerts__row" data-severity={a.severity}>
                {body}
              </Link>
            ) : (
              <div className="ae-alerts__row" data-severity={a.severity}>
                {body}
              </div>
            )}
          </li>
        );
      })}
    </ul>
  );
}
