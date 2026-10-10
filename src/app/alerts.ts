/**
 * In-app alerts, derived in TS from state the desk already polls (bot_status,
 * signal_health, trades_list, strategy_list, strategy_pnl) plus the vault
 * auto-lock event. Nothing here is
 * invented: an alert exists only while its source says so.
 */

import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import type { TFunction } from "i18next";
import type { DeskContextValue } from "./DeskProvider";
import { BUDGET_REASONS, groupSkips, isBotFailure, skipParams } from "@/lib/skipNotes";
import { streamAlertSeverity, streamErrorText } from "@/lib/ipc/signal/streamStatus";

export const ALERT_CATEGORIES = [
  "killSwitch",
  "botError",
  "budget",
  "stopLoss",
  "dealClosed",
  "vaultAutoLock",
  "streamDown",
  "regime",
] as const;
export type AlertCategory = (typeof ALERT_CATEGORIES)[number];

export interface DeskAlert {
  key: string;
  category: AlertCategory;
  severity: "danger" | "warn" | "info";
  title: string;
  detail?: string;
  count?: number;
  /** When the source event happened (UNIX ms), if known. */
  at?: number;
}

/* ---- Vault auto-lock: the gate takes over, so the time is kept for after unlock. ---- */

let autoLockedAt: number | null = null;
const autoLockListeners = new Set<() => void>();

export function recordVaultAutoLock(at: number) {
  autoLockedAt = at;
  autoLockListeners.forEach((l) => l());
}

export function useVaultAutoLockedAt(): number | null {
  return useSyncExternalStore(
    (l) => {
      autoLockListeners.add(l);
      return () => autoLockListeners.delete(l);
    },
    () => autoLockedAt,
  );
}

function utcDayStart(now: number): number {
  const d = new Date(now);
  return Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate());
}

/** DCA / Grid exits that a protective level forced (strategy.exit.*). */
const CYCLE_STOP_EXITS = new Set<string>(["sl", "liq", "stop", "ddstop", "portfolioDd", "dailyStop"]);

