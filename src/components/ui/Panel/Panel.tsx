import type { ReactNode } from "react";
import { InfoTip } from "@/components/ui/InfoTip/InfoTip";
import "./Panel.css";

interface PanelProps {
  title?: string;
  /** Right side of the title row (a count, a link, a small action). */
  aside?: ReactNode;
  /** Frame tone: "live" marks a zone whose actions place real orders. */
  tone?: "default" | "danger" | "live";
  className?: string;
  children: ReactNode;
}

/** A titled section of a page or side panel. */
export function Panel({ title, aside, tone = "default", className, children }: PanelProps) {
  return (
    <section className={`ae-panel${className ? ` ${className}` : ""}`} data-tone={tone}>
      {title || aside ? (
        <header className="ae-panel__head">
          {title ? <h2 className="ae-panel__title">{title}</h2> : <span />}
          {aside}
        </header>
      ) : null}
      {children}
    </section>
  );
}

export interface Fact {
  label: string;
  value: ReactNode;
  tone?: "up" | "down" | "warn" | "danger" | "muted";
  /** What the figure means: an InfoTip after the label. */
  info?: ReactNode;
}

/** Label / value rows, for side-panel readouts. */
export function FactList({ rows }: { rows: Fact[] }) {
  return (
    <dl className="ae-facts">
      {rows.map((r) => (
        <div key={r.label} className="ae-facts__row">
          <dt>
            {r.label}
            {r.info ? <InfoTip label={r.label} content={r.info} size="sm" placement="left" align="center" /> : null}
          </dt>
          <dd className="tabular" data-tone={r.tone}>
            {r.value}
          </dd>
        </div>
      ))}
    </dl>
  );
}
