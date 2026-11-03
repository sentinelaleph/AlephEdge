/**
 * The static pass. Runs before any bar executes and either throws the first
 * PineError or returns a CheckedScript: version resolved, exactly one
 * indicator(), every identifier resolved (variable slot, built-in variable or
 * constant), every call bound (checkCall.ts), history depths computed per
 * slot, inputs collected, warnings for stateful calls in conditional scopes.
 */
import type { Expr, FuncDef, Pos, Program, Stmt } from "./ast";
import { checkCall } from "./checkCall";
import type { CheckedScript, CheckerApi, CheckLimits, FuncInfo, Info, ScriptMeta, VarInfo } from "./checkTypes";
import { joinQ } from "./checkTypes";
import { evalConst, NOT_CONST } from "./constEval";
import { fail, pineError, type PineError } from "./errors";
import { CONSTANT_VALUES, FUNCTIONS, unsupportedReason, VARIABLES } from "./registry";
import type { InputInfo } from "./runtime/builtins/input";
import { DEFAULT_HISTORY } from "./runtime/series";
import type { Value, Version } from "./runtime/values";

interface Scope {
  vars: Map<string, VarInfo>;
  /** Block kind: function bodies stop global-assignment checks; loops allow break. */
  kind: "global" | "function" | "block" | "loop";
}

class Checker implements CheckerApi {
  readonly version: Version;
  private scopes: Scope[] = [{ vars: new Map(), kind: "global" }];
  private funcs = new Map<string, FuncInfo>();
  private funcList: FuncInfo[] = [];
  private fn: FuncInfo | null = null;
  private globalDepths: number[] = [];
  private cond = 0;
  private inputs: InputInfo[] = [];
  private outputs = 0;
  private warnings: PineError[] = [];
  private meta: ScriptMeta | null = null;

  constructor(private readonly program: Program, readonly limits: CheckLimits) {
    const v = program.version;
    const at = program.versionPos ?? { line: 1, col: 1 };
    if (v === null) this.warn("No //@version annotation; assuming //@version=6", at);
    else if (v < 5) fail("unsupported", `Pine v${v} is not supported; Pine-lite runs v5 and v6 scripts`, at, "Convert the script to //@version=5 or 6");
    else if (v > 6) fail("unsupported", `Pine v${v} is not supported; Pine-lite runs v5 and v6 scripts`, at);
    this.version = v === 5 ? 5 : 6;
  }

  run(): CheckedScript {
    this.stmts(this.program.body);
    if (!this.meta) fail("type", "The script must declare indicator(...)", { line: 1, col: 1 }, 'Add indicator("My script") after //@version=6');
    return { program: this.program, meta: this.meta, inputs: this.inputs, globalDepths: this.globalDepths, funcs: this.funcList, warnings: this.warnings };
  }

  // ---- CheckerApi ----
  inFunction = () => this.fn !== null;
  inLocalScope = () => this.scopes.length > 1;
  conditional = () => this.cond > 0;
  currentFunction = () => this.fn;
  findFunction = (name: string) => this.funcs.get(name);
  warn(message: string, at: Pos): void {
    if (!this.warnings.some((w) => w.line === at.line && w.col === at.col && w.message === message)) this.warnings.push(pineError("warning", message, at.line, at.col));
  }
  addInput(info: Omit<InputInfo, "id">): number {
    this.inputs.push({ id: this.inputs.length, ...info });
    return this.inputs.length - 1;
  }
  countOutput(at: Pos): void {
    if (++this.outputs > this.limits.maxPlots) fail("limit", `Too many outputs: Pine-lite allows at most ${this.limits.maxPlots} plot calls per script`, at);
  }
  setMeta(meta: Partial<ScriptMeta>, at: Pos): void {
    if (this.meta) fail("type", "indicator() can only be declared once", at);
    this.meta = { title: "", shorttitle: "", overlay: false, format: "inherit", precision: null, ...meta, version: this.version };
  }
  constOf(name: string): Value | undefined {
    return this.lookup(name)?.c;
  }

  // ---- scopes and slots ----
  private lookup(name: string): VarInfo | undefined {
    for (let i = this.scopes.length - 1; i >= 0; i--) {
      const v = this.scopes[i].vars.get(name);
      if (v) return v;
    }
    return undefined;
  }

  private depths(): number[] {
    return this.fn ? this.fn.depths : this.globalDepths;
  }

  private alloc(): number {
    this.depths().push(0);
    return this.depths().length - 1;
  }

  private declare(name: string, at: Pos, info: Info): VarInfo {
    const scope = this.scopes[this.scopes.length - 1];
    if (name !== "_" && scope.vars.has(name)) fail("name", `'${name}' is already declared in this scope; use := to reassign it`, at);
    const v: VarInfo = { g: this.fn === null, slot: this.alloc(), q: info.q, t: info.t, depths: this.depths(), c: info.c };
    scope.vars.set(name, v);
    return v;
  }

