/**
 * Call checking: resolves a call to indicator(), a user function or a
 * built-in, binds positional/named arguments to the canonical parameter
 * order (Call.bound), enforces where outputs and inputs may appear, applies
 * the simple/const qualifier rules, and collects input declarations.
 */
import type { Call, Expr, Pos } from "./ast";
import type { CheckerApi, Info } from "./checkTypes";
import { joinQ } from "./checkTypes";
import { evalConst, NOT_CONST, type ConstResult } from "./constEval";
import { fail, NOT_IN_V1 } from "./errors";
import { FUNCTIONS, unsupportedReason, VARIABLES } from "./registry";
import { colorDefault, SOURCE_NAMES, type InputInfo, type InputKind } from "./runtime/builtins/input";
import type { BuiltinFn, Param } from "./runtime/builtins/types";
import { isColor, type Value } from "./runtime/values";

const c = (name: string): Param => ({ name, constant: true });
/** indicator(): every argument TradingView documents; see IGNORED / REJECTED below. */
const INDICATOR: BuiltinFn = {
  name: "indicator",
  returns: "void",
  params: ["title", "shorttitle", "overlay", "format", "precision", "scale", "max_bars_back", "timeframe", "timeframe_gaps", "explicit_plot_zorder", "max_lines_count", "max_labels_count", "max_boxes_count", "calc_bars_count", "max_polylines_count", "dynamic_requests", "behind_chart"].map(c),
  impl: () => null,
};
INDICATOR.params[0].req = true;
/** Display-only in TradingView (or about objects v1 lacks): accepted and ignored. */
export const INDICATOR_IGNORED = ["scale", "max_bars_back", "timeframe_gaps", "explicit_plot_zorder", "max_lines_count", "max_labels_count", "max_boxes_count", "max_polylines_count", "dynamic_requests", "behind_chart"];

/** Binds args to `params` (trying each signature); returns canonical-order expressions. */
export function bind(name: string, params: Param[], sigs: string[][] | undefined, call: Call, ignored: string[] = []): (Expr | null)[] {
  const names = params.map((p) => p.name);
  for (const n of call.named) if (!names.includes(n.name) && !ignored.includes(n.name)) fail("type", `${name}: unknown argument '${n.name}'`, n);
  const named = new Map(call.named.map((n) => [n.name, n.value]));
  const options = sigs ?? [names];
  const k = call.args.length;
  for (const sig of options) {
    if (k > sig.length) continue;
    const positional = sig.slice(0, k);
    if (call.named.some((n) => positional.includes(n.name))) continue;
    const covered = (p: Param) => positional.includes(p.name) || named.has(p.name);
    if (params.some((p) => p.req && !covered(p))) continue;
    if (params.some((p) => !sig.includes(p.name) && named.has(p.name) && sig !== names)) continue;
    return params.map((p) => {
      const i = positional.indexOf(p.name);
      return i >= 0 ? call.args[i] : named.get(p.name) ?? null;
    });
  }
  const max = Math.max(...options.map((s) => s.length));
  if (k > max) fail("type", `${name}: too many arguments (expected at most ${max})`, call.args[max] ?? call);
  const dup = call.named.find((n) => options[0].slice(0, k).includes(n.name));
  if (dup) fail("type", `${name}: argument '${dup.name}' is given twice`, dup);
  const missing = params.find((p) => p.req && !named.has(p.name) && !options[options.length - 1].slice(0, k).includes(p.name));
  return fail("type", `${name}: missing argument '${missing?.name ?? "?"}'`, call);
}

function constArg(api: CheckerApi, e: Expr | null): ConstResult {
  return e === null ? NOT_CONST : evalConst(e, api.version, (n) => api.constOf(n));
}

