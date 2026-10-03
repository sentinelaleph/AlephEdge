/**
 * Keyboard model of an activatable DataTable row (pure, unit-tested): the
 * rows are one tab stop; arrows / Home / End move it, Enter / Space activate.
 */

/** Enter or Space activates the focused row. */
export function rowActivationKey(key: string): boolean {
  return key === "Enter" || key === " " || key === "Spacebar";
}

/** The row index a navigation key moves to, or null when the key is not one. */
export function nextRowIndex(key: string, index: number, count: number): number | null {
  if (count <= 0) return null;
  switch (key) {
    case "ArrowDown":
      return Math.min(count - 1, index + 1);
    case "ArrowUp":
      return Math.max(0, index - 1);
    case "Home":
      return 0;
    case "End":
      return count - 1;
    default:
      return null;
  }
}
