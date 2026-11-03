/**
 * color.* constants (TradingView v5/v6 palette) and functions: color.new,
 * color.rgb, color.r/g/b/t, color.from_gradient (linear in r, g, b and
 * transparency; value clamped to [bottom, top]).
 */
import { color, isColor, num, parseHexColor, type Color, type Value } from "../values";
import type { BuiltinFn } from "./types";

const PALETTE: Record<string, string> = {
  aqua: "#00BCD4", black: "#363A45", blue: "#2962FF", fuchsia: "#E040FB", gray: "#787B86", green: "#4CAF50",
  lime: "#00E676", maroon: "#880E4F", navy: "#311B92", olive: "#808000", orange: "#FF9800", purple: "#9C27B0",
  red: "#F23645", silver: "#B2B5BE", teal: "#089981", white: "#FFFFFF", yellow: "#FDD835",
};

export const COLOR_CONSTANTS: Record<string, Color> = Object.fromEntries(
  Object.entries(PALETTE).map(([k, hex]) => [`color.${k}`, parseHexColor(hex)]),
);

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));

function channel(name: "r" | "g" | "b" | "t"): BuiltinFn {
  return {
    name: `color.${name}`,
    params: [{ name: "color", req: true }],
    returns: "float",
    impl: (_rt, a) => (isColor(a[0]) ? a[0][name] : null),
  };
}

export const COLOR_FNS: BuiltinFn[] = [
  {
    name: "color.new",
    params: [{ name: "color", req: true }, { name: "transp", req: true }],
    returns: "color",
    impl: (_rt, a) => {
      const t = num(a[1]);
      return isColor(a[0]) && t !== null ? { ...a[0], t: clamp(t, 0, 100) } : null;
    },
  },
  {
    name: "color.rgb",
    params: [{ name: "red", req: true }, { name: "green", req: true }, { name: "blue", req: true }, { name: "transp", def: 0 }],
    returns: "color",
    impl: (_rt, a) => {
      const [r, g, b, t] = a.map(num);
      if (r === null || g === null || b === null || t === null) return null;
      return color(clamp(r, 0, 255), clamp(g, 0, 255), clamp(b, 0, 255), clamp(t, 0, 100));
    },
  },
  channel("r"),
  channel("g"),
  channel("b"),
  channel("t"),
  {
    name: "color.from_gradient",
    params: ["value", "bottom_value", "top_value", "bottom_color", "top_color"].map((name) => ({ name, req: true })),
    returns: "color",
    impl: (_rt, a: Value[]) => {
      const [v, lo, hi] = [num(a[0]), num(a[1]), num(a[2])];
      const c0 = a[3];
      const c1 = a[4];
      if (v === null || lo === null || hi === null || !isColor(c0) || !isColor(c1)) return null;
      const f = hi === lo ? 0 : clamp((v - lo) / (hi - lo), 0, 1);
      const mix = (x: number, y: number) => x + (y - x) * f;
      return color(mix(c0.r, c1.r), mix(c0.g, c1.g), mix(c0.b, c1.b), mix(c0.t, c1.t));
    },
  },
];
