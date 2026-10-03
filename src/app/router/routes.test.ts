import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en/en.json";
import { DEFAULT_PATH, parseHash, routeDef, ROUTES, withQuery, type RouteId } from "./routes";

/** A concrete path for a pattern: ":param" becomes "x-<param>". */
const concrete = (pattern: string) =>
  pattern
    .split("/")
    .map((s) => (s.startsWith(":") ? `x-${s.slice(1)}` : s))
    .join("/");

function lookup(key: string): unknown {
  return key.split(".").reduce<unknown>((o, k) => (o && typeof o === "object" ? (o as Record<string, unknown>)[k] : undefined), en);
}

describe("ROUTES", () => {
  it.each(ROUTES.map((r) => [r.id, r.pattern] as const))("%s matches its own pattern %s", (id, pattern) => {
    const path = concrete(pattern);
    const r = parseHash("#" + path);
    expect(r.id).toBe(id);
    expect(r.path).toBe(path);
  });

  it("ids and patterns are unique", () => {
    expect(new Set(ROUTES.map((r) => r.id)).size).toBe(ROUTES.length);
    expect(new Set(ROUTES.map((r) => r.pattern)).size).toBe(ROUTES.length);
  });

  it("every title key exists in en.json", () => {
    for (const r of ROUTES) expect(typeof lookup(r.titleKey), r.titleKey).toBe("string");
  });

  it("static segments win over :param routes", () => {
    const cases: [string, RouteId][] = [
      ["/bots/new", "botNew"],
      ["/bots/signal", "signalBots"],
      ["/bots/dca", "dcaBots"],
      ["/bots/dca/new", "dcaNew"],
      ["/bots/grid", "gridBots"],
      ["/bots/grid/new", "gridNew"],
      ["/bots/abc123", "botDetail"],
      ["/bots", "bots"],
    ];
    for (const [path, id] of cases) expect(parseHash("#" + path).id, path).toBe(id);
  });

  it("DEFAULT_PATH is the dashboard route", () => {
    expect(parseHash("#" + DEFAULT_PATH).id).toBe("dashboard");
  });
});

describe("parseHash", () => {
  it("extracts and decodes params", () => {
    expect(parseHash("#/bots/signal-futures").params).toEqual({ botId: "signal-futures" });
    expect(parseHash("#/presets/my%20preset").params).toEqual({ presetId: "my preset" });
    expect(parseHash("#/backtest/run%2F7").params).toEqual({ runId: "run/7" });
    expect(parseHash("#/bots/dca").params).toEqual({});
  });

  it("a malformed escape is not found, not a throw", () => {
    expect(parseHash("#/bots/%E0%A4%A").id).toBe("notFound");
  });

  it("splits the query off the path", () => {
    const r = parseHash("#/bots/dca/new?preset=p1&symbol=BTCUSDT");
    expect(r.id).toBe("dcaNew");
    expect(r.path).toBe("/bots/dca/new");
    expect(r.query.get("preset")).toBe("p1");
    expect(r.query.get("symbol")).toBe("BTCUSDT");
    expect(parseHash("#/signals?").query.toString()).toBe("");
    expect(parseHash("#/signals?tab=closed").id).toBe("signals");
  });

  it("drops trailing slashes", () => {
    expect(parseHash("#/bots/dca/").id).toBe("dcaBots");
    expect(parseHash("#/bots/dca//").path).toBe("/bots/dca");
  });

  it("unknown paths are notFound with the path kept", () => {
    const r = parseHash("#/nope/deeper?x=1");
    expect(r).toMatchObject({ id: "notFound", path: "/nope/deeper", params: {} });
    expect(r.query.get("x")).toBe("1");
    expect(parseHash("#/bots/a/b/c").id).toBe("notFound");
    expect(parseHash("#/Dashboard").id).toBe("notFound");
  });

  // The router replaces "", "#" and "#/" with DEFAULT_PATH before rendering
  // (router.tsx ensureInitialRoute / onPop); parseHash alone keeps "/" as "/".
  it("empty and root hashes parse to the root path", () => {
    for (const h of ["", "#", "#/"]) {
      const r = parseHash(h);
      expect(r.path, JSON.stringify(h)).toBe("/");
    }
  });

  it("works without the leading #", () => {
    expect(parseHash("/history").id).toBe("history");
  });
});

describe("routeDef / withQuery", () => {
  it("routeDef finds by id and falls back to notFound", () => {
    expect(routeDef("risk").pattern).toBe("/risk");
    expect(routeDef("nope" as RouteId).id).toBe("notFound");
  });

  it("withQuery drops empty values and encodes", () => {
    expect(withQuery("/bots/dca/new", {})).toBe("/bots/dca/new");
    expect(withQuery("/bots/dca/new", { preset: "a b", x: "", y: null, z: undefined })).toBe("/bots/dca/new?preset=a+b");
    const back = parseHash("#" + withQuery("/signals", { tab: "closed", q: "BTC&ETH" }));
    expect(back.id).toBe("signals");
    expect(back.query.get("q")).toBe("BTC&ETH");
  });
});
