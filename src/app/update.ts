/**
 * App-wide update state: one check shortly after launch and every six hours,
 * plus the manual check in Settings. Kept outside React so the banner and the
 * About panel read the same state without a second request.
 */

import { useEffect, useSyncExternalStore } from "react";
import { errorMessage } from "@/lib/ipc/bridge";
import { checkUpdate, installUpdate, onUpdateProgress, type AvailableUpdate, type UpdateProgress } from "@/lib/ipc/app/update";

export type UpdateStatus = "idle" | "checking" | "upToDate" | "available" | "installing" | "error" | "off";

export interface UpdateState {
  status: UpdateStatus;
  current: string | null;
  available: AvailableUpdate | null;
  progress: UpdateProgress | null;
  error: string | null;
  checkedAt: number | null;
  /** The version the user chose "Later" for; its banner stays hidden. */
  dismissed: string | null;
}

const FIRST_CHECK_MS = 15_000;
const EVERY_MS = 6 * 60 * 60 * 1000;
const DISMISS_KEY = "aleph-edge-update-dismissed";

function readDismissed(): string | null {
  try {
    return localStorage.getItem(DISMISS_KEY);
  } catch {
    return null;
  }
}

let state: UpdateState = {
  status: "idle",
  current: null,
  available: null,
  progress: null,
  error: null,
  checkedAt: null,
  dismissed: readDismissed(),
};
const listeners = new Set<() => void>();
const set = (patch: Partial<UpdateState>) => {
  state = { ...state, ...patch };
  listeners.forEach((l) => l());
};

export async function runUpdateCheck(): Promise<void> {
  if (state.status === "checking" || state.status === "installing") return;
  set({ status: "checking", error: null });
  try {
    const r = await checkUpdate();
    set({ status: r.off ? "off" : r.available ? "available" : "upToDate", current: r.current, available: r.available, checkedAt: Date.now() });
  } catch (e) {
    set({ status: "error", error: errorMessage(e, "updateCheckFailed"), checkedAt: Date.now() });
  }
}

export async function runUpdateInstall(): Promise<void> {
  if (state.status !== "available") return;
  set({ status: "installing", progress: { downloaded: 0, total: null }, error: null });
  const unlisten = await onUpdateProgress((p) => set({ progress: p }));
  try {
    await installUpdate(); // the app closes or restarts on success
  } catch (e) {
    set({ status: "available", error: errorMessage(e, "updateInstallFailed"), progress: null });
  } finally {
    unlisten();
  }
}

export function useUpdateState(): UpdateState {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => state,
  );
}

/** Starts the background checks; mount once (AppShell). */
export function useUpdateChecks() {
  useEffect(() => {
    const first = window.setTimeout(() => void runUpdateCheck(), FIRST_CHECK_MS);
    const every = window.setInterval(() => void runUpdateCheck(), EVERY_MS);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(every);
    };
  }, []);
}

export function dismissVersion(version: string) {
  try {
    localStorage.setItem(DISMISS_KEY, version);
  } catch {
    /* storage blocked: the banner simply returns next launch */
  }
  set({ dismissed: version });
}
