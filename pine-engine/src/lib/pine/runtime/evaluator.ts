/**
 * Expression evaluation (tree walking over the checked AST). Statement
 * execution, calls and the Rt services live in interpreter.ts, which extends
 * this class.
 */
import type { ColorLit, Expr, Stmt } from "../ast";
import { fail } from "../errors";
import type { Rt } from "./builtins/types";
import { DEFAULT_HISTORY, Slot } from "./series";
import { arith, compare, negate, not, num, parseHexColor, truthy, type Color, type Value, type Version } from "./values";

export interface Limits {
  /** Bars per run (the chart's loaded bars). */
  maxBars: number;
  /** Wall-clock budget for one run, ms. */
  maxWallMs: number;
  /** Evaluated AST nodes per bar, loops included. */
  maxNodesPerBar: number;
  /** Iterations per loop execution. */
  maxLoopIterations: number;
  /** plot-family calls per script (checked at compile time). */
  maxPlots: number;
  /** Source size in bytes (UTF-16 code units, checked at compile time). */
  maxSourceBytes: number;
}

export const DEFAULT_LIMITS: Limits = {
  maxBars: 5000,
  maxWallMs: 2000,
  maxNodesPerBar: 1_000_000,
  maxLoopIterations: 10_000,
  maxPlots: 64,
  maxSourceBytes: 500_000,
};

/** One activation context: the global script or one (call site, parent) function instance. */
export class Instance {
  readonly slots: Slot[];
  /** Built-in states and child function instances, keyed by call-site id. */
  readonly sites = new Map<number, unknown>();
  constructor(depths: number[]) {
    this.slots = depths.map((d) => new Slot(Math.min(Math.max(d, 0), DEFAULT_HISTORY)));
  }
}

export const BREAK = 1;
export const CONTINUE = 2;

export abstract class Evaluator {
  abstract readonly version: Version;
  protected abstract readonly limits: Limits;
  protected abstract readonly global: Instance;
  abstract bar: number;
  protected nodes = 0;
  protected totalNodes = 0;
  protected deadline = Infinity;
  protected ctl = 0;
  private colors = new Map<ColorLit, Color>();

  protected abstract call(e: Extract<Expr, { kind: "Call" }>, inst: Instance): Value;
  protected abstract execBlock(body: Stmt[], inst: Instance): Value;
  protected abstract asRt(): Rt;

  protected tick(at: Expr): void {
    if (++this.nodes > this.limits.maxNodesPerBar)
      fail("limit", `Run stopped: more than ${this.limits.maxNodesPerBar.toLocaleString("en-US")} evaluation steps on bar ${this.bar}`, at, "Reduce loop sizes or nesting");
    if ((this.nodes & 1023) === 0 && Date.now() > this.deadline) fail("limit", `Run stopped: wall time limit of ${this.limits.maxWallMs} ms exceeded`, at);
  }

  protected evaluate(e: Expr, inst: Instance): Value {
    this.tick(e);
    switch (e.kind) {
      case "Num":
      case "Str":
      case "Bool":
        return e.value;
      case "Na":
        return null;
      case "Color": {
        let c = this.colors.get(e);
        if (!c) this.colors.set(e, (c = parseHexColor(e.hex)));
        return c;
      }
      case "Ident": {
        const r = e.ref;
        if (!r) return fail("runtime", `Unresolved identifier '${e.name}'`, e);
        if (r.k === "slot") return (r.g ? this.global : inst).slots[r.i].cur;
        if (r.k === "const") return r.v;
        return r.v.get(this.asRt(), 0);
      }
      case "Call":
        return this.call(e, inst);
      case "Index":
        return this.index(e, inst);
      case "Unary": {
        const v = this.evaluate(e.operand, inst);
        return e.op === "-" ? negate(v) : e.op === "+" ? num(v) : not(v, this.version);
      }
      case "Binary":
        return this.binary(e, inst);
      case "Ternary":
        return truthy(this.evaluate(e.cond, inst)) ? this.evaluate(e.a, inst) : this.evaluate(e.b, inst);
      case "Tuple":
        return e.items.map((x) => this.evaluate(x, inst));
      case "If":
        if (truthy(this.evaluate(e.cond, inst))) return this.execBlock(e.then, inst);
        return e.else ? this.execBlock(e.else, inst) : null;
      case "Switch":
        return this.switchExpr(e, inst);
      case "For":
        return this.forExpr(e, inst);
      case "While":
        return this.whileExpr(e, inst);
    }
  }