  private block(body: Stmt[], kind: Scope["kind"], tupleOk = false): Info {
    this.scopes.push({ vars: new Map(), kind });
    let last: Info = { t: "any", q: "unknown" };
    body.forEach((s, i) => {
      last = this.stmt(s, tupleOk && i === body.length - 1);
    });
    this.scopes.pop();
    return last;
  }

  stmts(body: Stmt[]): void {
    for (const s of body) this.stmt(s, false);
  }

  // ---- statements ----
  private stmt(s: Stmt, tupleOk: boolean): Info {
    switch (s.kind) {
      case "VarDecl": {
        const info = s.init.kind === "Call" ? checkCall(this, s.init, true, s.name) : this.expr(s.init);
        const c = s.mode === null ? constValue(this, s.init) : undefined;
        s.slot = this.declare(s.name, s, { ...info, c }).slot;
        return info;
      }
      case "TupleDecl": {
        const info = this.expr(s.init, true);
        if (s.init.kind === "Call" && s.init.def && s.init.def.returns !== "tuple") fail("type", `${s.init.callee} does not return a tuple`, s.init);
        const size = s.init.kind === "Call" ? s.init.def?.tuple ?? this.funcList[s.init.user ?? -1]?.tuple : undefined;
        if (size !== undefined && size !== s.names.length) fail("type", `${s.init.kind === "Call" ? s.init.callee : "Expression"} returns ${size} values, but ${s.names.length} names are declared`, s);
        s.slots = s.names.map((n) => this.declare(n, s, { t: "any", q: info.q === "const" ? "unknown" : "series" }).slot);
        return info;
      }
      case "Assign":
        return this.assign(s);
      case "ExprStmt":
        return s.expr.kind === "Call" ? checkCall(this, s.expr, true, null) : this.expr(s.expr, tupleOk);
      case "FuncDef":
        this.funcDef(s);
        return { t: "void", q: "const" };
      case "Break":
      case "Continue":
        if (!this.scopes.slice(this.fnScopeIndex()).some((sc) => sc.kind === "loop")) fail("syntax", `'${s.kind.toLowerCase()}' outside of a loop`, s);
        return { t: "void", q: "const" };
    }
  }

  private fnScopeIndex(): number {
    for (let i = this.scopes.length - 1; i >= 0; i--) if (this.scopes[i].kind === "function") return i;
    return 0;
  }

  private assign(s: Extract<Stmt, { kind: "Assign" }>): Info {
    const v = this.lookup(s.name);
    if (!v) {
      if (VARIABLES.has(s.name) || s.name in CONSTANT_VALUES) fail("type", `Cannot modify the built-in '${s.name}'`, s);
      fail("name", `Undeclared identifier '${s.name}'`, s, s.op === ":=" ? `Declare it first with ${s.name} = ...` : undefined);
    }
    if (this.fn && v.g) fail("type", `Cannot modify global variable '${s.name}' in a function`, s);
    const info = this.expr(s.value);
    v.q = joinQ(joinQ(v.q, info.q), "series");
    v.c = undefined;
    s.ref = { g: v.g, i: v.slot };
    return info;
  }

  private funcDef(f: FuncDef): void {
    if (this.scopes.length > 1) fail("type", "Functions can only be declared in the global scope", f);
    if (this.funcs.has(f.name)) fail("name", `Function '${f.name}' is already declared`, f);
    if (FUNCTIONS.has(f.name) || VARIABLES.has(f.name)) fail("name", `'${f.name}' is a built-in and cannot be redefined`, f);
    const info: FuncInfo = { id: this.funcList.length, def: f, depths: [], stateful: false };
    f.id = info.id;
    for (const p of f.params) if (p.def) this.expr(p.def);
    this.fn = info;
    this.scopes.push({ vars: new Map(), kind: "function" });
    for (const p of f.params) this.declare(p.name, p, { t: "any", q: "unknown" });
    const last = f.body[f.body.length - 1];
    f.body.forEach((s) => this.stmt(s, s === last));
    if (last?.kind === "ExprStmt" && last.expr.kind === "Tuple") info.tuple = last.expr.items.length;
    this.scopes.pop();
    this.fn = null;
    this.funcs.set(f.name, info);
    this.funcList.push(info);
  }

  // ---- expressions ----
  expr(e: Expr, tupleOk = false): Info {
    switch (e.kind) {
      case "Num":
        return { t: e.isInt ? "int" : "float", q: "const", c: e.value };
      case "Str":
        return { t: "string", q: "const", c: e.value };
      case "Bool":
        return { t: "bool", q: "const", c: e.value };
      case "Na":
        return { t: "any", q: "const", c: null };
      case "Color":
        return { t: "color", q: "const" };
      case "Ident":
        return this.ident(e);
      case "Call":
        return checkCall(this, e, false, null);
      case "Index":
        return this.index(e);
      case "Unary": {
        const i = this.expr(e.operand);
        return { t: e.op === "not" ? "bool" : i.t, q: i.q };
      }
      case "Binary":
        return this.binary(e);
      case "Ternary": {
        const c = this.expr(e.cond);
        this.cond++;
        const a = this.expr(e.a, tupleOk);
        const b = this.expr(e.b, tupleOk);
        this.cond--;
        return { t: a.t === "any" ? b.t : a.t, q: joinQ(c.q, joinQ(a.q, b.q)) };
      }
      case "Tuple":
        if (!tupleOk) fail("type", "Tuples like [a, b] can only be returned from functions", e);
        e.items.forEach((x) => this.expr(x));
        return { t: "tuple", q: "series" };
      case "If":
      case "Switch":
      case "For":
      case "While":
        return this.control(e, tupleOk);
    }
  }

