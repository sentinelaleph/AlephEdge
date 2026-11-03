/**
 * Runtime values and Pine's na rules.
 *
 * Representation: na is `null`; int and float are JS numbers; bool is a JS
 * boolean; colours are {r, g, b, t} with t = transparency 0..100 (Pine's
 * convention); plot()/hline() return handles; tuples are arrays.
 *
 * na rules implemented (documented in docs/PINE_LITE_SPEC.md section 3):
 * - Arithmetic with na is na. Division by zero is na, and any arithmetic
 *   result that is not finite (NaN, ±Infinity) is na.
 * - Comparisons with an na operand: v6 -> false; v5 -> na (a bool na).
 * - `not na`: v6 -> true (na bool is false); v5 -> na.
 * - `and` / `or`: v6 evaluates lazily (right side only when needed); v5
 *   evaluates both sides. In both, an na operand counts as false and the
 *   result is a plain bool.
 * - Conditions (if, ternary, while, plotshape series) treat na as false; a
 *   number used as a condition is true when it is non-zero.
 */

export interface Color {
  r: number;
  g: number;
  b: number;
  /** Transparency 0 (opaque) .. 100 (invisible). */
  t: number;
}

export interface Handle {
  h: "plot" | "hline";
  id: number;
}

export type Value = number | boolean | string | Color | Handle | null | Value[];

export type Version = 5 | 6;

export const isColor = (v: Value): v is Color => typeof v === "object" && v !== null && !Array.isArray(v) && "r" in v;
export const isHandle = (v: Value): v is Handle => typeof v === "object" && v !== null && !Array.isArray(v) && "h" in v;

export function num(v: Value): number | null {
  if (typeof v === "number") return v;
  if (typeof v === "boolean") return v ? 1 : 0;
  return null;
}

export function truthy(v: Value): boolean {
  if (v === null) return false;
  if (typeof v === "boolean") return v;
  if (typeof v === "number") return v !== 0;
  return true;
}

export function finite(x: number): number | null {
  return Number.isFinite(x) ? x : null;
}

export function arith(op: string, a: Value, b: Value, intDiv = false): Value {
  if (typeof a === "string" || typeof b === "string") {
    if (op === "+" && typeof a === "string" && typeof b === "string") return a + b;
    return null;
  }
  const x = num(a);
  const y = num(b);
  if (x === null || y === null) return null;
  switch (op) {
    case "+":
      return finite(x + y);
    case "-":
      return finite(x - y);
    case "*":
      return finite(x * y);
    case "/":
      if (y === 0) return null;
      return intDiv ? Math.trunc(x / y) : finite(x / y);
    case "%":
      if (y === 0) return null;
      return finite(x % y);
  }
  return null;
}

function same(a: Value, b: Value): boolean {
  if (isColor(a) && isColor(b)) return a.r === b.r && a.g === b.g && a.b === b.b && a.t === b.t;
  if (typeof a === "boolean" || typeof b === "boolean") return truthy(a) === truthy(b);
  return a === b;
}

export function compare(op: string, a: Value, b: Value, version: Version): boolean | null {
  if (a === null || b === null) return version === 6 ? false : null;
  if (op === "==") return same(a, b);
  if (op === "!=") return !same(a, b);
  const x = typeof a === "string" ? a : num(a);
  const y = typeof b === "string" ? b : num(b);
  if (x === null || y === null) return version === 6 ? false : null;
  switch (op) {
    case "<":
      return x < y;
    case ">":
      return x > y;
    case "<=":
      return x <= y;
    case ">=":
      return x >= y;
  }
  return null;
}

export function not(v: Value, version: Version): boolean | null {
  if (v === null) return version === 6 ? true : null;
  return !truthy(v);
}

export function negate(v: Value): Value {
  const x = num(v);
  return x === null ? null : -x;
}

export function color(r: number, g: number, b: number, t = 0): Color {
  return { r, g, b, t };
}

export function parseHexColor(hex: string): Color {
  const h = hex.replace("#", "");
  const r = parseInt(h.slice(0, 2), 16);
  const g = parseInt(h.slice(2, 4), 16);
  const b = parseInt(h.slice(4, 6), 16);
  const a = h.length === 8 ? parseInt(h.slice(6, 8), 16) : 255;
  return { r, g, b, t: ((255 - a) / 255) * 100 };
}

const hex2 = (n: number) => Math.max(0, Math.min(255, Math.round(n))).toString(16).padStart(2, "0").toUpperCase();

/** "#RRGGBBAA" for output descriptors; null for na. */
export function colorToHex(c: Value): string | null {
  if (!isColor(c)) return null;
  return `#${hex2(c.r)}${hex2(c.g)}${hex2(c.b)}${hex2(((100 - c.t) / 100) * 255)}`;
}

export function typeName(v: Value): string {
  if (v === null) return "na";
  if (Array.isArray(v)) return "tuple";
  if (isColor(v)) return "color";
  if (isHandle(v)) return v.h;
  return typeof v === "number" ? "number" : typeof v;
}
