import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import { deriveAlerts, useAlertPrefs, useNow, useVaultAutoLockedAt } from "@/app/alerts";
import { useDeskContext } from "@/app/DeskProvider";
import { back, ensureInitialRoute, forward, navigate, useRoute } from "@/app/router/router";
import { KEYS_PATH, type ParsedRoute } from "@/app/router/routes";
import { AccountPage } from "@/pages/Account/AccountPage";
import { BotNewPage } from "@/pages/Bots/BotNewPage";
import { BotsPage } from "@/pages/Bots/BotsPage";
import { SignalBotsPage } from "@/pages/Bots/SignalBotsPage";
import { StrategyBotsPage } from "@/pages/Bots/StrategyBotsPage";
import { StrategyCreatePage } from "@/pages/Bots/StrategyCreatePage";
import { DashboardPage } from "@/pages/Dashboard/DashboardPage";
import { HistoryPage } from "@/pages/History/HistoryPage";
import { NotFoundPage } from "@/pages/NotFound/NotFoundPage";
import { PositionsPage } from "@/pages/Positions/PositionsPage";
import { RiskPage } from "@/pages/Risk/RiskPage";
import { SettingsPage } from "@/pages/Settings/SettingsPage";
import { SignalsPage } from "@/pages/Signals/SignalsPage";
import { AlertsDrawer } from "./AlertsDrawer";
import { UpdateBanner } from "./UpdateBanner";
import { useUpdateChecks } from "@/app/update";
import { Header } from "./Header";
import { NUMBER_SHORTCUTS } from "./navModel";
import { ShellSlotsContext } from "./shellSlots";
import { Sidebar, type NavMode } from "./Sidebar";
import { StatusBar } from "./StatusBar";
import "./AppShell.css";
import "@/pages/pages.css";

// Heavier, less-visited pages load on first visit to keep the first paint small.
const BotDetailPage = lazy(() => import("@/pages/Bots/BotDetailPage").then((m) => ({ default: m.BotDetailPage })));
const PresetsPage = lazy(() => import("@/pages/Presets/PresetsPage").then((m) => ({ default: m.PresetsPage })));
const PresetDetailPage = lazy(() =>
  import("@/pages/Presets/PresetsPage").then((m) => ({ default: m.PresetDetailPage })),
);
const GuidePage = lazy(() => import("@/pages/Guide/GuidePage").then((m) => ({ default: m.GuidePage })));
const BacktestPage = lazy(() => import("@/pages/Backtest/BacktestPage").then((m) => ({ default: m.BacktestPage })));
const BacktestReportPage = lazy(() =>
  import("@/pages/Backtest/BacktestPage").then((m) => ({ default: m.BacktestReportPage })),
);

const COLLAPSED_KEY = "aleph-edge-nav-collapsed";
/** 860-1279 px: the user pinned the full sidebar open (rail otherwise). */
const PINNED_KEY = "aleph-edge-nav-pinned";

function readFlag(key: string): boolean {
  try {
    return localStorage.getItem(key) === "1";
  } catch {
    return false;
  }
}

function writeFlag(key: string, on: boolean) {
  try {
    localStorage.setItem(key, on ? "1" : "0");
  } catch {
    /* not remembered; still toggles */
  }
}

function useWindowWidth(): number {
  const [w, setW] = useState(() => window.innerWidth);
  useEffect(() => {
    const on = () => setW(window.innerWidth);
    window.addEventListener("resize", on);
    return () => window.removeEventListener("resize", on);
  }, []);
  return w;
}

function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el) return false;
  const tag = el.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || el.isContentEditable;
}

export function renderRoute(route: ParsedRoute): ReactNode {
  switch (route.id) {
    case "dashboard":
      return <DashboardPage />;
    case "bots":
      return <BotsPage />;
    case "botNew":
      return <BotNewPage />;
    case "signalBots":
      return <SignalBotsPage />;
    case "dcaBots":
      return <StrategyBotsPage kind="dca" />;
    case "gridBots":
      return <StrategyBotsPage kind="grid" />;
    case "dcaNew":
      return <StrategyCreatePage kind="dca" />;
    case "gridNew":
      return <StrategyCreatePage kind="grid" />;
    case "botDetail":
      return <BotDetailPage botId={route.params.botId ?? ""} />;
    case "signals":
      return <SignalsPage />;
    case "presets":
      return <PresetsPage />;
    case "presetDetail":
      return <PresetDetailPage presetId={route.params.presetId ?? ""} />;
    case "backtest":
      return <BacktestPage />;
    case "guide":
      return <GuidePage />;
    case "backtestReport":
      return <BacktestReportPage runId={route.params.runId ?? ""} />;
    case "positions":
      return <PositionsPage />;
    case "history":
      return <HistoryPage />;
    case "accounts":
      return <LegacyAccountsRedirect />;
    case "risk":
      return <RiskPage />;
    case "settings":
      return <SettingsPage />;
    case "account":
      return <AccountPage />;
    case "notFound":
      return <NotFoundPage />;
  }
}

/** #/accounts was the vault page until it moved into Settings; old links land on the new tab. */
function LegacyAccountsRedirect() {
  useEffect(() => {
    navigate(KEYS_PATH, { replace: true });
  }, []);
  return null;
}

/**
 * The unlocked application frame: sidebar (page index), header (page heading,
 * actions, global state), the routed page, and the status bar. Mounted inside
 * DeskProvider, below the gates.
 */
