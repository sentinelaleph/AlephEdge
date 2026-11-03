/**
 * Canonical source printer for the round-trip property test: print(ast) is
 * valid Pine that parses back to the same AST (positions aside). Binary,
 * unary and ternary expressions are fully parenthesised, functions always use
 * the block form.
 */
import type { Expr, Stmt } from "@/lib/pine/ast";

const pad = (n: number) => "    ".repeat(n);

function num(v: number, isInt: boolean): string {
  const s = String(v);
  return isInt || /[.e]/.test(s) ? s : `${s}.0`;
}

export function printExpr(e: Expr, ind: number): string {
  switch (e.kind) {
    case "Num":
      return num(e.value, e.isInt);
    case "Str":
      return JSON.stringify(e.value);
    case "Bool":
      return String(e.value);
    case "Na":
      return "na";
    case "Color":
      return e.hex;
    case "Ident":
      return e.name;
    case "Call": {
      const args = [...e.args.map((a) => printExpr(a, ind)), ...e.named.map((n) => `${n.name} = ${printExpr(n.value, ind)}`)];
      return `${e.callee}(${args.join(", ")})`;
    }
    case "Index":
      return `(${printExpr(e.target, ind)})[${printExpr(e.offset, ind)}]`;
    case "Unary":
      return e.op === "not" ? `(not ${printExpr(e.operand, ind)})` : `(${e.op}${printExpr(e.operand, ind)})`;
    case "Binary":
      return `(${printExpr(e.left, ind)} ${e.op} ${printExpr(e.right, ind)})`;
    case "Ternary":
      return `(${printExpr(e.cond, ind)} ? ${printExpr(e.a, ind)} : ${printExpr(e.b, ind)})`;
    case "Tuple":
      return `[${e.items.map((x) => printExpr(x, ind)).join(", ")}]`;
    case "If": {
      let s = `if ${printExpr(e.cond, ind)}\n${printBlock(e.then, ind + 1)}`;
      const els = e.else;
      if (els) {
        const only = els.length === 1 && els[0].kind === "ExprStmt" && els[0].expr.kind === "If" ? els[0].expr : null;
        s += only ? `\n${pad(ind)}else ${printExpr(only, ind)}` : `\n${pad(ind)}else\n${printBlock(els, ind + 1)}`;
      }
      return s;
    }
    case "Switch": {
      const head = e.subject ? `switch ${printExpr(e.subject, ind)}` : "switch";
      const cases = e.cases.map((c) => `${pad(ind + 1)}${c.match ? printExpr(c.match, ind + 1) + " " : ""}=>\n${printBlock(c.body, ind + 2)}`);
      return [head, ...cases].join("\n");
    }
    case "For":
      return `for ${e.varName} = ${printExpr(e.from, ind)} to ${printExpr(e.to, ind)}${e.step ? ` by ${printExpr(e.step, ind)}` : ""}\n${printBlock(e.body, ind + 1)}`;
    case "While":
      return `while ${printExpr(e.cond, ind)}\n${printBlock(e.body, ind + 1)}`;
  }
}

export function printStmt(s: Stmt, ind: number): string {
  switch (s.kind) {
    case "VarDecl":
      return `${s.mode ? "var " : ""}${s.type ? s.type + " " : ""}${s.name} = ${printExpr(s.init, ind)}`;
    case "TupleDecl":
      return `[${s.names.join(", ")}] = ${printExpr(s.init, ind)}`;
    case "Assign":
      return `${s.name} ${s.op} ${printExpr(s.value, ind)}`;
    case "ExprStmt":
      return printExpr(s.expr, ind);
    case "FuncDef": {
      const params = s.params.map((p) => `${p.type ? p.type + " " : ""}${p.name}${p.def ? ` = ${printExpr(p.def, ind)}` : ""}`);
      return `${s.name}(${params.join(", ")}) =>\n${printBlock(s.body, ind + 1)}`;
    }
    case "Break":
      return "break";
    case "Continue":
      return "continue";
  }
}

export function printBlock(body: Stmt[], ind: number): string {
  return body.map((s) => pad(ind) + printStmt(s, ind)).join("\n");
}

export function printProgram(body: Stmt[], version: number | null): string {
  return `${version === null ? "" : `//@version=${version}\n`}${printBlock(body, 0)}\n`;
}

/** The AST without positions and call-site ids, for structural comparison. */
export function shape(x: unknown): unknown {
  return JSON.parse(JSON.stringify(x, (k, v) => (k === "line" || k === "col" || k === "site" ? undefined : v)));
}
