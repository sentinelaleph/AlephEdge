/**
 * Statement half of the Pine-lite parser (recursive descent). Produces the
 * Program AST; constructs Pine-lite v1 rejects at the syntax level (`varip`,
 * `type`, `method`, `import`, `export`, collection and drawing types) fail
 * here with their name and position. Name-level rejections (strategy.*,
 * request.*, ta.mode, ...) are the checker's job.
 */
import type { FuncDef, FuncParam, Program, Stmt } from "./ast";
import { fail, NOT_IN_V1 } from "./errors";
import { lex } from "./lexer";
import { ExprParser } from "./parseExpr";

export const TYPE_WORDS = new Set(["int", "float", "bool", "string", "color"]);
const QUALIFIERS = new Set(["const", "simple", "series", "input"]);
const UNSUPPORTED_TYPES: Record<string, string> = {
  array: "arrays", matrix: "matrices", map: "maps", line: "drawing objects (line)", label: "drawing objects (label)",
  box: "drawing objects (box)", table: "tables", linefill: "drawing objects (linefill)", polyline: "drawing objects (polyline)",
  chart: "drawing objects (chart.point)",
};
const ASSIGN_OPS = new Set([":=", "+=", "-=", "*=", "/=", "%="]);
/** Words that start statements or expressions, never a type name. */
const KEYWORDS = new Set(["if", "else", "for", "while", "switch", "var", "varip", "and", "or", "not", "true", "false", "na", "break", "continue", "import", "export"]);

class Parser extends ExprParser {
  program(): Stmt[] {
    const body: Stmt[] = [];
    for (;;) {
      while (this.peek().t === "nl") this.next();
      const t = this.peek();
      if (t.t === "eof") return body;
      if (t.t === "indent" || t.t === "dedent") fail("syntax", "Unexpected indentation", t);
      body.push(this.statement());
      this.terminator();
    }
  }

  protected block(): Stmt[] {
    this.expectNl();
    const t = this.peek();
    if (t.t !== "indent") this.unexpected("an indented block");
    this.next();
    this.enter(t);
    const body: Stmt[] = [];
    while (this.peek().t !== "dedent" && this.peek().t !== "eof") {
      body.push(this.statement());
      this.terminator();
      while (this.peek().t === "nl") this.next();
    }
    if (this.peek().t === "dedent") this.next();
    this.depth--;
    return body;
  }

  private terminator(): void {
    const t = this.peek();
    if (t.t === "nl") {
      while (this.peek().t === "nl") this.next();
      return;
    }
    if (t.t === "dedent" || t.t === "eof") return;
    if (this.toks[this.p - 1]?.t === "dedent") return;
    this.unexpected("end of line");
  }

  private statement(): Stmt {
    const t = this.peek();
    const at = { line: t.line, col: t.col };
    if (t.t === "op" && t.v === "[" && this.isTupleDecl()) return this.tupleDecl();
    if (t.t !== "ident") return { kind: "ExprStmt", expr: this.expr(), ...at };
    const word = t.v;
    if (word === "varip") fail("unsupported", `varip ${NOT_IN_V1}`, t);
    if (word === "import" || word === "export") fail("unsupported", `${word} ${NOT_IN_V1}`, t);
    if (word === "method" && this.peek(1).t === "ident") fail("unsupported", `Methods (method) ${NOT_IN_V1}`, t);
    if (word === "type" && this.peek(1).t === "ident") fail("unsupported", `User-defined types (type) ${NOT_IN_V1}`, t);
    if (word === "var") {
      this.next();
      return this.declaration(at, "var");
    }
    if (word === "break" || word === "continue") {
      this.next();
      return { kind: word === "break" ? "Break" : "Continue", ...at };
    }
    if (this.startsDeclaration()) return this.declaration(at, null);
    if (this.isOp("(", 1) && this.isFuncDef()) return this.funcDef();
    const nextTok = this.peek(1);
    if (nextTok.t === "op" && nextTok.v === "=") {
      this.next();
      this.next();
      return { kind: "VarDecl", name: t.v, mode: null, type: null, init: this.expr(), ...at };
    }
    if (nextTok.t === "op" && ASSIGN_OPS.has(nextTok.v)) {
      this.next();
      this.next();
      return { kind: "Assign", name: t.v, op: nextTok.v as ":=", value: this.expr(), ...at };
    }
    return { kind: "ExprStmt", expr: this.expr(), ...at };
  }

