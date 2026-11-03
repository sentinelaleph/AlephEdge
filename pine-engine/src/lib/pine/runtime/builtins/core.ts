/**
 * Language-level helpers: nz, na, fixnan, the int/float/bool casts, and the
 * str.* subset (tostring, format, length, contains).
 *
 * Number formatting (str.tostring without a format): integers print without
 * decimals; other values print with up to 8 decimals, trailing zeros
 * removed; na prints "NaN". Patterns: "#", "0", "#.##", "0.00", "#.##0" style
 * (digits after the point: 0 = always shown, # = shown when non-zero);
 * format.mintick, format.percent (2 decimals + "%"), format.volume (K/M/B).
 */
import { colorToHex, isColor, num, truthy, type Value } from "../values";
import { FixNan } from "./taCore";
import type { BuiltinFn, Rt } from "./types";

function fixed(v: number, min: number, max: number): string {
  let s = v.toFixed(max);
  const dot = s.indexOf(".");
  if (max > min && dot >= 0) {
    let end = s.length;
    while (end > dot + 1 + min && s[end - 1] === "0") end--;
    s = s.slice(0, end);
    if (s.endsWith(".")) s = s.slice(0, -1);
  }
  return /^-0(\.0*)?$/.test(s) ? s.slice(1) : s;
}

export function formatNumber(rt: Rt | null, v: number, format: string | null): string {
  if (format === null || format === "price" || format === "inherit") return Number.isInteger(v) ? String(v) : fixed(v, 0, 8);
  if (format === "mintick") {
    const t = rt?.host.mintick ?? 0.01;
    const decimals = Math.max(0, Math.min(10, Math.round(-Math.log10(t))));
    return (Math.round(v / t) * t).toFixed(decimals);
  }
  if (format === "percent") return `${v.toFixed(2)}%`;
  if (format === "volume") {
    const a = Math.abs(v);
    const [d, s] = a >= 1e9 ? [1e9, "B"] : a >= 1e6 ? [1e6, "M"] : a >= 1e3 ? [1e3, "K"] : [1, ""];
    return fixed(v / d, 0, 3) + s;
  }
  const m = /^[#0,]*(?:\.([#0]+))?$/.exec(format);
  if (!m) {
    if (rt) rt.fail(`str.tostring: format "${format}" is not supported in Pine-lite v1`);
    return String(v);
  }
  const dec = m[1] ?? "";
  return fixed(v, (dec.match(/0/g) ?? []).length, dec.length);
}

export function toStr(rt: Rt | null, v: Value, format: string | null = null): string {
  if (v === null) return "NaN";
  if (typeof v === "string") return v;
  if (typeof v === "boolean") return v ? "true" : "false";
  if (typeof v === "number") return formatNumber(rt, v, format);
  if (isColor(v)) return colorToHex(v) ?? "NaN";
  return String(v);
}

function strFormat(rt: Rt, fmt: string, args: Value[]): string {
  return fmt.replace(/\{([^{}]*)\}/g, (_m, inner: string) => {
    const parts = inner.split(",").map((p) => p.trim());
    const i = Number(parts[0]);
    if (!Number.isInteger(i) || i < 0 || i >= args.length) rt.fail(`str.format: placeholder {${inner}} has no matching argument`);
    if (parts.length === 1) return toStr(rt, args[i]);
    if (parts.length === 3 && parts[1] === "number") return toStr(rt, args[i], parts[2]);
    return rt.fail(`str.format: only {0} and {0,number,#.##} placeholders are supported in Pine-lite v1 (got {${inner}})`);
  });
}

export const CORE: BuiltinFn[] = [
  {
    name: "nz",
    params: [{ name: "source", req: true }, { name: "replacement", def: 0 }],
    returns: "any",
    impl: (_rt, a) => (a[0] === null ? a[1] : a[0]),
  },
  { name: "na", params: [{ name: "x", req: true }], returns: "bool", impl: (_rt, a) => a[0] === null },
  {
    name: "fixnan",
    params: [{ name: "source", req: true }],
    stateful: true,
    returns: "float",
    impl: (rt, a) => rt.state(() => new FixNan()).step(num(a[0]), rt.bar),
  },
  {
    name: "int",
    params: [{ name: "x", req: true }],
    returns: "int",
    impl: (_rt, a) => {
      const v = num(a[0]);
      return v === null ? null : Math.trunc(v);
    },
  },
  { name: "float", params: [{ name: "x", req: true }], returns: "float", impl: (_rt, a) => num(a[0]) },
  { name: "bool", params: [{ name: "x", req: true }], returns: "bool", impl: (_rt, a) => truthy(a[0]) },
  {
    name: "str.tostring",
    params: [{ name: "value", req: true }, { name: "format" }],
    returns: "string",
    impl: (rt, a) => toStr(rt, a[0], typeof a[1] === "string" ? a[1] : null),
  },
  {
    name: "str.format",
    params: [{ name: "formatString", req: true }],
    variadic: true,
    returns: "string",
    impl: (rt, a) => (typeof a[0] === "string" ? strFormat(rt, a[0], a.slice(1)) : null),
  },
  {
    name: "str.length",
    params: [{ name: "string", req: true }],
    returns: "int",
    impl: (_rt, a) => (typeof a[0] === "string" ? a[0].length : null),
  },
  {
    name: "str.contains",
    params: [{ name: "source", req: true }, { name: "str", req: true }],
    returns: "bool",
    impl: (_rt, a) => typeof a[0] === "string" && typeof a[1] === "string" && a[0].includes(a[1]),
  },
];