/** Current alerts, newest first, at most 20. */
export function deriveAlerts(ctx: DeskContextValue, t: TFunction, lockedAt: number | null, now = Date.now()): DeskAlert[] {
  const out: DeskAlert[] = [];
  const s = ctx.desk.status;

  if (ctx.desk.loaded && s.killSwitchTripped) {
    out.push({
      key: "killSwitch",
      category: "killSwitch",
      severity: "danger",
      title: t("alerts.killSwitch"),
      detail: ctx.risk.state
        ? t("alerts.killSwitchDetail", { pct: ctx.risk.state.effectiveDailyLossPct })
        : undefined,
    });
  }
  if (ctx.strategy.risk?.tripped) {
    out.push({
      key: "strategyBreaker",
      category: "killSwitch",
      severity: "danger",
      title: t("alerts.strategyBreaker"),
      detail: t("alerts.strategyBreakerDetail", { pct: ctx.strategy.risk.portfolioDdPct }),
    });
  }
  if (ctx.desk.error) {
    out.push({ key: "deskError", category: "botError", severity: "danger", title: t("alerts.botActionFailed"), detail: ctx.desk.error });
  }
  // Phase-driven: a normal connect never alerts; an outage does once Rust
  // says it needs the user, or after STREAM_ALERT_AFTER_SECS of retrying.
  const streamSeverity = ctx.feed.loaded ? streamAlertSeverity(ctx.feed.health) : null;
  if (streamSeverity) {
    out.push({
      key: "streamDown",
      category: "streamDown",
      severity: streamSeverity,
      title: t("alerts.streamDown"),
      detail: streamErrorText(ctx.feed.health),
    });
  }
  if (ctx.desk.loaded && s.btcRegime === "break") {
    out.push({ key: "btcBreak", category: "regime", severity: "warn", title: t("alerts.btcBreak") });
  }

  for (const g of groupSkips(s.recentSkips)) {
    const failure = isBotFailure(g.reason);
    const budget = BUDGET_REASONS.has(g.reason);
    if (!failure && !budget) continue;
    out.push({
      key: `skip:${g.reason}`,
      category: failure ? "botError" : "budget",
      severity: failure ? "danger" : "warn",
      title: t(`bots.skipReasons.${g.reason}`, skipParams(g.latest)),
      detail: g.latest.symbol,
      count: g.count,
      at: g.latest.atMs,
    });
  }

  const today = utcDayStart(now);
  const closedToday = ctx.pnl.trades.filter((tr) => tr.closedAt >= today);
  const slToday = closedToday.filter((tr) => tr.exitReason === "sl");
  if (slToday.length > 0) {
    out.push({
      key: "stopLoss",
      category: "stopLoss",
      severity: "warn",
      title: t("alerts.stopLossToday", { count: slToday.length }),
      count: slToday.length,
      at: Math.max(...slToday.map((tr) => tr.closedAt)),
    });
  }
  if (closedToday.length > 0) {
    out.push({
      key: "dealClosed",
      category: "dealClosed",
      severity: "info",
      title: t("alerts.closedToday", { count: closedToday.length }),
      count: closedToday.length,
      at: Math.max(...closedToday.map((tr) => tr.closedAt)),
    });
  }
  // DCA / Grid (paper): cycles closed today, those a stop closed, and bots a
  // stop or liquidation ended. They write no trade rows, so the alerts above
  // never saw them.
  const cyclesToday = (ctx.strategy.pnl?.recent ?? []).filter((c) => c.closedAt >= today);
  const stoppedToday = cyclesToday.filter((c) => c.exitReason !== null && CYCLE_STOP_EXITS.has(c.exitReason));
  if (stoppedToday.length > 0) {
    out.push({
      key: "cycleStop",
      category: "stopLoss",
      severity: stoppedToday.some((c) => c.exitReason === "liq") ? "danger" : "warn",
      title: t("alerts.cycleStopsToday", { count: stoppedToday.length }),
      count: stoppedToday.length,
      at: Math.max(...stoppedToday.map((c) => c.closedAt)),
    });
  }
  const closedCycles = ctx.strategy.pnl?.totals.todayCycles ?? 0;
  if (closedCycles > 0) {
    out.push({
      key: "cycleClosed",
      category: "dealClosed",
      severity: "info",
      title: t("alerts.cyclesClosedToday", { count: closedCycles }),
      count: closedCycles,
      at: cyclesToday.length > 0 ? Math.max(...cyclesToday.map((c) => c.closedAt)) : undefined,
    });
  }
  for (const b of ctx.strategy.bots ?? []) {
    if (b.runState !== "dead") continue;
    out.push({
      key: `strategyDead:${b.id}`,
      category: "botError",
      severity: "danger",
      title: t("alerts.strategyDead", { name: b.name }),
      detail: b.deadReason ? t(`strategy.exit.${b.deadReason}`, { defaultValue: b.deadReason }) : undefined,
    });
  }

  if (lockedAt !== null) {
    out.push({ key: "vaultAutoLock", category: "vaultAutoLock", severity: "info", title: t("alerts.vaultAutoLocked"), at: lockedAt });
  }

  const rank = { danger: 0, warn: 1, info: 2 } as const;
  return out.sort((a, b) => rank[a.severity] - rank[b.severity] || (b.at ?? now) - (a.at ?? now)).slice(0, 20);
}

/* ---- Which categories the user wants to see (Settings > Notifications). ---- */

const PREFS_KEY = "aleph-edge-notify";
type Prefs = Record<AlertCategory, boolean>;
const ALL_ON = Object.fromEntries(ALERT_CATEGORIES.map((c) => [c, true])) as Prefs;

let prefs: Prefs = ALL_ON;
let storageBlocked = false;
try {
  const raw = localStorage.getItem(PREFS_KEY);
  if (raw) prefs = { ...ALL_ON, ...(JSON.parse(raw) as Partial<Prefs>) };
} catch {
  storageBlocked = true;
}
const prefListeners = new Set<() => void>();

export function useAlertPrefs(): {
  prefs: Prefs;
  storageBlocked: boolean;
  setPref: (c: AlertCategory, on: boolean) => void;
} {
  const current = useSyncExternalStore(
    (l) => {
      prefListeners.add(l);
      return () => prefListeners.delete(l);
    },
    () => prefs,
  );
  const [blocked, setBlocked] = useState(storageBlocked);
  const setPref = useCallback((c: AlertCategory, on: boolean) => {
    prefs = { ...prefs, [c]: on };
    try {
      localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));
    } catch {
      storageBlocked = true;
      setBlocked(true);
    }
    prefListeners.forEach((l) => l());
  }, []);
  return { prefs: current, storageBlocked: blocked, setPref };
}

/** Re-renders every `ms` so ages and "today" stay current. */
export function useNow(ms = 30_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), ms);
    return () => window.clearInterval(id);
  }, [ms]);
  return now;
}
