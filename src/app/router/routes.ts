/**
 * The route table. A hash router (index.html#/bots/dca) because Tauri's asset
 * handler has no SPA fallback for path URLs: a reload of /bots/dca would 404,
 * while index.html#/bots/dca always resolves to the bundle.
 *
 * Order matters: static segments are listed before the `:param` route that
 * would otherwise swallow them (/bots/new before /bots/:botId).
 */

export type RouteId =
  | "dashboard"
  | "bots"
  | "botNew"
  | "signalBots"
  | "dcaBots"
  | "dcaNew"
  | "gridBots"
  | "gridNew"
  | "botDetail"
  | "signals"
  | "presets"
  | "presetDetail"
  | "backtest"
  | "backtestReport"
  | "guide"
  | "faq"
  | "positions"
  | "history"
  | "accounts"
  | "risk"
  | "settings"
  | "account"
  | "notFound";

export interface RouteDef {
  id: RouteId;
  pattern: string;
  /** i18n key of the page title (also the document title). */
  titleKey: string;
}

export const ROUTES: RouteDef[] = [
  { id: "dashboard", pattern: "/dashboard", titleKey: "nav.dashboard" },
  { id: "botNew", pattern: "/bots/new", titleKey: "page.botNew" },
  { id: "signalBots", pattern: "/bots/signal", titleKey: "nav.signalBots" },
  { id: "dcaNew", pattern: "/bots/dca/new", titleKey: "page.dcaNew" },
  { id: "dcaBots", pattern: "/bots/dca", titleKey: "nav.dcaBots" },
  { id: "gridNew", pattern: "/bots/grid/new", titleKey: "page.gridNew" },
  { id: "gridBots", pattern: "/bots/grid", titleKey: "nav.gridBots" },
  { id: "botDetail", pattern: "/bots/:botId", titleKey: "page.botDetail" },
  { id: "bots", pattern: "/bots", titleKey: "nav.allBots" },
  { id: "signals", pattern: "/signals", titleKey: "nav.signals" },
  { id: "presetDetail", pattern: "/presets/:presetId", titleKey: "page.presetDetail" },
  { id: "presets", pattern: "/presets", titleKey: "nav.presets" },
  { id: "backtestReport", pattern: "/backtest/:runId", titleKey: "page.backtestReport" },
  { id: "backtest", pattern: "/backtest", titleKey: "nav.backtest" },
  { id: "guide", pattern: "/guide", titleKey: "nav.guide" },
  { id: "faq", pattern: "/faq", titleKey: "nav.faq" },
  { id: "positions", pattern: "/positions", titleKey: "nav.positions" },
  { id: "history", pattern: "/history", titleKey: "nav.history" },
  { id: "accounts", pattern: "/accounts", titleKey: "nav.accounts" },
  { id: "risk", pattern: "/risk", titleKey: "nav.risk" },
  { id: "settings", pattern: "/settings", titleKey: "nav.settings" },
  { id: "account", pattern: "/account", titleKey: "nav.account" },
  { id: "notFound", pattern: "/not-found", titleKey: "page.notFound" },
];

export const DEFAULT_PATH = "/dashboard";

/** Settings, exchange keys tab: the vault. */
export const KEYS_PATH = "/settings?tab=keys";

/** Legacy signal-bot kinds expressed as bot ids until they become instances. */
export const SIGNAL_BOT_IDS = {
  futures: "signal-futures",
  spot: "signal-spot",
  pump: "signal-pump",
} as const;

export interface ParsedRoute {
  id: RouteId;
  /** The path part of the hash, e.g. "/bots/dca". */
  path: string;
  params: Record<string, string>;
  query: URLSearchParams;
}

function matchPattern(pattern: string, path: string): Record<string, string> | null {
  const p = pattern.split("/").filter(Boolean);
  const s = path.split("/").filter(Boolean);
  if (p.length !== s.length) return null;
  const params: Record<string, string> = {};
  for (let i = 0; i < p.length; i += 1) {
    if (p[i].startsWith(":")) {
      try {
        params[p[i].slice(1)] = decodeURIComponent(s[i]);
      } catch {
        return null;
      }
    } else if (p[i] !== s[i]) {
      return null;
    }
  }
  return params;
}

/** Splits "#/bots/dca?preset=x" into its route, params and query. */
export function parseHash(hash: string): ParsedRoute {
  const raw = hash.replace(/^#/, "");
  const q = raw.indexOf("?");
  const path = (q >= 0 ? raw.slice(0, q) : raw) || "/";
  const query = new URLSearchParams(q >= 0 ? raw.slice(q + 1) : "");
  const clean = path.length > 1 ? path.replace(/\/+$/, "") : path;
  for (const def of ROUTES) {
    const params = matchPattern(def.pattern, clean);
    if (params) return { id: def.id, path: clean, params, query };
  }
  return { id: "notFound", path: clean, params: {}, query };
}

export function routeDef(id: RouteId): RouteDef {
  return ROUTES.find((r) => r.id === id) ?? ROUTES[ROUTES.length - 1];
}

/** Builds "/path?k=v", dropping empty values. */
export function withQuery(path: string, query: Record<string, string | null | undefined>): string {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(query)) {
    if (v !== null && v !== undefined && v !== "") q.set(k, v);
  }
  const s = q.toString();
  return s ? `${path}?${s}` : path;
}
