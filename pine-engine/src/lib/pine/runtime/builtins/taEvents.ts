/**
 * Event and bookkeeping functions: cum, crossover, crossunder, cross, rising,
 * falling, barssince, valuewhen, pivothigh, pivotlow.
 *
 * Pivots: the candidate is source[rightbars]; it must be strictly greater
 * (pivothigh) / smaller (pivotlow) than each of the `leftbars` values before
 * it and greater-or-equal / smaller-or-equal than each of the `rightbars`
 * values after it, so with equal highs the first one is the pivot. Returned on
 * the confirming bar (rightbars later), na otherwise.
 */
import { Hist, Rec } from "../series";
import { truthy } from "../values";
import { ge, gt, le, len, lt, n, P, ta, type N } from "./taCore";
import type { BuiltinFn, Rt } from "./types";

class Pair {
  a = new Hist();
  b = new Hist();
}

function crossing(rt: Rt, x: N, y: N): { over: boolean; under: boolean } {
  const st = rt.state(() => new Pair());
  st.a.push(x, rt.bar);
  st.b.push(y, rt.bar);
  const x1 = st.a.at(1);
  const y1 = st.b.at(1);
  return { over: gt(x, y) && le(x1, y1), under: lt(x, y) && ge(x1, y1) };
}

function monotone(name: string, up: boolean): BuiltinFn {
  return ta(
    name,
    [P("source"), P("length")],
    (rt, a) => {
      const h = rt.state(() => new Hist(true));
      h.push(n(a[0]), rt.bar);
      const l = len(rt, a[1], `ta.${name}`);
      if (l === null) return false;
      const w = h.raw(l + 1);
      if (w === null) return false;
      const x = w[0];
      for (let i = 1; i <= l; i++) if (!(up ? gt(x, w[i]) : lt(x, w[i]))) return false;
      return true;
    },
    { returns: "bool" },
  );
}

function pivot(name: string, high: boolean): BuiltinFn {
  return ta(
    name,
    [P("source", { req: false, src: high ? "high" : "low" }), P("leftbars"), P("rightbars")],
    (rt, a) => {
      const h = rt.state(() => new Hist());
      h.push(n(a[0]), rt.bar);
      const left = len(rt, a[1], `ta.${name}`, "leftbars", 0);
      const right = len(rt, a[2], `ta.${name}`, "rightbars", 0);
      if (left === null || right === null) return null;
      const w = h.raw(left + right + 1);
      if (w === null) return null;
      const c = w[right];
      if (c === null) return null;
      for (let i = right + 1; i <= right + left; i++) if (!(high ? lt(w[i], c) : gt(w[i], c))) return null;
      for (let i = 0; i < right; i++) if (!(high ? le(w[i], c) : ge(w[i], c))) return null;
      return c;
    },
    { sigs: [["leftbars", "rightbars"], ["source", "leftbars", "rightbars"]] },
  );
}

class ValueWhen {
  vals: N[] = [];
  ep = -1;
  pushedAt = -1;
}

export const EVENTS: BuiltinFn[] = [
  // Running total; na values add nothing.
  ta("cum", [P("source")], (rt, a) => {
    const rec = rt.state(() => new Rec<number>(0));
    const x = n(a[0]);
    return rec.step(rt.bar, (prev) => prev + (x ?? 0));
  }),
  ta("crossover", [P("source1"), P("source2")], (rt, a) => crossing(rt, n(a[0]), n(a[1])).over, { returns: "bool" }),
  ta("crossunder", [P("source1"), P("source2")], (rt, a) => crossing(rt, n(a[0]), n(a[1])).under, { returns: "bool" }),
  ta(
    "cross",
    [P("source1"), P("source2")],
    (rt, a) => {
      const c = crossing(rt, n(a[0]), n(a[1]));
      return c.over || c.under;
    },
    { returns: "bool" },
  ),
  monotone("rising", true),
  monotone("falling", false),
  ta(
    "barssince",
    [P("condition")],
    (rt, a) => {
      const rec = rt.state(() => new Rec<N>(null));
      const c = truthy(a[0]);
      return rec.step(rt.bar, (prev) => (c ? 0 : prev === null ? null : prev + 1));
    },
    { returns: "int" },
  ),
  ta("valuewhen", [P("condition"), P("source"), P("occurrence")], (rt, a) => {
    const st = rt.state(() => new ValueWhen());
    if (st.ep === rt.bar && st.pushedAt === rt.bar) {
      st.vals.pop();
      st.pushedAt = -1;
    }
    st.ep = rt.bar;
    if (truthy(a[0])) {
      st.vals.push(n(a[1]));
      st.pushedAt = rt.bar;
      if (st.vals.length > 10000) st.vals.splice(0, 5000);
    }
    const occ = len(rt, a[2], "ta.valuewhen", "occurrence", 0);
    if (occ === null) return null;
    const i = st.vals.length - 1 - occ;
    return i >= 0 ? st.vals[i] : null;
  }),
  pivot("pivothigh", true),
  pivot("pivotlow", false),
];
