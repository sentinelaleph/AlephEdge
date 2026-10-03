/**
 * Column count for a tile grid with no orphan row (pure, unit-tested).
 *
 * The widest count that fits (`fit`) gives the number of rows the tiles need;
 * the tiles are then spread evenly over those rows, so 6 tiles in room for 4
 * become 3 + 3 instead of 4 + 2, and 5 tiles in room for 4 become 3 + 2.
 */
export function balancedColumns(count: number, width: number, minTile: number, gap: number): number {
  if (count <= 0) return 1;
  if (!Number.isFinite(width) || width <= 0) return Math.min(count, 4);
  const fit = Math.max(1, Math.floor((width + gap) / (minTile + gap)));
  if (fit >= count) return count;
  const rows = Math.ceil(count / fit);
  return Math.ceil(count / rows);
}
