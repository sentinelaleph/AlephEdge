/**
 * Bar-by-bar execution of a CheckedScript (Pine's execution model): the whole
 * script runs once per bar, oldest to newest; `var` initialises once; plain
 * declarations re-initialise on every execution; user functions and stateful
 * built-ins keep state per (function instance, call site).
 */
import type { Bar } from "@/lib/ta/core";

import type { Call, Expr, Stmt } from "../ast";
import type { CheckedScript } from "../checkTypes";
import { fail } from "../errors";
import type { Output } from "../outputs";
import type { InputInfo } from "./builtins/input";
import { mulberry32 } from "./builtins/math";
import { computeTaSeries, type TaSeriesName } from "./builtins/taVars";
import { barValue, type HostInfo, type Rt, type SourceName } from "./builtins/types";
import { BREAK, CONTINUE, Evaluator, Instance, type Limits } from "./evaluator";
import { arith, type Value, type Version } from "./values";

export interface ExecOptions {
  bars: Bar[];
  inputs: Value[];
  host: HostInfo;
  limits: Limits;
  seed: number;
}

export interface ExecResult {
  outputs: Output[];
  nodes: number;
}

export class Interpreter extends Evaluator implements Rt {
  readonly version: Version;
  readonly bars: Bar[];
  readonly host: HostInfo;
  bar = 0;
  protected readonly limits: Limits;
  protected readonly global: Instance;
  private readonly inputs: Value[];
  private readonly inputInfo: InputInfo[];
  private readonly outputs: Output[] = [];
  private readonly taCache = new Map<TaSeriesName, (number | null)[]>();
  private readonly rng: () => number;
  private curInst: Instance;
  private curSite = -1;
  private curNode: { line: number; col: number } = { line: 1, col: 1 };

  constructor(private readonly script: CheckedScript, opts: ExecOptions) {
    super();
    this.version = script.meta.version;
    this.bars = opts.bars;
    this.host = opts.host;
    this.limits = opts.limits;
    this.inputs = opts.inputs;
    this.inputInfo = script.inputs;
    this.global = new Instance(script.globalDepths);
    this.curInst = this.global;
    this.rng = mulberry32(opts.seed);
  }

  run(): ExecResult {
    this.deadline = Date.now() + this.limits.maxWallMs;
    const body = this.script.program.body;
    for (let i = 0; i < this.bars.length; i++) {
      this.bar = i;
      this.nodes = 0;
      this.execBlock(body, this.global);
      this.totalNodes += this.nodes;
      if (Date.now() > this.deadline) fail("limit", `Run stopped: wall time limit of ${this.limits.maxWallMs} ms exceeded`, { line: 1, col: 1 });
    }
    return { outputs: this.outputs, nodes: this.totalNodes };
  }

  // ---- Rt ----
  protected asRt(): Rt {
    return this;
  }

  state<T>(make: () => T): T {
    let s = this.curInst.sites.get(this.curSite);
    if (s === undefined) {
      s = make();
      this.curInst.sites.set(this.curSite, s);
    }
    return s as T;
  }

  fail(message: string): never {
    return fail("runtime", message, this.curNode);
  }

  random(): number {
    return this.rng();
  }

  series(name: TaSeriesName): (number | null)[] {
    let s = this.taCache.get(name);
    if (!s) this.taCache.set(name, (s = computeTaSeries(name, this.bars)));
    return s;
  }

  addOutput(o: Output): number {
    o.id = this.outputs.length;
    o.line = this.curNode.line;
    this.outputs.push(o);
    return o.id;
  }

  output(id: number): Output {
    return this.outputs[id];
  }

  // ---- statements ----
  protected execBlock(body: Stmt[], inst: Instance): Value {
    let last: Value = null;
    for (let k = 0; k < body.length; k++) {
      last = this.exec(body[k], inst);
      if (this.ctl !== 0) break;
    }
    return last;
  }

  private exec(s: Stmt, inst: Instance): Value {
    switch (s.kind) {
      case "VarDecl": {
        const slot = inst.slots[s.slot as number];
        if (s.mode === "var" && slot.init) {
          slot.declare(slot.cur, this.bar);
          return slot.cur;
        }
        let v = this.evaluate(s.init, inst);
        if (v === null && s.type === "bool" && this.version === 6) v = false;
        slot.declare(v, this.bar);
        return v;
      }
      case "TupleDecl": {
        const v = this.evaluate(s.init, inst);
        if (!Array.isArray(v) || v.length !== s.names.length)
          fail("runtime", `Expected ${s.names.length} values from the right-hand side, got ${Array.isArray(v) ? v.length : 1}`, s);
        (s.slots as number[]).forEach((i, k) => inst.slots[i].declare(v[k], this.bar));
        return null;
      }
      case "Assign": {
        const ref = s.ref as { g: boolean; i: number };
        const slot = (ref.g ? this.global : inst).slots[ref.i];
        const v = this.evaluate(s.value, inst);
        slot.cur = s.op === ":=" ? v : arith(s.op[0], slot.cur, v);
        return slot.cur;
      }
      case "ExprStmt":
        return this.evaluate(s.expr, inst);
      case "FuncDef":
        return null;
      case "Break":
        this.ctl = BREAK;
        return null;
      case "Continue":
        this.ctl = CONTINUE;
        return null;
    }
  }

  // ---- calls ----
  protected call(e: Call, inst: Instance): Value {
    if (e.decl) return null;
    if (e.input !== undefined) return this.inputValue(e.input);
    const bound = e.bound as (Expr | null)[];
    if (e.user !== undefined) return this.callUser(e, bound, inst);
    const def = e.def;
    if (!def) return fail("runtime", `Unresolved call '${e.callee}'`, e);
    const args: Value[] = new Array(bound.length);
    for (let i = 0; i < bound.length; i++) {
      const b = bound[i];
      if (b !== null) args[i] = this.evaluate(b, inst);
      else {
        const p = def.params[i];
        args[i] = p.src ? barValue(this.bars, this.bar, p.src) : p.def ?? null;
      }
    }
    this.curInst = inst;
    this.curSite = e.site;
    this.curNode = e;
    return def.impl(this, args);
  }

  private callUser(e: Call, bound: (Expr | null)[], inst: Instance): Value {
    const fn = this.script.funcs[e.user as number];
    const args = bound.map((b, i) => {
      if (b !== null) return this.evaluate(b, inst);
      const d = fn.def.params[i].def;
      return d ? this.evaluate(d, inst) : null;
    });
    let child = inst.sites.get(e.site) as Instance | undefined;
    if (!child) {
      child = new Instance(fn.depths);
      inst.sites.set(e.site, child);
    }
    for (let i = 0; i < args.length; i++) child.slots[i].declare(args[i], this.bar);
    const v = this.execBlock(fn.def.body, child);
    this.ctl = 0;
    return v;
  }

  private inputValue(i: number): Value {
    const v = this.inputs[i];
    return this.inputInfo[i].kind === "source" ? barValue(this.bars, this.bar, v as SourceName) : v;
  }
}