  /** `[qualifier] type [[]] name = ...` or `Name name = ...` (a UDT). */
  private startsDeclaration(): boolean {
    const a = this.peek();
    const b = this.peek(1);
    if (a.t !== "ident" || KEYWORDS.has(a.v)) return false;
    if (QUALIFIERS.has(a.v) && b.t === "ident") return true;
    if (b.t === "ident" && this.isOp("=", 2)) return true;
    if (UNSUPPORTED_TYPES[a.v] !== undefined && this.isOp("<", 1)) return true;
    return TYPE_WORDS.has(a.v) && this.isOp("[", 1) && this.isOp("]", 2);
  }

  private declaration(at: { line: number; col: number }, mode: "var" | null): Stmt {
    const type = this.typeSpec();
    if (this.isOp("[") && type === null) {
      if (mode === "var") fail("syntax", "Tuple declarations cannot use 'var'", at);
      return this.tupleDecl();
    }
    const name = this.expectIdent("a variable name");
    this.expectOp("=");
    return { kind: "VarDecl", name: name.v, mode, type, init: this.expr(), ...at };
  }

  /** Optional `[qualifier] type` prefix; rejects collection, drawing and user types. */
  private typeSpec(): string | null {
    while (this.peek().t === "ident" && QUALIFIERS.has(this.peek().v) && this.peek(1).t === "ident") this.next();
    const t = this.peek();
    if (t.t !== "ident") return null;
    const unsupported = UNSUPPORTED_TYPES[t.v];
    const typed = this.peek(1).t === "ident" || this.isOp("<", 1) || (this.isOp("[", 1) && this.isOp("]", 2));
    if (!typed) return null;
    if (unsupported !== undefined) fail("unsupported", `${t.v}: ${unsupported} ${NOT_IN_V1}`, t);
    if (!TYPE_WORDS.has(t.v)) fail("unsupported", `User-defined types (${t.v}) ${NOT_IN_V1}`, t);
    this.next();
    if (this.isOp("[")) fail("unsupported", `${t.v}[]: arrays ${NOT_IN_V1}`, t);
    return t.v;
  }

  private isTupleDecl(): boolean {
    let k = 1;
    for (;;) {
      if (this.peek(k).t !== "ident") return false;
      k++;
      if (this.isOp("]", k)) return this.isOp("=", k + 1);
      if (!this.isOp(",", k)) return false;
      k++;
    }
  }

  private tupleDecl(): Stmt {
    const t = this.expectOp("[");
    const names = [this.expectIdent().v];
    while (this.isOp(",")) {
      this.next();
      names.push(this.expectIdent().v);
    }
    this.expectOp("]");
    this.expectOp("=");
    return { kind: "TupleDecl", names, init: this.expr(), line: t.line, col: t.col };
  }

  private isFuncDef(): boolean {
    let k = 1;
    let depth = 0;
    for (;;) {
      const t = this.peek(k);
      if (t.t === "eof" || t.t === "nl") return false;
      if (t.t === "op" && (t.v === "(" || t.v === "[")) depth++;
      if (t.t === "op" && (t.v === ")" || t.v === "]")) {
        depth--;
        if (depth === 0) return this.isOp("=>", k + 1);
      }
      k++;
    }
  }

  private funcDef(): FuncDef {
    const name = this.next();
    this.expectOp("(");
    const params: FuncParam[] = [];
    while (!this.isOp(")")) {
      const p0 = this.peek();
      const type = this.typeSpec();
      const pn = this.expectIdent("a parameter name");
      let def = null;
      if (this.isOp("=")) {
        this.next();
        def = this.expr();
      }
      params.push({ name: pn.v, def, type, line: p0.line, col: p0.col });
      if (!this.isOp(",")) break;
      this.next();
    }
    this.expectOp(")");
    this.expectOp("=>");
    const body = this.peek().t === "nl" ? this.block() : this.inlineBody();
    return { kind: "FuncDef", name: name.v, params, body, line: name.line, col: name.col };
  }

  siteCount(): number {
    return this.sites;
  }
}

/** Lexes and parses; throws PineFailure on the first syntax error. */
export function parse(source: string): Program {
  const { tokens, version, versionPos } = lex(source);
  const p = new Parser(tokens);
  const body = p.program();
  return { body, version, versionPos, siteCount: p.siteCount() };
}
