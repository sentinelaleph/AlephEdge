import { useEffect, useSyncExternalStore } from "react";
import { strategyLiveStatus, type StrategyLiveView } from "@/lib/ipc/strategy/strategy";

/**
 * Real-money state of every DCA / Grid bot, app-wide. One poll (started by
 * the AppShell) feeds every reader: the header badge, sidebar, dashboard,
 * lists, positions, risk page and the bot's own panel all see the same state.
 */

const POLL_MS = 5_000;

let views: StrategyLiveView[] = [];
const listeners = new Set<() => void>();

function publish(next: StrategyLiveView[]) {
  views = next;
  listeners.forEach((l) => l());
}

export async function refreshStrategyLive(): Promise<void> {
  try {
    publish(await strategyLiveStatus());
  } catch {
    /* keep the last known state; the panel shows its sync time */
  }
}

/** Real money is in play for this view: switched on, or a position held. */
export function isLiveView(v: StrategyLiveView | null | undefined): boolean {
  return !!v && (v.enabled || v.realQty !== 0);
}

/** Starts the app-wide poll; mount once (AppShell). */
export function useStrategyLivePolling() {
  useEffect(() => {
    void refreshStrategyLive();
    const t = window.setInterval(() => void refreshStrategyLive(), POLL_MS);
    return () => window.clearInterval(t);
  }, []);
}

export function useStrategyLive() {
  const current = useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => views,
  );
  const live = current.filter(isLiveView);
  return {
    views: current,
    /** Bots with real money in play. */
    liveIds: new Set(live.map((v) => v.botId)),
    anyLive: live.length > 0,
    viewFor: (id: string) => current.find((v) => v.botId === id) ?? null,
    refresh: refreshStrategyLive,
    setViews: publish,
  };
}
