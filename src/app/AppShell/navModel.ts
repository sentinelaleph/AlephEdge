import type { ParsedRoute, RouteId } from "@/app/router/routes";
import type { IconName } from "./icons";

export type NavItemId =
  | "dashboard"
  | "bots"
  | "signalBots"
  | "dcaBots"
  | "gridBots"
  | "signals"
  | "presets"
  | "backtest"
  | "guide"
  | "faq"
  | "positions"
  | "history"
  | "risk"
  | "settings"
  | "account"
  | "botNew";

export interface NavItem {
  id: NavItemId;
  path: string;
  labelKey: string;
  icon: IconName;
}

export type NavGroupId = "overview" | "bots" | "market" | "research" | "portfolio" | "control";

export interface NavGroup {
  id: NavGroupId;
  labelKey: string;
  collapsible: boolean;
  items: NavItem[];
}

export const NAV_GROUPS: NavGroup[] = [
  {
    id: "overview",
    labelKey: "nav.groups.overview",
    collapsible: false,
    items: [{ id: "dashboard", path: "/dashboard", labelKey: "nav.dashboard", icon: "dashboard" }],
  },
  {
    id: "bots",
    labelKey: "nav.groups.bots",
    collapsible: true,
    items: [
      { id: "bots", path: "/bots", labelKey: "nav.allBots", icon: "bots" },
      { id: "signalBots", path: "/bots/signal", labelKey: "nav.signalBots", icon: "signal" },
      { id: "dcaBots", path: "/bots/dca", labelKey: "nav.dcaBots", icon: "dca" },
      { id: "gridBots", path: "/bots/grid", labelKey: "nav.gridBots", icon: "grid" },
    ],
  },
  {
    id: "market",
    labelKey: "nav.groups.market",
    collapsible: false,
    items: [{ id: "signals", path: "/signals", labelKey: "nav.signals", icon: "signals" }],
  },
  {
    id: "research",
    labelKey: "nav.groups.research",
    collapsible: true,
    items: [
      { id: "presets", path: "/presets", labelKey: "nav.presets", icon: "presets" },
      { id: "backtest", path: "/backtest", labelKey: "nav.backtest", icon: "backtest" },
      { id: "guide", path: "/guide", labelKey: "nav.guide", icon: "guide" },
      { id: "faq", path: "/faq", labelKey: "nav.faq", icon: "help" },
    ],
  },
  {
    id: "portfolio",
    labelKey: "nav.groups.portfolio",
    collapsible: true,
    items: [
      { id: "positions", path: "/positions", labelKey: "nav.positions", icon: "positions" },
      { id: "history", path: "/history", labelKey: "nav.history", icon: "history" },
    ],
  },
  {
    id: "control",
    labelKey: "nav.groups.control",
    collapsible: false,
    items: [
      { id: "risk", path: "/risk", labelKey: "nav.risk", icon: "risk" },
    ],
  },
];

const DIRECT: Partial<Record<RouteId, NavItemId>> = {
  dashboard: "dashboard",
  bots: "bots",
  botNew: "botNew",
  signalBots: "signalBots",
  dcaBots: "dcaBots",
  dcaNew: "dcaBots",
  gridBots: "gridBots",
  gridNew: "gridBots",
  signals: "signals",
  presets: "presets",
  presetDetail: "presets",
  backtest: "backtest",
  backtestReport: "backtest",
  guide: "guide",
  faq: "faq",
  positions: "positions",
  history: "history",
  // The vault moved into Settings (2026-10-03); the old address lands there.
  accounts: "settings",
  risk: "risk",
  settings: "settings",
  account: "account",
};

/**
 * Which nav item a route highlights. A bot detail highlights its type's list,
 * or "All bots" when the user came from there.
 */
export function activeNavItem(
  route: ParsedRoute,
  from: string | null,
  strategyKindOf: (botId: string) => "dca" | "grid" | null = () => null,
): NavItemId | null {
  if (route.id === "botDetail") {
    if (from === "/bots") return "bots";
    const id = route.params.botId ?? "";
    if (id.startsWith("signal-")) return "signalBots";
    // Strategy ids ("sb_...") carry no kind: the loaded bot list does.
    const kind = strategyKindOf(id);
    if (kind === "dca") return "dcaBots";
    if (kind === "grid") return "gridBots";
    return "bots";
  }
  return DIRECT[route.id] ?? null;
}

export function groupOf(item: NavItemId | null): NavGroupId | null {
  if (!item) return null;
  return NAV_GROUPS.find((g) => g.items.some((i) => i.id === item))?.id ?? null;
}

/** Ctrl+1..9 targets, in order. */
export const NUMBER_SHORTCUTS = [
  "/dashboard",
  "/bots",
  "/bots/signal",
  "/bots/dca",
  "/bots/grid",
  "/signals",
  "/positions",
  "/history",
  "/risk",
];
