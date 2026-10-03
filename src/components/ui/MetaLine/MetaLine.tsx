import { Fragment, type ReactNode } from "react";
import "./MetaLine.css";

export interface MetaItem {
  /** Plain text or a small node (a tabular figure). */
  text: ReactNode;
  /** Stable key when `text` is not a string. */
  key?: string;
  /** Amber text: a limitation the reader must not miss ("Funding not included"). */
  warn?: boolean;
}

/**
 * Labels as one muted line, "Historical simulation · Fees included", in
 * place of a row of chips or a hand-built " · " string. The separator is
 * drawn here, never translated. `aside` sits at the end (an InfoTip).
 *
 *   <MetaLine items={[{ text: t("backtest.label.historical") }, { text: t("backtest.label.noFunding"), warn: true }]}
 *     aside={<InfoTip label=… content=… />} />
 */
export function MetaLine({ items, aside, className }: { items: MetaItem[]; aside?: ReactNode; className?: string }) {
  return (
    <p className={`ae-metaline${className ? ` ${className}` : ""}`}>
      {items.map((it, i) => (
        <Fragment key={it.key ?? (typeof it.text === "string" ? it.text : i)}>
          {i > 0 ? (
            <span className="ae-metaline__sep" aria-hidden="true">
              ·
            </span>
          ) : null}
          <span data-tone={it.warn ? "warn" : undefined}>{it.text}</span>
        </Fragment>
      ))}
      {aside}
    </p>
  );
}
