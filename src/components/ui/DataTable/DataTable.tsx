import { Fragment, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Tooltip } from "@/components/ui/Tooltip/Tooltip";
import { DEFAULT_BREAKPOINTS, splitColumns, type ColumnPriority, type PriorityBreakpoints } from "./columns";
import { nextRowIndex, rowActivationKey } from "./rowKeys";
import "./DataTable.css";

export interface DataColumn<R> {
  id: string;
  /** Short header: one or two words. Never wraps; put the explanation in `headerTip`. */
  header: string;
  /** Tooltip on the header (the long form, or what the figure means). */
  headerTip?: ReactNode;
  /** Label of the value in the row detail expander. Defaults to `header`. */
  detailLabel?: string;
  cell: (row: R) => ReactNode;
  /** Right-aligned tabular mono figures (prices, amounts, percents, counts). */
  numeric?: boolean;
  align?: "left" | "right" | "center";
  /** 1 = always (default), 2/3 = hide below the breakpoints into the row detail. */
  priority?: ColumnPriority;
  /** Never a column: the value always lives in the row detail expander. */
  detailOnly?: boolean;
  /** Never folded into the expander to make the table fit (the first column never is). */
  keep?: boolean;
  /** Long localized text wraps inside the cell instead of widening the table. */
  wrap?: boolean;
  /** Signed results: "up" | "down" colour the cell; undefined = neutral text. */
  tone?: (row: R) => "up" | "down" | "muted" | undefined;
  /** CSS width hint for the column (e.g. "1%" to shrink-wrap, "12ch"). */
  width?: string;
  sortable?: boolean;
}

export type SortDir = "asc" | "desc";

interface DataTableProps<R> {
  /** Accessible name of the table. */
  label: string;
  columns: DataColumn<R>[];
  rows: R[];
  rowKey: (row: R) => string;
  /** Row actions: compact Buttons (size="xs") / IconButtons. Pinned right, never scrolled away. */
  actions?: (row: R) => ReactNode;
  /** Shown instead of the body when `rows` is empty (an EmptyState, a short line). */
  empty?: ReactNode;
  /** Skeleton rows while the first load is pending. */
  loading?: boolean;
  /** A full-width line under a row (a hint, an error). Muted unless `rowTone` marks the row. */
  rowNote?: (row: R) => ReactNode | null;
  rowTone?: (row: R) => "danger" | "warn" | undefined;
  isSelected?: (row: R) => boolean;
  /**
   * Whole-row activation (select / open / toggle): a click on the row, or
   * Enter / Space on the focused row. The rows become one tab stop (roving
   * tabindex): arrows, Home and End move between rows. Clicks and keys on
   * the row's own controls (links, buttons, inputs) stay theirs.
   */
  onRowActivate?: (row: R) => void;
  sort?: { id: string; dir: SortDir } | null;
  onSort?: (columnId: string) => void;
  /** No outer frame: the table sits inside a Panel that already draws one. */
  bare?: boolean;
  /** 28px rows instead of 32px. */
  compact?: boolean;
  breakpoints?: PriorityBreakpoints;
}

const OWN_CONTROLS = "a, button, input, select, textarea, label, [role=button]";

/**
 * The one table of the app. Fits its container without a horizontal scroll:
 * low-priority columns fold into a per-row detail expander when the
 * container is narrow, and if the shown columns still overflow (a phone,
 * long localized text) more fold, lowest priority and rightmost first.
 * Headers never wrap (short label + `headerTip`); numeric columns are
 * right-aligned tabular figures; the actions column is pinned to the right
 * edge; rows have one height and single hairline borders.
 *
 *   const columns: DataColumn<Bot>[] = [
 *     { id: "name", header: t("table.name"), cell: (b) => <Link to={…} className="ae-link">{b.name}</Link> },
 *     { id: "state", header: t("table.state"), cell: (b) => <StatusChip status={…} /> },
 *     { id: "drawdown", header: t("strategy.col.drawdownShort"), headerTip: t("strategy.col.drawdown"),
 *       detailLabel: t("strategy.col.drawdown"), numeric: true, priority: 3, cell: (b) => formatSignedPercent(b.dd, locale) },
 *     { id: "pnl", header: t("strategy.col.totalPnl"), numeric: true,
 *       cell: (b) => formatSignedPercent(b.pnl, locale), tone: (b) => pnlToneAttr(b.pnl) },
 *     { id: "details", header: t("table.details"), detailOnly: true, cell: (b) => b.note },
 *   ];
 *   <DataTable label={t("nav.dcaBots")} columns={columns} rows={bots} rowKey={(b) => b.id}
 *     actions={(b) => <Button size="xs" variant="secondary">…</Button>} empty={<EmptyState title=… />} />
 */
