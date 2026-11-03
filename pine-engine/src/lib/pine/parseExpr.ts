/**
 * Expression half of the Pine-lite parser: a Pratt parser over the token
 * stream. Statement parsing (parser.ts) extends this class and supplies
 * `block()` so that `if`/`switch`/`for`/`while` work in expression position.
 *
 * Precedence (Pine): ?: < or < and < == != < comparisons < + - < * / % <
 * unary (- + not) < postfix ([] history, call).
 */
import type { Call, Expr, NamedArg, Pos, Stmt } from "./ast";
import { fail, NOT_IN_V1 } from "./errors";
import type { Token } from "./lexer";

const BINARY: Record<string, number> = {
  or: 2, and: 3, "==": 4, "!=": 4, "<": 5, ">": 5, "<=": 5, ">=": 5, "+": 6, "-": 6, "*": 7, "/": 7, "%": 7,
};
const UNARY_BP = 8;
export const MAX_DEPTH = 120;

export abstract class ExprParser {
  protected p = 0;
  protected depth = 0;
  protected sites = 0;

  constructor(protected readonly toks: Token[]) {}

  protected abstract block(): Stmt[];

  protected peek(k = 0): Token {
    return this.toks[Math.min(this.p + k, this.toks.length - 1)];
  }

  protected next(): Token {
    const t = this.toks[this.p];
    if (this.p < this.toks.length - 1) this.p++;
    return t;
  }

  protected isOp(v: string, k = 0): boolean {
    const t = this.peek(k);
    return t.t === "op" && t.v === v;
  }

  protected isWord(v: string, k = 0): boolean {
    const t = this.peek(k);
    return t.t === "ident" && t.v === v;
  }

  protected expectOp(v: string): Token {
    if (!this.isOp(v)) this.unexpected(`'${v}'`);
    return this.next();
  }

  protected expectIdent(what = "a name"): Token {
    const t = this.peek();
    if (t.t !== "ident") this.unexpected(what);
    return this.next();
  }

  protected unexpected(expected: string): never {
    const t = this.peek();
    const got = t.t === "nl" ? "end of line" : t.t === "eof" ? "end of script" : t.t === "indent" ? "an indented block" : t.t === "dedent" ? "end of block" : `'${t.v}'`;
    fail("syntax", `Expected ${expected} but found ${got}`, t);
  }

  protected enter(at: Pos): void {
    if (++this.depth > MAX_DEPTH) fail("limit", `Script is nested more than ${MAX_DEPTH} levels deep`, at);
  }

  expr(minBp = 0): Expr {
    const start = this.peek();
    this.enter(start);
    let left = this.prefix();
    for (;;) {
      const t = this.peek();
      if (t.t === "op" && t.v === "[") {
        this.next();
        const offset = this.expr();
        this.expectOp("]");
        left = { kind: "Index", target: left, offset, line: left.line, col: left.col };
        continue;
      }
      if (t.t === "op" && t.v === "?" && minBp <= 1) {
        this.next();
        const a = this.expr(1);
        this.expectOp(":");
        const b = this.expr(1);
        left = { kind: "Ternary", cond: left, a, b, line: left.line, col: left.col };
        continue;
      }
      const key = t.t === "op" || (t.t === "ident" && (t.v === "and" || t.v === "or")) ? t.v : "";
      const bp = BINARY[key];
      if (bp === undefined || bp <= minBp) break;
      this.next();
      const right = this.expr(bp);
      left = { kind: "Binary", op: key, left, right, line: t.line, col: t.col };
    }
    this.depth--;
    return left;
  }