export function checkCall(api: CheckerApi, call: Call, statement: boolean, declName: string | null): Info {
  if (call.callee === "indicator") return indicator(api, call, statement);
  const user = api.findFunction(call.callee);
  const current = api.currentFunction();
  if (current && current.def.name === call.callee) fail("type", `Recursive call to '${call.callee}': recursion is not supported (Pine rejects it too)`, call);
  if (user) {
    const params = user.def.params.map((p) => ({ name: p.name, req: p.def === null }));
    call.bound = bind(call.callee, params, undefined, call);
    call.user = user.id;
    for (const a of call.bound) if (a) api.expr(a);
    if (user.stateful) noteStateful(api, call, call.callee);
    return { t: user.tuple ? "tuple" : "any", q: "unknown" };
  }
  const def = FUNCTIONS.get(call.callee);
  if (!def) {
    if (VARIABLES.has(call.callee)) fail("unsupported", `${call.callee}() with arguments ${NOT_IN_V1}`, call);
    const u = unsupportedReason(call.callee);
    if (u) fail("unsupported", u.message, call, u.hint);
    return fail("name", `Could not find function or function reference '${call.callee}'`, call);
  }
  if (def.output) {
    if (!statement || api.inLocalScope()) fail("type", `Cannot use '${def.name}' in local scope; output calls must be global statements`, call);
    api.countOutput(call);
  }
  if (def.variadic) {
    if (call.named.length > 0) fail("type", `${def.name}: named arguments are not supported here`, call.named[0]);
    if (call.args.length < def.params.filter((p) => p.req).length) fail("type", `${def.name}: missing argument '${def.params[0].name}'`, call);
    call.bound = [...call.args];
  } else {
    const rejected = call.named.find((n) => def.unsupportedArgs?.includes(n.name));
    if (rejected) fail("unsupported", `${def.name}(${rejected.name}=...) ${NOT_IN_V1}`, rejected);
    call.bound = bind(def.name, def.params, def.sigs, call, def.ignored);
  }
  call.def = def;
  if (def.name.startsWith("input")) return input(api, def, call, declName);
  let q: Info["q"] = "const";
  call.bound.forEach((a, i) => {
    if (!a) return;
    const info = api.expr(a);
    const p = def.params[Math.min(i, def.params.length - 1)];
    if (p.simple && info.q === "series") fail("type", `${def.name}: argument '${p.name}' must be a simple value, but a series was given`, a, "Use a constant or an input for this argument");
    if (p.constant && info.q === "series") fail("type", `${def.name}: argument '${p.name}' must be a constant`, a);
    q = joinQ(q, info.q);
  });
  for (const n of call.named) if (def.ignored?.includes(n.name)) api.expr(n.value);
  if (def.stateful) noteStateful(api, call, def.name);
  return { t: def.returns, q: def.stateful || def.output ? "series" : q };
}

function noteStateful(api: CheckerApi, call: Call, name: string): void {
  const fn = api.currentFunction();
  if (fn) fn.stateful = true;
  if (api.conditional())
    api.warn(`The function '${name}' should be called on each calculation for consistency. It is recommended to extract the call from this scope`, call);
}

function indicator(api: CheckerApi, call: Call, statement: boolean): Info {
  if (!statement || api.inLocalScope()) fail("type", "indicator() must be a global statement", call);
  const b = bind("indicator", INDICATOR.params, undefined, call, []);
  call.decl = true;
  const get = (i: number): Value | undefined => {
    const e = b[i];
    if (!e) return undefined;
    api.expr(e);
    const v = constArg(api, e);
    if (v === NOT_CONST) fail("type", `indicator: argument '${INDICATOR.params[i].name}' must be a constant`, e);
    return v;
  };
  const title = get(0);
  if (typeof title !== "string") fail("type", "indicator: title must be a string", b[0] ?? call);
  const tf = get(7);
  if (tf !== undefined && tf !== "" && tf !== null) fail("unsupported", `indicator(timeframe=...) ${NOT_IN_V1}`, b[7] ?? call, "Multi-timeframe scripts need request.security, which v1 does not have");
  if (get(13) !== undefined) fail("unsupported", `indicator(calc_bars_count=...) ${NOT_IN_V1}`, b[13] ?? call);
  INDICATOR.params.forEach((p, i) => INDICATOR_IGNORED.includes(p.name) && get(i));
  const shorttitle = get(1);
  const overlay = get(2);
  const format = get(3);
  const precision = get(4);
  api.setMeta(
    {
      title,
      shorttitle: typeof shorttitle === "string" ? shorttitle : title,
      overlay: overlay === true,
      format: typeof format === "string" ? format : "inherit",
      precision: typeof precision === "number" ? precision : null,
    },
    call,
  );
  return { t: "void", q: "const" };
}

