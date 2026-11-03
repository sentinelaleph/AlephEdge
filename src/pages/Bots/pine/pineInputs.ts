import type { PineInput } from "@/lib/ipc/pine/pine";

/**
 * The engine's `InputInfo` (pine-engine/src/lib/pine/runtime/builtins/input.ts)
 * as the compile reports it, read defensively: the IPC carries it as JSON.
 * Overrides are keyed by `id` (as a string); the engine rejects a value
 * outside the declaration with an "input" error rather than clamping, so the
 * form checks the same rules before Save.
 */
export type PineInputKind = "int" | "float" | "bool" | "string" | "color" | "source" | "timeframe";

export interface PineInputInfo {
  id: number;
  kind: PineInputKind;
  title: string;
  defval: number | boolean | string;
  minval?: number;
  maxval?: number;
  step?: number;
  options?: (number | string)[];
  tooltip?: string;
  group?: string;
  line: number;
}

const KINDS: readonly PineInputKind[] = ["int", "float", "bool", "string", "color", "source", "timeframe"];
export const SOURCE_NAMES = ["open", "high", "low", "close", "volume", "hl2", "hlc3", "ohlc4", "hlcc4"] as const;
const HEX = /^#([0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;

const num = (v: unknown) => (typeof v === "number" && Number.isFinite(v) ? v : undefined);
const str = (v: unknown) => (typeof v === "string" && v.trim() ? v : undefined);

/** One reported input, or null when it is not readable. */
export function inputInfo(raw: PineInput): PineInputInfo | null {
  const id = num(raw.id);
  const kind = raw.kind as PineInputKind;
  const d = raw.defval;
  if (id === undefined || !KINDS.includes(kind) || !(typeof d === "number" || typeof d === "boolean" || typeof d === "string")) return null;
  const options = Array.isArray(raw.options) ? raw.options.filter((o): o is number | string => typeof o === "number" || typeof o === "string") : undefined;
  return {
    id,
    kind,
    title: typeof raw.title === "string" ? raw.title : "",
    defval: d,
    minval: num(raw.minval),
    maxval: num(raw.maxval),
    step: num(raw.step),
    options: options && options.length ? options : undefined,
    tooltip: str(raw.tooltip),
    group: str(raw.group),
    line: num(raw.line) ?? 0,
  };
}

export function inputInfos(raw: PineInput[]): PineInputInfo[] {
  return raw.map(inputInfo).filter((i): i is PineInputInfo => i !== null);
}

/** Inputs whose value can be a stop or target percent (read by title). */
export function numericTitles(infos: PineInputInfo[]): string[] {
  return [...new Set(infos.filter((i) => (i.kind === "float" || i.kind === "int") && i.title.trim()).map((i) => i.title.trim()))];
}

/** Why `v` is not a valid value for `info` (an i18n key under botsPine.inputError and its params), or null. */
export function inputProblem(info: PineInputInfo, v: unknown): { key: string; params?: Record<string, string | number> } | null {
  switch (info.kind) {
    case "int":
    case "float":
      if (typeof v !== "number" || !Number.isFinite(v)) return { key: "number" };
      if (info.kind === "int" && !Number.isInteger(v)) return { key: "integer" };
      if (info.minval !== undefined && v < info.minval) return { key: "min", params: { min: info.minval } };
      if (info.maxval !== undefined && v > info.maxval) return { key: "max", params: { max: info.maxval } };
      break;
    case "bool":
      if (typeof v !== "boolean") return { key: "bool" };
      break;
    case "color":
      if (typeof v !== "string" || !HEX.test(v)) return { key: "color" };
      break;
    case "source":
      if (typeof v !== "string" || !(SOURCE_NAMES as readonly string[]).includes(v)) return { key: "options" };
      break;
    default:
      if (typeof v !== "string") return { key: "text" };
  }
  if (info.options && !info.options.includes(v as number | string)) return { key: "options" };
  return null;
}

/** The value a field shows: the override when set, else the default. */
export function inputValue(info: PineInputInfo, overrides: Record<string, unknown>): unknown {
  const o = overrides[String(info.id)];
  return o === undefined ? info.defval : o;
}
