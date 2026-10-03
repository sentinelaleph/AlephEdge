import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "@/app/AppShell/icons";
import { IconButton } from "@/components/ui/Button/Button";
import "./OverflowMenu.css";

interface OverflowMenuProps {
  /** The actions that did not fit: the page's own Buttons, listed top to bottom. */
  children: ReactNode;
  /** Accessible name and tooltip of the trigger. Defaults to "More". */
  label?: string;
}

/**
 * "More": a disclosure holding header actions that do not fit next to the
 * page title. The items are the page's own buttons, drawn as a plain list;
 * choosing one closes the list. Esc and an outside click close it and focus
 * returns to the trigger. Opens with a short scale + fade from the trigger
 * corner (reduced motion: no transform).
 */
export function OverflowMenu({ children, label }: OverflowMenuProps) {
  const { t } = useTranslation();
  const id = useId();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const popRef = useRef<HTMLDivElement>(null);
  const name = label ?? t("nav.more");

  useEffect(() => {
    if (!open) return;
    // Move focus to the first enabled item.
    requestAnimationFrame(() =>
      popRef.current?.querySelector<HTMLElement>("button:not(:disabled), a[href], [tabindex='0']")?.focus(),
    );
    const onDown = (e: PointerEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      setOpen(false);
      rootRef.current?.querySelector<HTMLElement>(".ae-more__trigger")?.focus();
    };
    document.addEventListener("pointerdown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <div className="ae-more" ref={rootRef}>
      <IconButton
        className="ae-more__trigger"
        label={name}
        icon={<Icon name="more" />}
        active={open}
        aria-expanded={open}
        aria-controls={id}
        tooltipAlign="end"
        onClick={() => setOpen((v) => !v)}
      />
      <div
        ref={popRef}
        id={id}
        className="ae-more__pop"
        data-open={open || undefined}
        hidden={!open}
        onClick={(e) => {
          // An item ran its action: the list has done its job.
          if ((e.target as HTMLElement).closest("button:not(:disabled), a[href]")) setOpen(false);
        }}
      >
        {children}
      </div>
    </div>
  );
}