const KIND: Record<string, InputKind> = {
  "input.int": "int", "input.float": "float", "input.bool": "bool", "input.string": "string",
  "input.color": "color", "input.source": "source", "input.timeframe": "timeframe",
};

function input(api: CheckerApi, def: BuiltinFn, call: Call, declName: string | null): Info {
  if (api.inLocalScope()) fail("type", `Cannot use '${def.name}' in local scope; inputs must be declared globally`, call);
  const bound = call.bound as (Expr | null)[];
  const at = (name: string) => bound[def.params.findIndex((p) => p.name === name)] ?? null;
  const value = (name: string): Value | undefined => {
    const e = at(name);
    if (!e) return undefined;
    if (e.kind !== "Tuple") api.expr(e);
    const v = constArg(api, e);
    if (v === NOT_CONST) fail("type", `${def.name}: argument '${name}' must be a constant`, e);
    return v;
  };
  const dv = at("defval") as Expr;
  let kind = KIND[def.name];
  let defval: Value | undefined;
  if (dv.kind === "Ident" && (SOURCE_NAMES as string[]).includes(dv.name) && (kind === undefined || kind === "source")) {
    kind = "source";
    defval = dv.name;
  } else {
    if (kind === "source") fail("type", `input.source: default must be one of ${SOURCE_NAMES.join(", ")}`, dv);
    defval = value("defval");
    kind ??= typeof defval === "boolean" ? "bool" : typeof defval === "string" ? "string" : isColor(defval as Value) ? "color" : dv.kind === "Num" && dv.isInt ? "int" : "float";
  }
  const info = buildInfo(def.name, kind, defval as Value, value, dv);
  const title = value("title");
  const id = api.addInput({ ...info, title: typeof title === "string" && title !== "" ? title : declName ?? `Input ${call.site}`, line: call.line, col: call.col });
  call.input = id;
  const t = kind === "source" ? "float" : kind === "timeframe" ? "string" : kind;
  return { t, q: kind === "source" ? "series" : "input" };
}

function buildInfo(name: string, kind: InputKind, defval: Value, value: (n: string) => Value | undefined, at: Pos): Omit<InputInfo, "id" | "title" | "line" | "col"> {
  const ok =
    kind === "int" ? typeof defval === "number" && Number.isInteger(defval)
    : kind === "float" ? typeof defval === "number"
    : kind === "bool" ? typeof defval === "boolean"
    : kind === "color" ? isColor(defval)
    : typeof defval === "string";
  if (!ok) fail("type", `${name}: default value must be ${kind === "int" ? "an integer" : `a ${kind}`}`, at);
  const out: Omit<InputInfo, "id" | "title" | "line" | "col"> = { kind, defval: kind === "color" ? (colorDefault(defval) as string) : (defval as number | boolean | string) };
  for (const k of ["minval", "maxval", "step"] as const) {
    const v = value(k);
    if (typeof v === "number") out[k] = v;
  }
  const opts = value("options");
  if (Array.isArray(opts)) {
    out.options = opts.filter((o): o is number | string => typeof o === "number" || typeof o === "string");
    if (!out.options.includes(out.defval as number | string)) fail("type", `${name}: default value must be one of the options`, at);
  }
  for (const k of ["tooltip", "group", "inline"] as const) {
    const v = value(k);
    if (typeof v === "string") out[k] = v;
  }
  return out;
}