  private index(e: Extract<Expr, { kind: "Index" }>, inst: Instance): Value {
    const o = num(this.evaluate(e.offset, inst));
    const t = e.target;
    let value: Value = null;
    if (e.hidden !== undefined) value = this.evaluate(t, inst);
    if (o === null) return null;
    const n = Math.trunc(o);
    if (n < 0) fail("runtime", `History offset must not be negative (got ${n})`, e.offset);
    if (t.kind === "Ident" && t.ref?.k === "slot") return (t.ref.g ? this.global : inst).slots[t.ref.i].read(n);
    if (t.kind === "Ident" && t.ref?.k === "bv") return t.ref.v.get(this.asRt(), n);
    const slot = inst.slots[e.hidden as number];
    slot.declare(value, this.bar);
    return slot.read(n);
  }

  private binary(e: Extract<Expr, { kind: "Binary" }>, inst: Instance): Value {
    if (e.op === "and" || e.op === "or") {
      const a = truthy(this.evaluate(e.left, inst));
      if (this.version === 6) {
        if (e.op === "and" ? !a : a) return a;
        return truthy(this.evaluate(e.right, inst));
      }
      const b = truthy(this.evaluate(e.right, inst));
      return e.op === "and" ? a && b : a || b;
    }
    const a = this.evaluate(e.left, inst);
    const b = this.evaluate(e.right, inst);
    if (e.op === "==" || e.op === "!=" || e.op === "<" || e.op === ">" || e.op === "<=" || e.op === ">=") return compare(e.op, a, b, this.version);
    return arith(e.op, a, b, e.intDiv === true);
  }

  private switchExpr(e: Extract<Expr, { kind: "Switch" }>, inst: Instance): Value {
    const subject = e.subject ? this.evaluate(e.subject, inst) : null;
    for (const c of e.cases) {
      const hit = c.match === null || (e.subject ? compare("==", subject, this.evaluate(c.match, inst), 6) === true : truthy(this.evaluate(c.match, inst)));
      if (hit) return this.execBlock(c.body, inst);
    }
    return null;
  }

  private loopGuard(count: number, at: Expr): void {
    if (count > this.limits.maxLoopIterations)
      fail("limit", `Run stopped: a loop ran more than ${this.limits.maxLoopIterations.toLocaleString("en-US")} iterations on bar ${this.bar}`, at);
  }

  private forExpr(e: Extract<Expr, { kind: "For" }>, inst: Instance): Value {
    const from = num(this.evaluate(e.from, inst));
    const to = num(this.evaluate(e.to, inst));
    const rawStep = e.step ? num(this.evaluate(e.step, inst)) : 1;
    if (from === null || to === null || rawStep === null) return null;
    if (rawStep === 0) fail("runtime", "for loop step must not be 0", e.step ?? e);
    const step = from <= to ? Math.abs(rawStep) : -Math.abs(rawStep);
    const slot = inst.slots[e.slot as number];
    let last: Value = null;
    let count = 0;
    for (let i = from; step > 0 ? i <= to : i >= to; i += step) {
      this.loopGuard(++count, e);
      slot.declare(i, this.bar);
      last = this.execBlock(e.body, inst);
      if (this.ctl === BREAK) {
        this.ctl = 0;
        break;
      }
      this.ctl = 0;
    }
    return last;
  }

  private whileExpr(e: Extract<Expr, { kind: "While" }>, inst: Instance): Value {
    let last: Value = null;
    let count = 0;
    while (truthy(this.evaluate(e.cond, inst))) {
      this.loopGuard(++count, e);
      last = this.execBlock(e.body, inst);
      if (this.ctl === BREAK) {
        this.ctl = 0;
        break;
      }
      this.ctl = 0;
    }
    return last;
  }
}
