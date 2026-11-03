/**
 * Output descriptors: what a script draws, as data. The engine never draws;
 * the chart UI renders these. Every per-bar array has exactly one entry per
 * input bar (index-aligned with the bars passed to run()); null means
 * "nothing at this bar" (na value or na colour). Colours are "#RRGGBBAA".
 *
 * `offset` is reported, not applied: the UI shifts the series by `offset`
 * bars (positive = to the right / into the future), as TradingView does.
 */

/** "line" bridges na gaps; "linebr" breaks the line at na values (TradingView rule). */
export type PlotStyle = "line" | "linebr" | "stepline" | "histogram" | "columns" | "circles" | "cross" | "area";
export type ShapeStyle =
  | "xcross" | "cross" | "triangleup" | "triangledown" | "flag" | "circle"
  | "arrowup" | "arrowdown" | "labelup" | "labeldown" | "square" | "diamond";
export type ShapeLocation = "abovebar" | "belowbar" | "top" | "bottom" | "absolute";
export type ShapeSize = "auto" | "tiny" | "small" | "normal" | "large" | "huge";
export type LineStyle = "solid" | "dotted" | "dashed";
export type Display = "all" | "none";

interface OutputBase {
  /** Stable id within one run, in source order. */
  id: number;
  title: string;
  display: Display;
  /** Source position of the call, for click-to-line. */
  line: number;
}

export interface PlotOutput extends OutputBase {
  kind: "plot";
  style: PlotStyle;
  linewidth: number;
  offset: number;
  /** Base value for histogram / columns / area. */
  histbase: number;
  /** Bars shown counting back from the last bar; null = all. */
  showLast: number | null;
  values: (number | null)[];
  colors: (string | null)[];
}

export interface ShapeOutput extends OutputBase {
  kind: "plotshape" | "plotchar" | "plotarrow";
  /** plotshape style; "arrowup"/"arrowdown" per bar for plotarrow; null for plotchar. */
  style: ShapeStyle | null;
  /** plotchar character. */
  char: string | null;
  location: ShapeLocation;
  size: ShapeSize;
  text: string;
  offset: number;
  showLast: number | null;
  /**
   * location "absolute": the price to draw at. plotarrow: the signed value
   * (> 0 up arrow, < 0 down arrow). Otherwise 1 where a marker is drawn.
   */
  values: (number | null)[];
  colors: (string | null)[];
  textColors: (string | null)[];
}

export interface HlineOutput extends OutputBase {
  kind: "hline";
  price: number | null;
  color: string | null;
  linestyle: LineStyle;
  linewidth: number;
}

export interface FillOutput extends OutputBase {
  kind: "fill";
  /** Two plot ids or two hline ids (mixed fills are rejected). */
  between: "plots" | "hlines";
  a: number;
  b: number;
  colors: (string | null)[];
}

export interface BarColorsOutput extends OutputBase {
  /** bgcolor: pane background per bar; barcolor: candle colour per bar. */
  kind: "bgcolor" | "barcolor";
  offset: number;
  showLast: number | null;
  colors: (string | null)[];
}

export interface AlertOutput extends OutputBase {
  kind: "alertcondition";
  message: string;
  /** Bar indices where the condition was true (no delivery in v1). */
  fired: number[];
}

export type Output = PlotOutput | ShapeOutput | HlineOutput | FillOutput | BarColorsOutput | AlertOutput;
