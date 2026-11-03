/**
 * Named constants used as arguments: plot styles, shapes, locations, sizes,
 * hline styles, display and format. They evaluate to plain strings; the
 * output layer maps them onto descriptor fields. Styles Pine-lite does not
 * render (plot.style_steplinebr, plot.style_areabr, ...) are deliberately
 * absent, so the checker rejects them by name instead of drawing something
 * different. plot.style_linebr (v1.1) is a line that breaks at na values.
 */
import type { Value } from "../values";
import { COLOR_CONSTANTS } from "./color";
import { MATH_CONSTANTS } from "./math";

const group = (ns: string, names: string[], prefix = ""): Record<string, string> =>
  Object.fromEntries(names.map((n) => [`${ns}.${prefix}${n}`, n]));

export const PLOT_STYLES = ["line", "linebr", "stepline", "histogram", "columns", "circles", "cross", "area"];
export const SHAPES = ["xcross", "cross", "triangleup", "triangledown", "flag", "circle", "arrowup", "arrowdown", "labelup", "labeldown", "square", "diamond"];
export const LOCATIONS = ["abovebar", "belowbar", "top", "bottom", "absolute"];
export const SIZES = ["auto", "tiny", "small", "normal", "large", "huge"];
export const LINE_STYLES = ["solid", "dotted", "dashed"];

export const CONSTANTS: Record<string, Value> = {
  ...COLOR_CONSTANTS,
  ...MATH_CONSTANTS,
  ...group("plot", PLOT_STYLES, "style_"),
  ...group("shape", SHAPES),
  ...group("location", LOCATIONS),
  ...group("size", SIZES),
  ...group("hline", LINE_STYLES, "style_"),
  ...group("display", ["all", "none"]),
  ...group("format", ["price", "volume", "percent", "mintick", "inherit"]),
};
