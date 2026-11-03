/**
 * math.* — na in, na out; results that are not finite (sqrt(-1), log(0)) are
 * na. math.round rounds ties up (toward +infinity), as the reference states.
 * math.random is deterministic: without a seed it draws from the run's
 * generator (seeded by RunOptions.seed, default 1); with a seed argument each
 * call site owns a generator seeded with it.
 */
import { finite, num, type Value } from "../values";
import { len, Sum } from "./taCore";
import type { BuiltinFn, Param } from "./types";

const x: Param = { name: "number", req: true };

function m1(name: string, f: (v: number) => number, returns: "float" | "int" = "float"): BuiltinFn {
  return {
    name: `math.${name}`,
    params: [x],
    returns,
    impl: (_rt, a) => {
      const v = num(a[0]);
      return v === null ? null : finite(f(v));
    },
  };
}

function variadic(name: string, f: (vals: number[]) => number): BuiltinFn {
  return {
    name: `math.${name}`,
    params: [{ name: "number0", req: true }],
    variadic: true,
    returns: "float",
    impl: (_rt, a) => {
      const vals: number[] = [];
      for (const v of a) {
        const k = num(v);
        if (k === null) return null;
        vals.push(k);
      }
      return finite(f(vals));
    },
  };
}

/** mulberry32: small, fast, well-distributed 32-bit PRNG. */
export function mulberry32(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const roundTo = (v: number, p: number) => {
  const k = Math.pow(10, p);
  return Math.round(v * k) / k;
};

export const MATH: BuiltinFn[] = [
  m1("abs", Math.abs),
  m1("ceil", Math.ceil, "int"),
  m1("floor", Math.floor, "int"),
  {
    name: "math.round",
    params: [x, { name: "precision" }],
    returns: "float",
    impl: (_rt, a: Value[]) => {
      const v = num(a[0]);
      if (v === null) return null;
      const p = num(a[1]);
      return finite(p === null ? Math.round(v) : roundTo(v, Math.trunc(p)));
    },
  },
  m1("sign", Math.sign),
  m1("sqrt", Math.sqrt),
  {
    name: "math.pow",
    params: [{ name: "base", req: true }, { name: "exponent", req: true }],
    returns: "float",
    impl: (_rt, a) => {
      const b = num(a[0]);
      const e = num(a[1]);
      return b === null || e === null ? null : finite(Math.pow(b, e));
    },
  },
  m1("exp", Math.exp),
  m1("log", Math.log),
  m1("log10", Math.log10),
  m1("sin", Math.sin),
  m1("cos", Math.cos),
  m1("tan", Math.tan),
  m1("asin", Math.asin),
  m1("acos", Math.acos),
  m1("atan", Math.atan),
  m1("todegrees", (v) => (v * 180) / Math.PI),
  m1("toradians", (v) => (v * Math.PI) / 180),
  variadic("max", (v) => Math.max(...v)),
  variadic("min", (v) => Math.min(...v)),
  variadic("avg", (v) => v.reduce((s, k) => s + k, 0) / v.length),
  {
    name: "math.sum",
    params: [{ name: "source", req: true }, { name: "length", req: true }],
    stateful: true,
    returns: "float",
    impl: (rt, a) => {
      const st = rt.state(() => new Sum());
      const l = len(rt, a[1], "math.sum");
      return l === null ? null : st.step(num(a[0]), l, rt.bar);
    },
  },
  {
    name: "math.round_to_mintick",
    params: [x],
    returns: "float",
    impl: (rt, a) => {
      const v = num(a[0]);
      const t = rt.host.mintick;
      return v === null || !(t > 0) ? null : finite(Math.round(v / t) * t);
    },
  },
  {
    name: "math.random",
    params: [{ name: "min", def: 0 }, { name: "max", def: 1 }, { name: "seed" }],
    stateful: true,
    returns: "float",
    impl: (rt, a) => {
      const lo = num(a[0]);
      const hi = num(a[1]);
      const seed = num(a[2]);
      if (lo === null || hi === null) return null;
      const r = seed === null ? rt.random() : rt.state(() => mulberry32(seed))();
      return lo + r * (hi - lo);
    },
  },
];

export const MATH_CONSTANTS: Record<string, number> = {
  "math.pi": Math.PI,
  "math.e": Math.E,
  "math.phi": (1 + Math.sqrt(5)) / 2,
  "math.rphi": (Math.sqrt(5) - 1) / 2,
};