  private prefix(): Expr {
    const t = this.peek();
    const at = { line: t.line, col: t.col };
    if (t.t === "num") {
      this.next();
      return { kind: "Num", value: t.n as number, isInt: t.int === true, ...at };
    }
    if (t.t === "str") {
      this.next();
      return { kind: "Str", value: t.v, ...at };
    }
    if (t.t === "color") {
      this.next();
      return { kind: "Color", hex: t.v, ...at };
    }
    if (t.t === "op") {
      if (t.v === "(") {
        this.next();
        const e = this.expr();
        this.expectOp(")");
        return e;
      }
      if (t.v === "-" || t.v === "+") {
        this.next();
        return { kind: "Unary", op: t.v, operand: this.expr(UNARY_BP), ...at };
      }
      if (t.v === "[") {
        this.next();
        const items: Expr[] = [];
        if (!this.isOp("]")) {
          items.push(this.expr());
          while (this.isOp(",")) {
            this.next();
            items.push(this.expr());
          }
        }
        this.expectOp("]");
        return { kind: "Tuple", items, ...at };
      }
    }
    if (t.t !== "ident") this.unexpected("an expression");
    switch (t.v) {
      case "true":
      case "false":
        this.next();
        return { kind: "Bool", value: t.v === "true", ...at };
      case "not":
        this.next();
        return { kind: "Unary", op: "not", operand: this.expr(UNARY_BP), ...at };
      case "if":
        return this.ifExpr();
      case "switch":
        return this.switchExpr();
      case "for":
        return this.forExpr();
      case "while": {
        this.next();
        const cond = this.expr();
        return { kind: "While", cond, body: this.block(), ...at };
      }
      case "na":
        if (!this.isOp("(", 1)) {
          this.next();
          return { kind: "Na", ...at };
        }
    }
    const name = this.dotted();
    if (this.isOp("<") && /^(array|matrix|map)(\.|$)/.test(name)) fail("unsupported", `${name}<...> (collections) ${NOT_IN_V1}`, at);
    if (this.isOp("(")) return this.call(name, at);
    return { kind: "Ident", name, ...at };
  }

  /** a.b.c as one qualified name (Pine v1 has no objects, only namespaces). */
  protected dotted(): string {
    let name = this.expectIdent().v;
    while (this.isOp(".") && this.peek(1).t === "ident") {
      this.next();
      name += "." + this.next().v;
    }
    return name;
  }

  protected call(callee: string, at: Pos): Call {
    this.expectOp("(");
    const args: Expr[] = [];
    const named: NamedArg[] = [];
    while (!this.isOp(")")) {
      const t = this.peek();
      if (t.t === "ident" && this.isOp("=", 1)) {
        this.next();
        this.next();
        named.push({ name: t.v, value: this.expr(), line: t.line, col: t.col });
      } else {
        if (named.length > 0) fail("syntax", "Positional arguments cannot follow named arguments", t);
        args.push(this.expr());
      }
      if (!this.isOp(",")) break;
      this.next();
    }
    this.expectOp(")");
    return { kind: "Call", callee, args, named, site: this.sites++, ...at };
  }

  private ifExpr(): Expr {
    const t = this.next();
    const cond = this.expr();
    const then = this.block();
    let els: Stmt[] | null = null;
    if (this.isWord("else")) {
      this.next();
      if (this.isWord("if")) {
        const nested = this.ifExpr();
        els = [{ kind: "ExprStmt", expr: nested, line: nested.line, col: nested.col }];
      } else {
        els = this.block();
      }
    }
    return { kind: "If", cond, then, else: els, line: t.line, col: t.col };
  }

  private switchExpr(): Expr {
    const t = this.next();
    const subject = this.peek().t === "nl" ? null : this.expr();
    this.expectNl();
    if (this.peek().t !== "indent") this.unexpected("an indented list of cases");
    this.next();
    const cases = [];
    while (this.peek().t !== "dedent" && this.peek().t !== "eof") {
      const c = this.peek();
      const match = this.isOp("=>") ? null : this.expr();
      this.expectOp("=>");
      const body = this.peek().t === "nl" ? this.block() : this.inlineBody();
      cases.push({ match, body, line: c.line, col: c.col });
      while (this.peek().t === "nl") this.next();
    }
    if (this.peek().t === "dedent") this.next();
    return { kind: "Switch", subject, cases, line: t.line, col: t.col };
  }

  private forExpr(): Expr {
    const t = this.next();
    if (this.isOp("[") || this.isWord("in", 1)) fail("unsupported", `for...in loops (arrays) ${NOT_IN_V1}`, t);
    const v = this.expectIdent("a loop variable");
    this.expectOp("=");
    const from = this.expr();
    if (!this.isWord("to")) this.unexpected("'to'");
    this.next();
    const to = this.expr();
    let step: Expr | null = null;
    if (this.isWord("by")) {
      this.next();
      step = this.expr();
    }
    return { kind: "For", varName: v.v, from, to, step, body: this.block(), line: t.line, col: t.col };
  }

  /** A single expression body on the same line (switch case, `=>` function). */
  protected inlineBody(): Stmt[] {
    const e = this.expr();
    return [{ kind: "ExprStmt", expr: e, line: e.line, col: e.col }];
  }

  protected expectNl(): void {
    if (this.peek().t !== "nl") this.unexpected("end of line");
    while (this.peek().t === "nl") this.next();
  }
}
