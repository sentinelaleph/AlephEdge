/**
 * How many header actions collapse into "More" (pure, unit-tested).
 *
 *   level 0  everything inline: [secondary…][primary]
 *   level 1  [More: secondary…][primary]
 *   level 2  [More: primary, secondary…]
 *
 * The title never truncates while a level that fits remains: the lowest
 * level whose actions leave the title its natural width wins; when none
 * does (a phone), the highest level is used and the title may wrap.
 */

export type ActionLevel = 0 | 1 | 2;

export interface ActionParts {
  /** Natural width of the secondary group, 0 when the page has none. */
  secondary: number;
  /** Natural width of the primary action, 0 when the page has none. */
  primary: number;
  /** Width of the More trigger. */
  more: number;
  /** Gap between items in the action bar. */
  gap: number;
}

/** The levels a page can use: no secondary skips 1, no primary skips 2. */
export function levelsFor(parts: Pick<ActionParts, "secondary" | "primary">): ActionLevel[] {
  const out: ActionLevel[] = [0];
  if (parts.secondary > 0) out.push(1);
  if (parts.primary > 0) out.push(2);
  return out;
}

/** Width the collapsible part of the action bar takes at `level` (each item plus its gap). */
export function partsWidth(level: ActionLevel, p: ActionParts): number {
  const items =
    level === 0 ? [p.secondary, p.primary] : level === 1 ? [p.more, p.primary] : [p.more];
  return items.filter((w) => w > 0).reduce((sum, w) => sum + w + p.gap, 0);
}

/**
 * @param room   width shared by the heading and the action bar now
 * @param fixed  width of the action bar that never collapses (the panel toggle)
 * @param title  natural (one-line) width of the page title
 */
export function pickLevel(room: number, fixed: number, title: number, parts: ActionParts): ActionLevel {
  const levels = levelsFor(parts);
  for (const level of levels) {
    if (title + fixed + partsWidth(level, parts) <= room) return level;
  }
  return levels[levels.length - 1];
}
