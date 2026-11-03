/**
 * Compile-time evaluation for arguments Pine requires to be constant:
 * indicator() settings, input defaults/limits/options, plot titles and
 * styles. Handles literals, named constants, never-reassigned constant
 * variables, unary/binary operators, ternaries, tuples of constants (input
 * options) and color.new / color.rgb on constants.
 */
import type { Expr } from "./ast";
import { CONSTANT_VALUES, FUNCTIONS } from "./registry";
import type { Rt } from "./runtime/builtins/types";
import { arith, compare, negate, not, parseHexColor, truthy, type Value, type Version } from "./runtime/values";

export const NOT_CONST = Symbol("not-const");
export type ConstResult = Value | typeof NOT_CONST;

const PURE_CALLS = new Set(["color.new", "color.rgb"]);

export function evalConst(e: Expr, version: Version, lookup: (name: string) => Value | undefined): ConstResult {
  const rec = (x: Expr): ConstResult => evalConst(x, version, lookup);
  switch (e.kind) {
    case "Num":
      return e.value;
    case "Str":
      return e.value;
    case "Bool":
      return e.value;
    case "Na":
      return null;
    case "Color":
      return parseHexColor(e.hex);
    case "Ident": {
      const local = lookup(e.name);
      if (local !== undefined) return local;
      return e.name in CONSTANT_VALUES ? CONSTANT_VALUES[e.name] : NOT_CONST;
    }
    case "Unary": {
      const v = rec(e.operand);
      if (v === NOT_CONST) return v;
      return e.op === "-" ? negate(v) : e.op === "+" ? v : not(v, version);
    }
    case "Binary": {
      const a = rec(e.left);
      const b = rec(e.right);
      if (a === NOT_CONST || b === NOT_CONST) return NOT_CONST;
      if (e.op === "and") return truthy(a) && truthy(b);
      if (e.op === "or") return truthy(a) || truthy(b);
      if (["==", "!=", "<", ">", "<=", ">="].includes(e.op)) return compare(e.op, a, b, version);
      return arith(e.op, a, b, e.intDiv === true);
    }
    case "Ternary": {
      const c = rec(e.cond);
      if (c === NOT_CONST) return c;
      return truthy(c) ? rec(e.a) : rec(e.b);
    }
    case "Tuple": {
      const items: Value[] = [];
      for (const it of e.items) {
        const v = rec(it);
        if (v === NOT_CONST) return v;
        items.push(v);
      }
      return items;
    }
    case "Call": {
      if (!PURE_CALLS.has(e.callee) || e.named.length > 0) return NOT_CONST;
      const def = FUNCTIONS.get(e.callee);
      if (!def) return NOT_CONST;
      const args: Value[] = [];
      for (let i = 0; i < def.params.length; i++) {
        const a = e.args[i];
        if (a === undefined) {
          args.push(def.params[i].def ?? null);
          continue;
        }
        const v = rec(a);
        if (v === NOT_CONST) return v;
        args.push(v);
      }
      return def.impl(null as unknown as Rt, args);
    }
    default:
      return NOT_CONST;
  }
}
