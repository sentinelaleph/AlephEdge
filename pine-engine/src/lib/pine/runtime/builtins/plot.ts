/**
 * Output built-ins -> descriptors (outputs.ts). The checker only allows these
 * as global, statement-level calls, so each runs exactly once per bar and its
 * descriptor is created on the first bar (source order = id order).
 * Titles, styles, locations and sizes are read on the first bar; series
 * values and colours are recorded per bar.
 */
import type { Display, LineStyle, Output, PlotStyle, ShapeLocation, ShapeOutput, ShapeSize, ShapeStyle } from "../../outputs";
import { colorToHex, isHandle, num, parseHexColor, truthy, type Value } from "../values";
import type { BuiltinFn, Param, Rt } from "./types";

const BLUE = parseHexColor("#2962FF");
const GRAY = parseHexColor("#787B86");
const GREEN = parseHexColor("#4CAF50");
const RED = parseHexColor("#F23645");

const p = (name: string, extra: Partial<Param> = {}): Param => ({ name, ...extra });
const k = (name: string, def?: Value): Param => ({ name, def, constant: true });
const str = (v: Value, d = ""): string => (typeof v === "string" ? v : d);
const intOr = (v: Value, d: number): number => {
  const x = num(v);
  return x === null ? d : Math.trunc(x);
};
const showLast = (v: Value): number | null => {
  const x = num(v);
  return x === null ? null : Math.trunc(x);
};
const arr = <T>(rt: Rt): (T | null)[] => new Array<T | null>(rt.bars.length).fill(null);

/** The descriptor for this call site, created on first execution. */
function out<T extends Output>(rt: Rt, make: () => Omit<T, "id">): T {
  const id = rt.state(() => rt.addOutput({ id: -1, ...make() } as T));
  return rt.output(id) as T;
}

function shape(kind: ShapeOutput["kind"], rt: Rt, cfg: { style: ShapeStyle | null; char: string | null; loc: Value; size: Value; text: Value; offset: Value; showLast: Value; title: Value; display: Value }): ShapeOutput {
  return out<ShapeOutput>(rt, () => ({
    kind,
    title: str(cfg.title),
    display: str(cfg.display, "all") as Display,
    line: 0,
    style: cfg.style,
    char: cfg.char,
    location: str(cfg.loc, "abovebar") as ShapeLocation,
    size: str(cfg.size, "auto") as ShapeSize,
    text: str(cfg.text),
    offset: intOr(cfg.offset, 0),
    showLast: showLast(cfg.showLast),
    values: arr<number>(rt),
    colors: arr<string>(rt),
    textColors: arr<string>(rt),
  }));
}

function marker(o: ShapeOutput, rt: Rt, series: Value, color: Value, textColor: Value): void {
  const on = o.location === "absolute" ? num(series) : truthy(series) ? 1 : null;
  o.values[rt.bar] = on;
  o.colors[rt.bar] = on === null ? null : colorToHex(color);
  o.textColors[rt.bar] = on === null ? null : colorToHex(textColor);
}

const handle = (h: "plot" | "hline", id: number): Value => ({ h, id });

