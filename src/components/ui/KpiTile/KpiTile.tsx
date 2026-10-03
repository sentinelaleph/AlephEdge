import type { ReactNode } from "react";
import "./KpiTile.css";

interface KpiTileProps {
  label: string;
  /** Null renders the loading bar (fixed height: no layout shift). */
  value: ReactNode | null;
  tone?: "up" | "down" | "muted";
  /** Small line under the value (denominator, window, "as of"). */
  note?: ReactNode;
  /** Shown as a warning mark with the text on hover; the last value stays. */
  error?: string | null;
}

/** One headline number. Money and percent use tabular mono figures. */
export function KpiTile({ label, value, tone, note, error }: KpiTileProps) {
  return (
    <div className="ae-kpi" aria-busy={value === null || undefined}>
      <span className="ae-kpi__label">
        {label}
        {error ? (
          <span className="ae-kpi__warn" title={error} role="img" aria-label={error}>
            !
          </span>
        ) : null}
      </span>
      {value === null ? (
        <span className="ae-kpi__skel" aria-hidden="true" />
      ) : (
        <span className="ae-kpi__value tabular" data-tone={tone}>
          {value}
        </span>
      )}
      {note ? <span className="ae-kpi__note">{note}</span> : null}
    </div>
  );
}

/** A thin meter (e.g. daily loss used toward the kill switch). */
export function Meter({ fraction, label }: { fraction: number; label: string }) {
  const pct = Math.max(0, Math.min(1, fraction)) * 100;
  const tone = pct >= 80 ? "danger" : pct >= 50 ? "warn" : "ok";
  return (
    <span
      className="ae-meter"
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(pct)}
      data-tone={tone}
    >
      <span className="ae-meter__fill" style={{ transform: `scaleX(${pct / 100})` }} />
    </span>
  );
}
