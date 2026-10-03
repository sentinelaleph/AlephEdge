import { useEffect, useId, useRef, useState, type FormEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button/Button";
import "./ConfirmDialog.css";

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  /** What will happen, in labels and numbers. */
  body?: ReactNode;
  /**
   * The literal the user must type (e.g. "CLOSE"); untranslated on purpose.
   * Omitted for a reversible step: the dialog then only asks for a click.
   */
  word?: string;
  confirmLabel: string;
  busy?: boolean;
  /** Marks a dialog whose action places real orders. */
  live?: boolean;
  /**
   * A destructive action (close, delete, reset): the confirm button is the
   * danger variant. Row triggers stay quiet (secondary xs); the red lives here.
   */
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * A confirmation: the action runs only after the exact word is typed (or, for
 * a reversible step without `word`, after the confirm button is pressed).
 * Focus is trapped inside; Esc and the scrim cancel; focus returns to the
 * control that opened it.
 */
export function ConfirmDialog({ open, title, body, word, confirmLabel, busy, live, danger, onConfirm, onCancel }: ConfirmDialogProps) {
  const { t } = useTranslation();
  const titleId = useId();
  const [typed, setTyped] = useState("");
  const boxRef = useRef<HTMLFormElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const confirmRef = useRef<HTMLButtonElement>(null);
  const returnTo = useRef<Element | null>(null);
  const cancelRef = useRef(onCancel);
  cancelRef.current = onCancel;

  useEffect(() => {
    if (!open) return;
    returnTo.current = document.activeElement;
    setTyped("");
    requestAnimationFrame(() => (inputRef.current ?? confirmRef.current)?.focus());
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        cancelRef.current();
      } else if (e.key === "Tab" && boxRef.current) {
        const items = boxRef.current.querySelectorAll<HTMLElement>("input, button:not([disabled])");
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
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("keydown", onKey, true);
      (returnTo.current as HTMLElement | null)?.focus?.();
    };
  }, [open]);

  if (!open) return null;
  const matches = word === undefined || typed.trim() === word;

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (matches && !busy) onConfirm();
  };

  return createPortal(
    <div className="ae-dialog">
      <button type="button" className="ae-dialog__scrim" aria-label={t("states.cancel")} tabIndex={-1} onClick={onCancel} />
      <form
        ref={boxRef}
        className="ae-dialog__box"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={titleId}
        data-live={live || undefined}
        onSubmit={submit}
      >
        <h2 id={titleId} className="ae-dialog__title">
          {title}
        </h2>
        {body ? <div className="ae-dialog__body">{body}</div> : null}
        {word !== undefined ? (
          <label className="ae-dialog__label">
            {t("states.typeToConfirm", { word })}
            <input
              ref={inputRef}
              className="ae-dialog__input mono"
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
              autoComplete="off"
              spellCheck={false}
            />
          </label>
        ) : null}
        <div className="ae-dialog__actions">
          <Button variant="secondary" size="sm" onClick={onCancel}>
            {t("states.cancel")}
          </Button>
          <Button ref={confirmRef} type="submit" size="sm" variant={danger ? "danger" : "primary"} className="ae-dialog__confirm" disabled={!matches || busy}>
            {confirmLabel}
          </Button>
        </div>
      </form>
    </div>,
    document.body,
  );
}