export const PLOT_FNS: BuiltinFn[] = [
  {
    name: "plot",
    output: true,
    returns: "handle",
    params: [p("series", { req: true }), k("title", ""), p("color", { def: BLUE }), k("linewidth", 1), k("style", "line"), k("trackprice"), k("histbase", 0), k("offset", 0), k("join"), k("editable"), k("show_last"), k("display", "all"), k("format"), k("precision")],
    ignored: ["trackprice", "join", "editable", "format", "precision"],
    unsupportedArgs: ["force_overlay"],
    impl: (rt, a) => {
      const o = out<Extract<Output, { kind: "plot" }>>(rt, () => ({
        kind: "plot", title: str(a[1]), display: str(a[11], "all") as Display, line: 0, style: str(a[4], "line") as PlotStyle,
        linewidth: intOr(a[3], 1), offset: intOr(a[7], 0), histbase: num(a[6]) ?? 0, showLast: showLast(a[10]),
        values: arr<number>(rt), colors: arr<string>(rt),
      }));
      const v = num(a[0]);
      o.values[rt.bar] = v;
      o.colors[rt.bar] = v === null ? null : colorToHex(a[2]);
      return handle("plot", o.id);
    },
  },
  {
    name: "plotshape",
    output: true,
    returns: "void",
    params: [p("series", { req: true }), k("title", ""), k("style", "xcross"), k("location", "abovebar"), p("color", { def: BLUE }), k("offset", 0), k("text", ""), p("textcolor", { def: BLUE }), k("editable"), k("size", "auto"), k("show_last"), k("display", "all"), k("format"), k("precision")],
    ignored: ["editable", "format", "precision"],
    impl: (rt, a) => {
      const o = shape("plotshape", rt, { style: str(a[2], "xcross") as ShapeStyle, char: null, loc: a[3], size: a[9], text: a[6], offset: a[5], showLast: a[10], title: a[1], display: a[11] });
      marker(o, rt, a[0], a[4], a[7]);
      return null;
    },
  },
  {
    name: "plotchar",
    output: true,
    returns: "void",
    params: [p("series", { req: true }), k("title", ""), k("char", "★"), k("location", "abovebar"), p("color", { def: BLUE }), k("offset", 0), k("text", ""), p("textcolor", { def: BLUE }), k("editable"), k("size", "auto"), k("show_last"), k("display", "all"), k("format"), k("precision")],
    ignored: ["editable", "format", "precision"],
    impl: (rt, a) => {
      const o = shape("plotchar", rt, { style: null, char: str(a[2], "★"), loc: a[3], size: a[9], text: a[6], offset: a[5], showLast: a[10], title: a[1], display: a[11] });
      marker(o, rt, a[0], a[4], a[7]);
      return null;
    },
  },
  {
    name: "plotarrow",
    output: true,
    returns: "void",
    params: [p("series", { req: true }), k("title", ""), p("colorup", { def: GREEN }), p("colordown", { def: RED }), k("offset", 0), k("minheight"), k("maxheight"), k("editable"), k("show_last"), k("display", "all"), k("format"), k("precision")],
    ignored: ["minheight", "maxheight", "editable", "format", "precision"],
    impl: (rt, a) => {
      const o = shape("plotarrow", rt, { style: null, char: null, loc: "absolute", size: "auto", text: "", offset: a[4], showLast: a[8], title: a[1], display: a[9] });
      const v = num(a[0]);
      const on = v === null || v === 0 ? null : v;
      o.values[rt.bar] = on;
      o.colors[rt.bar] = on === null ? null : colorToHex(on > 0 ? a[2] : a[3]);
      return null;
    },
  },
  {
    name: "hline",
    output: true,
    returns: "handle",
    params: [p("price", { req: true }), k("title", ""), p("color", { def: GRAY }), k("linestyle", "solid"), k("linewidth", 1), k("editable"), k("display", "all")],
    ignored: ["editable"],
    impl: (rt, a) => {
      const o = out<Extract<Output, { kind: "hline" }>>(rt, () => ({
        kind: "hline", title: str(a[1]), display: str(a[6], "all") as Display, line: 0, price: num(a[0]), color: colorToHex(a[2]),
        linestyle: str(a[3], "solid") as LineStyle, linewidth: intOr(a[4], 1),
      }));
      return handle("hline", o.id);
    },
  },
  {
    name: "fill",
    output: true,
    returns: "void",
    params: [p("hline1", { req: true }), p("hline2", { req: true }), p("color", { def: parseHexColor("#2962FF33") }), k("title", ""), k("editable"), k("fillgaps"), k("display", "all")],
    ignored: ["editable", "fillgaps"],
    unsupportedArgs: ["top_value", "bottom_value", "top_color", "bottom_color"],
    impl: (rt, a) => {
      const x = a[0];
      const y = a[1];
      if (!isHandle(x) || !isHandle(y)) return rt.fail("fill: both arguments must be plot() or hline() results");
      if (x.h !== y.h) return rt.fail("fill: a plot and an hline cannot be filled together; use two plots or two hlines");
      const o = out<Extract<Output, { kind: "fill" }>>(rt, () => ({
        kind: "fill", title: str(a[3]), display: str(a[6], "all") as Display, line: 0, between: x.h === "plot" ? "plots" : "hlines", a: x.id, b: y.id, colors: arr<string>(rt),
      }));
      o.colors[rt.bar] = colorToHex(a[2]);
      return null;
    },
  },
  ...(["bgcolor", "barcolor"] as const).map(
    (kind): BuiltinFn => ({
      name: kind,
      output: true,
      returns: "void",
      params: [p("color", { req: true }), k("offset", 0), k("editable"), k("show_last"), k("title", ""), k("display", "all")],
      ignored: ["editable"],
      impl: (rt, a) => {
        const o = out<Extract<Output, { kind: "bgcolor" | "barcolor" }>>(rt, () => ({
          kind, title: str(a[4]), display: str(a[5], "all") as Display, line: 0, offset: intOr(a[1], 0), showLast: showLast(a[3]), colors: arr<string>(rt),
        }));
        o.colors[rt.bar] = colorToHex(a[0]);
        return null;
      },
    }),
  ),
  {
    name: "alertcondition",
    output: true,
    returns: "void",
    params: [p("condition", { req: true }), k("title", ""), k("message", "")],
    impl: (rt, a) => {
      const o = out<Extract<Output, { kind: "alertcondition" }>>(rt, () => ({ kind: "alertcondition", title: str(a[1]), display: "all", line: 0, message: str(a[2]), fired: [] }));
      if (truthy(a[0])) o.fired.push(rt.bar);
      return null;
    },
  },
];
