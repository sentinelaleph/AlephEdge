/**
 * input.* — declared at compile time (CompileResult.inputs), resolved once per
 * run: the default unless RunOptions.inputs overrides it by input id.
 * Overrides are validated against the declaration (type, minval/maxval,
 * options) and rejected with an "input" error rather than clamped.
 * input.timeframe only accepts the chart's own timeframe ("" or equal to
 * timeframe.period); any other value fails at run time.
 */
import { fail } from "../../errors";
import { colorToHex, isColor, parseHexColor, type Value } from "../values";
import type { BuiltinFn, HostInfo, Param, SourceName } from "./types";

export type InputKind = "int" | "float" | "bool" | "string" | "color" | "source" | "timeframe";

export interface InputInfo {
  /** Override key for RunOptions.inputs (declaration order, from 0). */
  id: number;
  kind: InputKind;
  title: string;
  /** Colour defaults as "#RRGGBBAA", source defaults as the source name. */
  defval: number | boolean | string;
  minval?: number;
  maxval?: number;
  step?: number;
  options?: (number | string)[];
  tooltip?: string;
  group?: string;
  inline?: string;
  line: number;
  col: number;
}

export type InputOverride = number | boolean | string;

export const SOURCE_NAMES: SourceName[] = ["open", "high", "low", "close", "volume", "hl2", "hlc3", "ohlc4", "hlcc4"];

const c = (name: string, req = false): Param => ({ name, req, constant: true });
const tail = ["tooltip", "inline", "group", "confirm", "display"].map((n) => c(n));
const numeric = [c("defval", true), c("title"), c("minval"), c("maxval"), c("step"), ...tail, c("options")];

const notCalled = (): Value => {
  throw new Error("input.* is resolved by the interpreter");
};

const def = (name: string, params: Param[], returns: BuiltinFn["returns"]): BuiltinFn => ({ name, params, returns, impl: notCalled });

export const INPUT_FNS: BuiltinFn[] = [
  def("input", [c("defval", true), c("title"), ...tail, c("options"), c("minval"), c("maxval"), c("step")], "any"),
  def("input.int", numeric, "int"),
  def("input.float", numeric, "float"),
  def("input.bool", [c("defval", true), c("title"), ...tail], "bool"),
  def("input.string", [c("defval", true), c("title"), c("options"), ...tail], "string"),
  def("input.source", [c("defval", true), c("title"), ...tail], "float"),
  def("input.color", [c("defval", true), c("title"), ...tail], "color"),
  def("input.timeframe", [c("defval", true), c("title"), c("options"), ...tail], "string"),
];

const HEX = /^#([0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;

function check(info: InputInfo, v: InputOverride): string | null {
  const title = `Input "${info.title}"`;
  switch (info.kind) {
    case "int":
    case "float":
      if (typeof v !== "number" || !Number.isFinite(v) || (info.kind === "int" && !Number.isInteger(v)))
        return `${title} must be ${info.kind === "int" ? "an integer" : "a number"}`;
      if (info.minval !== undefined && v < info.minval) return `${title} must be at least ${info.minval}`;
      if (info.maxval !== undefined && v > info.maxval) return `${title} must be at most ${info.maxval}`;
      break;
    case "bool":
      if (typeof v !== "boolean") return `${title} must be true or false`;
      break;
    case "color":
      if (typeof v !== "string" || !HEX.test(v)) return `${title} must be a colour like #RRGGBB or #RRGGBBAA`;
      break;
    case "source":
      if (typeof v !== "string" || !SOURCE_NAMES.includes(v as SourceName)) return `${title} must be one of ${SOURCE_NAMES.join(", ")}`;
      break;
    default:
      if (typeof v !== "string") return `${title} must be text`;
  }
  if (info.options && !info.options.includes(v as number | string)) return `${title} must be one of ${info.options.join(", ")}`;
  return null;
}

/** Input values for one run (colours as Color, sources as their name). */
export function resolveInputs(infos: InputInfo[], overrides: Record<number, InputOverride> | undefined, host: HostInfo): Value[] {
  return infos.map((info) => {
    const o = overrides?.[info.id];
    let v: InputOverride = info.defval;
    if (o !== undefined) {
      const problem = check(info, o);
      if (problem) fail("input", problem, info);
      v = o;
    }
    if (info.kind === "color") return parseHexColor(v as string);
    if (info.kind === "timeframe") {
      if (v !== "" && v !== host.timeframe)
        fail("unsupported", `input.timeframe: only the chart timeframe is supported in Pine-lite v1 (got "${v}")`, info);
      return host.timeframe;
    }
    return v;
  });
}

/** Normalises a constant colour default for the input list. */
export function colorDefault(v: Value): string | null {
  return isColor(v) ? colorToHex(v) : null;
}