export function DataTable<R>({
  label,
  columns,
  rows,
  rowKey,
  actions,
  empty,
  loading,
  rowNote,
  rowTone,
  isSelected,
  onRowActivate,
  sort,
  onSort,
  bare,
  compact,
  breakpoints = DEFAULT_BREAKPOINTS,
}: DataTableProps<R>) {
  const { t } = useTranslation();
  const wrapRef = useRef<HTMLDivElement>(null);
  const bodyRef = useRef<HTMLTableSectionElement>(null);
  // The measured width, how many extra columns fold to fit it and, as the
  // last step, whether the row actions stack; a new width starts over.
  const [fit, setFit] = useState<{ width: number | null; fold: number; stack: boolean }>({
    width: null,
    fold: 0,
    stack: false,
  });
  const [open, setOpen] = useState<Set<string>>(() => new Set());
  const [focusKey, setFocusKey] = useState<string | null>(null);

  useLayoutEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    setFit({ width: el.clientWidth, fold: 0, stack: false });
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width;
      if (w === undefined) return;
      const width = Math.round(w);
      setFit((prev) => (prev.width === width ? prev : { width, fold: 0, stack: false }));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const { visible, hidden, canFold } = splitColumns(columns, fit.width, breakpoints, fit.fold);

  // Still wider than the frame: fold one more column, before paint; with
  // nothing left to fold, stack the row actions (a phone, long labels).
  useLayoutEffect(() => {
    const el = wrapRef.current;
    if (!el || fit.width === null || el.scrollWidth <= el.clientWidth + 1) return;
    if (canFold) setFit((prev) => ({ ...prev, fold: prev.fold + 1 }));
    else if (actions && !fit.stack) setFit((prev) => ({ ...prev, stack: true }));
  });

  const expandable = hidden.length > 0;
  const span = visible.length + (expandable ? 1 : 0) + (actions ? 1 : 0);
  const keys = rows.map(rowKey);
  const selectedKey = isSelected ? keys.find((_, i) => isSelected(rows[i])) : undefined;
  const tabKey = focusKey !== null && keys.includes(focusKey) ? focusKey : (selectedKey ?? keys[0]);

  const toggle = (key: string) =>
    setOpen((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const focusRow = (index: number) => {
    const key = keys[index];
    if (key === undefined) return;
    setFocusKey(key);
    const trs = bodyRef.current?.querySelectorAll<HTMLTableRowElement>("tr[data-rowkey]") ?? [];
    for (const tr of trs) {
      if (tr.dataset.rowkey === key) {
        tr.focus();
        break;
      }
    }
  };

  const onRowKey = (e: KeyboardEvent<HTMLTableRowElement>, row: R, index: number) => {
    if (!onRowActivate || e.target !== e.currentTarget) return;
    if (rowActivationKey(e.key)) {
      e.preventDefault();
      onRowActivate(row);
      return;
    }
    const next = nextRowIndex(e.key, index, rows.length);
    if (next === null) return;
    e.preventDefault();
    focusRow(next);
  };

  const alignOf = (c: DataColumn<R>) => c.align ?? (c.numeric ? "right" : "left");

  const headerCell = (c: DataColumn<R>) => {
    const sorted = sort?.id === c.id ? sort.dir : null;
    const inner =
      c.sortable && onSort ? (
        <button
          type="button"
          className="ae-dt__sort"
          data-active={sorted || undefined}
          data-tip={c.headerTip ? true : undefined}
          onClick={() => onSort(c.id)}
        >
          {c.header}
          <span className="ae-dt__sortmark" aria-hidden="true">
            {sorted === "asc" ? "▲" : sorted === "desc" ? "▼" : ""}
          </span>
        </button>
      ) : (
        <span className="ae-dt__htip" tabIndex={0}>
          {c.header}
        </span>
      );
    if (!c.headerTip && !(c.sortable && onSort)) return c.header;
    return c.headerTip ? (
      <Tooltip content={c.headerTip} placement="bottom" align={alignOf(c) === "right" ? "end" : "start"}>
        {inner}
      </Tooltip>
    ) : (
      inner
    );
  };

  return (
    <div
      ref={wrapRef}
      className="ae-dt"
      data-bare={bare || undefined}
      data-compact={compact || undefined}
      data-stack-actions={fit.stack || undefined}
    >
      <table className="ae-dt__table" aria-label={label} aria-busy={loading || undefined}>
        <thead>
          <tr>
            {expandable ? (
              <th className="ae-dt__exp" scope="col">
                <span className="ae-visually-hidden">{t("table.details")}</span>
              </th>
            ) : null}
            {visible.map((c) => {
              const sorted = sort?.id === c.id ? sort.dir : null;
              return (
                <th
                  key={c.id}
                  scope="col"
                  data-align={alignOf(c)}
                  style={c.width ? { width: c.width } : undefined}
                  aria-sort={sorted ? (sorted === "asc" ? "ascending" : "descending") : undefined}
                >
                  {headerCell(c)}
                </th>
              );
            })}
            {actions ? (
              <th className="ae-dt__actions" scope="col" data-align="right">
                <span className="ae-visually-hidden">{t("table.actions")}</span>
              </th>
            ) : null}
          </tr>
        </thead>
        <tbody ref={bodyRef}>
          {loading && rows.length === 0
            ? [0, 1, 2].map((i) => (
                <tr key={`skel-${i}`} className="ae-dt__skel" aria-hidden="true">
                  <td colSpan={span}>
                    <span />
                  </td>
                </tr>
              ))
            : null}
          {!loading && rows.length === 0 && empty ? (
            <tr className="ae-dt__emptyrow">
              <td colSpan={span}>{empty}</td>
            </tr>
          ) : null}
          {rows.map((row, index) => {
            const key = keys[index];
            const isOpen = expandable && open.has(key);
            const note = rowNote?.(row) ?? null;
            const tone = rowTone?.(row);
            const selected = isSelected?.(row) || undefined;
            return (
              <Fragment key={key}>
                <tr
                  data-rowkey={key}
                  data-selected={selected}
                  aria-current={selected && onRowActivate ? "true" : undefined}
                  data-tone={tone}
                  data-clickable={onRowActivate ? true : undefined}
                  data-open={isOpen || undefined}
                  data-has-note={note ? true : undefined}
                  tabIndex={onRowActivate ? (key === tabKey ? 0 : -1) : undefined}
                  onFocus={onRowActivate ? (e) => e.target === e.currentTarget && setFocusKey(key) : undefined}
                  onKeyDown={onRowActivate ? (e) => onRowKey(e, row, index) : undefined}
                  onClick={
                    onRowActivate
                      ? (e) => {
                          // Clicks on the row's own controls are theirs.
                          if ((e.target as HTMLElement).closest(OWN_CONTROLS)) return;
                          setFocusKey(key);
                          onRowActivate(row);
                        }
                      : undefined
                  }
                >
                  {expandable ? (
                    <td className="ae-dt__exp">
                      <button
                        type="button"
                        className="ae-dt__expbtn"
                        aria-expanded={isOpen}
                        aria-label={t(isOpen ? "table.hideDetails" : "table.showDetails")}
                        title={t(isOpen ? "table.hideDetails" : "table.showDetails")}
                        onClick={() => toggle(key)}
                      >
                        <svg width="12" height="12" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
                          <path d="M9 6l6 6-6 6" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
                        </svg>
                      </button>
                    </td>
                  ) : null}
                  {visible.map((c) => (
                    <td
                      key={c.id}
                      data-align={alignOf(c)}
                      data-num={c.numeric || undefined}
                      data-wrap={c.wrap || undefined}
                      data-tone={c.tone?.(row)}
                    >
                      {c.cell(row)}
                    </td>
                  ))}
                  {actions ? (
                    <td className="ae-dt__actions" data-align="right">
                      <span className="ae-dt__actionsinner">{actions(row)}</span>
                    </td>
                  ) : null}
                </tr>
                {isOpen ? (
                  <tr className="ae-dt__detail">
                    <td colSpan={span}>
                      <dl className="ae-dt__detaillist">
                        {hidden.map((c) => (
                          <div key={c.id} className="ae-dt__detailitem" data-wrap={c.wrap || undefined}>
                            <dt>{c.detailLabel ?? c.header}</dt>
                            <dd data-num={c.numeric || undefined} data-tone={c.tone?.(row)}>
                              {c.cell(row)}
                            </dd>
                          </div>
                        ))}
                      </dl>
                    </td>
                  </tr>
                ) : null}
                {note ? (
                  <tr className="ae-dt__note" data-tone={tone}>
                    <td colSpan={span}>{note}</td>
                  </tr>
                ) : null}
              </Fragment>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

export type { ColumnPriority, PriorityBreakpoints } from "./columns";
