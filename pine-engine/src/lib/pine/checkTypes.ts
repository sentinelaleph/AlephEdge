/**
 * Types shared by the checker modules, and the CheckedScript it produces for
 * the interpreter.
 */
import type { Expr, FuncDef, Pos, Program, Stmt } from "./ast";
import type { PineError } from "./errors";
import type { InputInfo } from "./runtime/builtins/input";
import type { PType, Qual } from "./runtime/builtins/types";
import type { Value, Version } from "./runtime/values";

/** Qualifier as far as the checker can tell; "unknown" never triggers an error. */
export type Q = Qual | "unknown";

export interface Info {
  t: PType;
  q: Q;
  /** Compile-time value when the expression is a constant. */
  c?: Value;
}

const ORDER: Qual[] = ["const", "input", "simple", "series"];

export function joinQ(a: Q, b: Q): Q {
  if (a === "series" || b === "series") return "series";
  if (a === "unknown" || b === "unknown") return "unknown";
  return ORDER[Math.max(ORDER.indexOf(a), ORDER.indexOf(b))];
}

export interface VarInfo {
  g: boolean;
  slot: number;
  q: Q;
  t: PType;
  /** History depth table the slot lives in (global or the function's). */
  depths: number[];
  /** Compile-time value for never-reassigned constants (input defaults may use them). */
  c?: Value;
}

export interface FuncInfo {
  id: number;
  def: FuncDef;
  depths: number[];
  /** Contains stateful calls or history references (warn when called conditionally). */
  stateful: boolean;
  /** Tuple size when the function's last expression is a tuple literal. */
  tuple?: number;
}

export interface ScriptMeta {
  title: string;
  shorttitle: string;
  overlay: boolean;
  format: string;
  precision: number | null;
  version: Version;
}

export interface CheckedScript {
  program: Program;
  meta: ScriptMeta;
  inputs: InputInfo[];
  globalDepths: number[];
  funcs: FuncInfo[];
  warnings: PineError[];
}

export interface CheckLimits {
  maxPlots: number;
}

/** What checkCall.ts needs from the checker. */
export interface CheckerApi {
  readonly version: Version;
  readonly limits: CheckLimits;
  expr(e: Expr, tupleOk?: boolean): Info;
  /** True inside a function body. */
  inFunction(): boolean;
  /** True inside an if/switch/loop body (or a function). */
  inLocalScope(): boolean;
  /** True where a call may be skipped on some bars. */
  conditional(): boolean;
  currentFunction(): FuncInfo | null;
  findFunction(name: string): FuncInfo | undefined;
  warn(message: string, at: Pos): void;
  addInput(info: Omit<InputInfo, "id">): number;
  countOutput(at: Pos): void;
  setMeta(meta: Partial<ScriptMeta>, at: Pos): void;
  /** Resolves a plain variable name to its compile-time constant, if any. */
  constOf(name: string): Value | undefined;
  stmts(body: Stmt[]): void;
}
