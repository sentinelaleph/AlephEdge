/**
 * Column priority logic for DataTable (pure, unit-tested).
 *
 * priority 1  always shown (identity, state, the headline number, actions)
 * priority 2  shown when the table's own container is at least `p2` wide
 * priority 3  shown when it is at least `p3` wide
 * detailOnly  never a column: always in the row's detail expander
 * A hidden column moves into the row's detail expander, never off-screen.
 *
 * Fit: when the shown columns still overflow the container (a phone, long
 * localized text), `fold` more columns move into the expander, lowest
 * priority and rightmost first. The first column (the row's identity) and
 * columns marked `keep` never fold.
 */

export type ColumnPriority = 1 | 2 | 3;

export interface PriorityBreakpoints {
  /** Container width (CSS px) from which priority-2 columns show. */
  p2: number;
  /** Container width (CSS px) from which priority-3 columns show. */
  p3: number;
}

/** What splitColumns reads from a column. */
export interface SplittableColumn {
  priority?: ColumnPriority;
  detailOnly?: boolean;
  keep?: boolean;
}

/**
 * Defaults sized for the 1180 x 760 default window: a center column there is
 * ~790-1060 px depending on the side panels, so priority-3 columns show only
 * on a two-panel or wider layout and priority-2 columns survive the common case.
 */
export const DEFAULT_BREAKPOINTS: PriorityBreakpoints = { p2: 640, p3: 900 };

/** Highest priority that fits; null width (not measured yet, no DOM) shows all. */
export function maxPriority(width: number | null, bp: PriorityBreakpoints = DEFAULT_BREAKPOINTS): ColumnPriority {
  if (width === null || !Number.isFinite(width)) return 3;
  if (width >= bp.p3) return 3;
  if (width >= bp.p2) return 2;
  return 1;
}

/**
 * The order in which shown columns fold when the table still overflows:
 * priority 3 before 2 before 1, rightmost first within a priority. Never the
 * first shown column, never a `keep` column.
 */
export function foldOrder<C extends SplittableColumn>(visible: C[]): C[] {
  return visible
    .map((c, i) => ({ c, i }))
    .filter(({ c, i }) => i > 0 && !c.keep)
    .sort((a, b) => (b.c.priority ?? 1) - (a.c.priority ?? 1) || b.i - a.i)
    .map(({ c }) => c);
}

export function splitColumns<C extends SplittableColumn>(
  columns: C[],
  width: number | null,
  bp: PriorityBreakpoints = DEFAULT_BREAKPOINTS,
  fold = 0,
): { visible: C[]; hidden: C[]; canFold: boolean } {
  const max = maxPriority(width, bp);
  const shown = columns.filter((c) => !c.detailOnly && (c.priority ?? 1) <= max);
  const order = foldOrder(shown);
  const folded = new Set(order.slice(0, Math.max(0, fold)));
  const visible = shown.filter((c) => !folded.has(c));
  const hidden = columns.filter((c) => !visible.includes(c));
  return { visible, hidden, canFold: fold < order.length };
}