  private ident(e: Extract<Expr, { kind: "Ident" }>): Info {
    const v = this.lookup(e.name);
    if (v) {
      e.ref = { k: "slot", g: v.g, i: v.slot };
      return { t: v.t, q: v.q, c: v.c };
    }
    const bv = VARIABLES.get(e.name);
    if (bv) {
      e.ref = { k: "bv", v: bv };
      return { t: bv.type, q: bv.q };
    }
    if (e.name in CONSTANT_VALUES) {
      e.ref = { k: "const", v: CONSTANT_VALUES[e.name] };
      return { t: "any", q: "const", c: CONSTANT_VALUES[e.name] };
    }
    if (FUNCTIONS.has(e.name) || this.funcs.has(e.name)) fail("type", `'${e.name}' is a function; call it with ( )`, e);
    const u = unsupportedReason(e.name);
    if (u) fail("unsupported", u.message, e, u.hint);
    return fail("name", `Undeclared identifier '${e.name}'`, e);
  }

  private index(e: Extract<Expr, { kind: "Index" }>): Info {
    const target = this.expr(e.target);
    this.expr(e.offset);
    let depth = DEFAULT_HISTORY;
    if (e.offset.kind === "Num" || (e.offset.kind === "Unary" && e.offset.operand.kind === "Num")) {
      const k = evalConst(e.offset, this.version, () => undefined) as number;
      if (k < 0) fail("type", "History offsets must not be negative", e.offset);
      if (!Number.isInteger(k)) fail("type", "History offsets must be integers", e.offset);
      depth = k;
    }
    if (this.fn) this.fn.stateful = true;
    const t = e.target;
    if (t.kind === "Ident" && t.ref?.k === "slot") {
      const v = this.lookup(t.name) as VarInfo;
      v.depths[v.slot] = Math.max(v.depths[v.slot], depth);
    } else if (!(t.kind === "Ident" && t.ref?.k === "bv")) {
      e.hidden = this.alloc();
      this.depths()[e.hidden] = depth;
    }
    return { t: target.t, q: "series" };
  }

  private binary(e: Extract<Expr, { kind: "Binary" }>): Info {
    const a = this.expr(e.left);
    const lazy = this.version === 6 && (e.op === "and" || e.op === "or");
    if (lazy) this.cond++;
    const b = this.expr(e.right);
    if (lazy) this.cond--;
    const q = joinQ(a.q, b.q);
    if (e.op === "and" || e.op === "or" || ["==", "!=", "<", ">", "<=", ">="].includes(e.op)) return { t: "bool", q };
    const isNum = (t: string) => t === "int" || t === "float";
    if ((a.t === "string" && isNum(b.t)) || (b.t === "string" && isNum(a.t)))
      fail("type", `Cannot apply '${e.op}' to a string and a number; use str.tostring()`, e);
    if (e.op === "/" && this.version === 5 && a.t === "int" && b.t === "int" && a.q === "const" && b.q === "const") e.intDiv = true;
    const t = a.t === "string" ? "string" : a.t === "float" || b.t === "float" || e.op === "/" ? (e.intDiv ? "int" : "float") : a.t === "int" && b.t === "int" ? "int" : "any";
    return { t, q };
  }

  private control(e: Extract<Expr, { kind: "If" | "Switch" | "For" | "While" }>, tupleOk: boolean): Info {
    this.cond++;
    let q: Info["q"] = "series";
    if (e.kind === "If") {
      q = this.expr(e.cond).q;
      this.block(e.then, "block", tupleOk);
      if (e.else) this.block(e.else, "block", tupleOk);
    } else if (e.kind === "Switch") {
      if (e.subject) this.expr(e.subject);
      for (const c of e.cases) {
        if (c.match) this.expr(c.match);
        this.block(c.body, "block", tupleOk);
      }
    } else if (e.kind === "For") {
      this.expr(e.from);
      this.expr(e.to);
      if (e.step) this.expr(e.step);
      this.scopes.push({ vars: new Map(), kind: "loop" });
      e.slot = this.declare(e.varName, e, { t: "int", q: "series" }).slot;
      this.block(e.body, "loop");
      this.scopes.pop();
    } else {
      this.expr(e.cond);
      this.block(e.body, "loop");
    }
    this.cond--;
    return { t: "any", q: joinQ(q, "series") };
  }
}

function constValue(ck: Checker, e: Expr): Value | undefined {
  const v = evalConst(e, ck.version, (n) => ck.constOf(n));
  return v === NOT_CONST ? undefined : v;
}

export function check(program: Program, limits: CheckLimits): CheckedScript {
  return new Checker(program, limits).run();
}
