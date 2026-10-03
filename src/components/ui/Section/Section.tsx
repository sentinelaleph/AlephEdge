import type { ReactNode } from "react";
import "./Section.css";

interface SectionProps {
  title: string;
  /** A count or short figure next to the title ("3", "1 of 50"). */
  count?: ReactNode;
  /** Right side of the heading row: a small secondary action or link. */
  aside?: ReactNode;
  /** h2 by default; h3 inside a Panel. */
  level?: 2 | 3;
  className?: string;
  children: ReactNode;
}

/**
 * An unframed group in the center column, with the same heading type as a
 * Panel title (13px / 600). Use it instead of a big in-content heading that
 * repeats the page title, and instead of a Panel when the content (a
 * DataTable, a KpiGrid) already draws its own border, so frames never nest.
 *
 *   <Section title={t("table.bot")} count={rows.length} aside={<Link …/>}>
 *     <DataTable … />
 *   </Section>
 */
export function Section({ title, count, aside, level = 2, className, children }: SectionProps) {
  const H = level === 2 ? "h2" : "h3";
  return (
    <section className={`ae-section${className ? ` ${className}` : ""}`}>
      <header className="ae-section__head">
        <H className="ae-section__title">
          {title}
          {count !== undefined && count !== null ? <span className="ae-section__count tabular">{count}</span> : null}
        </H>
        {aside ? <div className="ae-section__aside">{aside}</div> : null}
      </header>
      {children}
    </section>
  );
}
