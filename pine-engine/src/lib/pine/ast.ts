/**
 * Pine-lite AST. Every node carries a 1-based line/col: its first token,
 * except Binary nodes, which carry their operator's position (so operator
 * errors point at the operator).
 * The parser builds the tree; the checker annotates it in place (the optional
 * fields below) and the interpreter only reads it.
 */
import type { BuiltinFn, BuiltinVar } from "./runtime/builtins/types";
import type { Value } from "./runtime/values";

export interface Pos {
  line: number;
  col: number;
}

/** Where an identifier resolves. Set by the checker. */
export type Ref =
  /** A variable slot: global instance (g) or the current function instance. */
  | { k: "slot"; g: boolean; i: number }
  /** A built-in series/simple variable (close, bar_index, ta.tr, ...). */
  | { k: "bv"; v: BuiltinVar }
  /** A compile-time constant (color.red, shape.circle, math.pi, ...). */
  | { k: "const"; v: Value };

export interface NumLit extends Pos { kind: "Num"; value: number; isInt: boolean }
export interface StrLit extends Pos { kind: "Str"; value: string }
export interface BoolLit extends Pos { kind: "Bool"; value: boolean }
export interface NaLit extends Pos { kind: "Na" }
export interface ColorLit extends Pos { kind: "Color"; hex: string }
export interface Ident extends Pos { kind: "Ident"; name: string; ref?: Ref }

export interface NamedArg extends Pos { name: string; value: Expr }

export interface Call extends Pos {
  kind: "Call";
  callee: string;
  args: Expr[];
  named: NamedArg[];
  /** Unique call-site id (per script), assigned by the parser. */
  site: number;
  /** Arguments in the callee's canonical parameter order; null = default. */
  bound?: (Expr | null)[];
  def?: BuiltinFn;
  /** Index into CheckedScript.funcs for user functions. */
  user?: number;
  /** Index into the declared inputs for input.* calls. */
  input?: number;
  /** Marks the indicator() declaration (skipped at run time). */
  decl?: true;
}

export interface Index extends Pos {
  kind: "Index";
  target: Expr;
  offset: Expr;
  /** Hidden history slot when the target is not a plain variable. */
  hidden?: number;
}

export interface Unary extends Pos { kind: "Unary"; op: "-" | "+" | "not"; operand: Expr }
export interface Binary extends Pos { kind: "Binary"; op: string; left: Expr; right: Expr; intDiv?: true }
export interface Ternary extends Pos { kind: "Ternary"; cond: Expr; a: Expr; b: Expr }
export interface TupleLit extends Pos { kind: "Tuple"; items: Expr[] }

export interface IfExpr extends Pos {
  kind: "If";
  cond: Expr;
  then: Stmt[];
  /** `else if` chains are a nested IfExpr in a one-element block. */
  else: Stmt[] | null;
}

export interface SwitchCase extends Pos {
  /** null for the default (`=>`) case. */
  match: Expr | null;
  body: Stmt[];
}

export interface SwitchExpr extends Pos { kind: "Switch"; subject: Expr | null; cases: SwitchCase[] }

export interface ForExpr extends Pos {
  kind: "For";
  varName: string;
  from: Expr;
  to: Expr;
  step: Expr | null;
  body: Stmt[];
  slot?: number;
}

export interface WhileExpr extends Pos { kind: "While"; cond: Expr; body: Stmt[] }

export type Expr =
  | NumLit | StrLit | BoolLit | NaLit | ColorLit | Ident | Call | Index
  | Unary | Binary | Ternary | TupleLit | IfExpr | SwitchExpr | ForExpr | WhileExpr;

export interface VarDecl extends Pos {
  kind: "VarDecl";
  name: string;
  mode: "var" | null;
  /** Declared type keyword (int, float, bool, string, color), if any. */
  type: string | null;
  init: Expr;
  slot?: number;
}

export interface TupleDecl extends Pos { kind: "TupleDecl"; names: string[]; init: Expr; slots?: number[] }

export interface Assign extends Pos {
  kind: "Assign";
  name: string;
  op: ":=" | "+=" | "-=" | "*=" | "/=" | "%=";
  value: Expr;
  ref?: { g: boolean; i: number };
}

export interface ExprStmt extends Pos { kind: "ExprStmt"; expr: Expr }

export interface FuncParam extends Pos { name: string; def: Expr | null; type: string | null }

export interface FuncDef extends Pos {
  kind: "FuncDef";
  name: string;
  params: FuncParam[];
  body: Stmt[];
  id?: number;
}

export interface BreakStmt extends Pos { kind: "Break" }
export interface ContinueStmt extends Pos { kind: "Continue" }

export type Stmt = VarDecl | TupleDecl | Assign | ExprStmt | FuncDef | BreakStmt | ContinueStmt;

export interface Program {
  body: Stmt[];
  /** From `//@version=N`; null when absent. */
  version: number | null;
  versionPos: Pos | null;
  /** Number of call sites (ids are 0..siteCount-1). */
  siteCount: number;
}
