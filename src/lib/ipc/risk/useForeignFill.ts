import { useEffect, useSyncExternalStore } from "react";
import { foreignFillAck, foreignFillSetEnabled, foreignFillStatus, type ForeignFillStatus } from "./foreignFill";

/**
 * The foreign-trade watch, app-wide. One poll (started by the DeskProvider)
 * feeds the header alerts, the dashboard and the Risk page. A paper build
 * answers `liveBuild: false` once and is not polled again.
 */

const POLL_MS = 15_000;

let current: ForeignFillStatus | null = null;
const listeners = new Set<() => void>();

function publish(next: ForeignFillStatus) {
  current = next;
  listeners.forEach((l) => l());
}

export async function refreshForeignFill(): Promise<ForeignFillStatus | null> {
  try {
    publish(await foreignFillStatus());
  } catch {
    /* keep the last known state; the panel shows its check time */
  }
  return current;
}

/** Starts the poll; mount once (DeskProvider). Returns the current state. */
export function useForeignFillPolling(): ForeignFillStatus | null {
  useEffect(() => {
    let alive = true;
    let timer: number | undefined;
    void refreshForeignFill().then((s) => {
      if (alive && s?.liveBuild) timer = window.setInterval(() => void refreshForeignFill(), POLL_MS);
    });
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, []);
  return useForeignFill().status;
}

export function useForeignFill() {
  const status = useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => current,
  );
  return {
    status,
    ack: async () => publish(await foreignFillAck()),
    setEnabled: async (on: boolean) => publish(await foreignFillSetEnabled(on)),
  };
}