export function AppShell() {
  // Land on a real route (or the replayed deep link) before the first render.
  useState(() => {
    ensureInitialRoute();
    return null;
  });
  const { t } = useTranslation();
  const ctx = useDeskContext();
  const route = useRoute();
  useUpdateChecks();
  const width = useWindowWidth();
  const [collapsed, setCollapsed] = useState(() => readFlag(COLLAPSED_KEY));
  const [pinnedOpen, setPinnedOpen] = useState(() => readFlag(PINNED_KEY));
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [alertsOpen, setAlertsOpen] = useState(false);
  const [heading, setHeading] = useState<HTMLDivElement | null>(null);
  const [actions, setActions] = useState<HTMLDivElement | null>(null);

  // A navigation closes the overlay drawers.
  useEffect(() => {
    setDrawerOpen(false);
  }, [route.path]);

  const mode: NavMode =
    width < 860 ? "drawer" : width < 1280 ? (pinnedOpen ? "expanded" : "rail") : collapsed ? "rail" : "expanded";

  const toggleNav = useCallback(() => {
    if (window.innerWidth < 860) {
      setDrawerOpen((v) => !v);
    } else if (window.innerWidth < 1280) {
      setPinnedOpen((v) => {
        writeFlag(PINNED_KEY, !v);
        return !v;
      });
    } else {
      setCollapsed((v) => {
        writeFlag(COLLAPSED_KEY, !v);
        return !v;
      });
    }
  }, []);

  // ---- Alerts ----
  const lockedAt = useVaultAutoLockedAt();
  const now = useNow();
  const { prefs } = useAlertPrefs();
  const alerts = useMemo(
    () => deriveAlerts(ctx, t, lockedAt, now).filter((a) => prefs[a.category]),
    [ctx, t, lockedAt, now, prefs],
  );
  const seen = useRef(new Set<string>());
  const signature = (a: { key: string; count?: number }) => `${a.key}:${a.count ?? 1}`;
  const unread = alerts.filter((a) => !seen.current.has(signature(a))).length;
  const openAlerts = () => {
    alerts.forEach((a) => seen.current.add(signature(a)));
    setAlertsOpen((v) => !v);
  };

  // ---- Keyboard shortcuts ----
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // An open dialog or the alerts drawer owns the keyboard (they handle Esc);
      // a page shortcut must not navigate away underneath them.
      if (document.querySelector(".ae-dialog, .ae-alerts[data-open]")) return;
      const typing = isTyping(e.target);
      if (e.ctrlKey && !e.altKey && !e.shiftKey) {
        const n = Number(e.key);
        if (n >= 1 && n <= 9) {
          e.preventDefault();
          navigate(NUMBER_SHORTCUTS[n - 1]);
          return;
        }
        if (e.key === ",") {
          e.preventDefault();
          navigate("/settings");
          return;
        }
        if (e.key.toLowerCase() === "b") {
          e.preventDefault();
          toggleNav();
          return;
        }
      }
      if (e.altKey && !e.ctrlKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
        e.preventDefault();
        if (e.key === "ArrowLeft") back();
        else forward();
        return;
      }
      if (typing || e.ctrlKey || e.altKey || e.metaKey) return;
      if (e.key === "n" || e.key === "N") {
        e.preventDefault();
        navigate("/bots/new");
      } else if (e.key === "/") {
        const field = document.querySelector<HTMLElement>("[data-page-search]");
        if (field) {
          e.preventDefault();
          field.focus();
        }
      } else if (e.key === "Escape") {
        setDrawerOpen(false);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [toggleNav]);

  // Drawer sidebar: move focus in when it opens.
  const sidebarWrap = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (mode === "drawer" && drawerOpen) {
      requestAnimationFrame(() =>
        sidebarWrap.current?.querySelector<HTMLElement>('[aria-current="page"], a, button')?.focus(),
      );
    }
  }, [drawerOpen, mode]);
  const trapDrawer = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (mode !== "drawer" || !drawerOpen || e.key !== "Tab" || !sidebarWrap.current) return;
    const items = sidebarWrap.current.querySelectorAll<HTMLElement>("a, button");
    if (items.length === 0) return;
    const first = items[0];
    const last = items[items.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  };

  const slots = useMemo(() => ({ heading, actions }), [heading, actions]);

  return (
    <ShellSlotsContext.Provider value={slots}>
      <div className="ae-shell" data-nav={mode} data-drawer-open={(mode === "drawer" && drawerOpen) || undefined}>
        <a
          href="#ae-main-content"
          className="ae-skiplink"
          onClick={(e) => {
            e.preventDefault();
            document.getElementById("ae-main-content")?.focus();
          }}
        >
          {t("nav.skipToContent")}
        </a>
        <div className="ae-shell__sidebar" ref={sidebarWrap} onKeyDown={trapDrawer}>
          <Sidebar mode={mode} onToggle={toggleNav} onClose={mode === "drawer" ? () => setDrawerOpen(false) : undefined} />
        </div>
        {mode === "drawer" && drawerOpen ? (
          <button
            type="button"
            className="ae-shell__scrim"
            aria-label={t("nav.closeMenu")}
            tabIndex={-1}
            onClick={() => setDrawerOpen(false)}
          />
        ) : null}
        <Header
          showMenu={mode === "drawer"}
          menuOpen={drawerOpen}
          onMenu={() => setDrawerOpen((v) => !v)}
          headingRef={setHeading}
          actionsRef={setActions}
          unread={unread}
          alertsOpen={alertsOpen}
          onAlerts={openAlerts}
        />
        <main className="ae-shell__main" tabIndex={-1}>
          <UpdateBanner />
          <Suspense fallback={<p className="ae-shell__loading">{t("workspace.loading")}</p>}>
            {heading ? <div key={route.path} className="ae-shell__page">{renderRoute(route)}</div> : null}
          </Suspense>
        </main>
        <StatusBar />
        <AlertsDrawer open={alertsOpen} alerts={alerts} onClose={() => setAlertsOpen(false)} />
      </div>
    </ShellSlotsContext.Provider>
  );
}
