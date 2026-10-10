import { useEffect, useState } from "react";
import { appInfo, type AppInfo } from "@/lib/ipc/app/about";
import { linkState, type LinkStateView } from "@/lib/ipc/link/link";

export const SETTINGS_TABS = ["general", "notifications", "devices", "keys", "about"] as const;
export type SettingsTab = (typeof SETTINGS_TABS)[number];

export function isSettingsTab(v: string | null): v is SettingsTab {
  return (SETTINGS_TABS as readonly string[]).includes(v ?? "");
}

let infoRequest: Promise<AppInfo> | null = null;

/** This installation (app_info). Read once per app run: none of it changes while the app runs. */
export function useAppInfo(): AppInfo | null {
  const [info, setInfo] = useState<AppInfo | null>(null);
  useEffect(() => {
    let alive = true;
    (infoRequest ??= appInfo())
      .then((i) => {
        if (alive) setInfo(i);
      })
      .catch(() => {
        infoRequest = null;
      });
    return () => {
      alive = false;
    };
  }, []);
  return info;
}

/** Tests only: forget the cached app_info read. */
export function resetAppInfoForTests(): void {
  infoRequest = null;
}

const LINK_POLL_MS = 15_000;

/** The phone link's state for the section list and the summary (Devices polls its own, faster). */
export function useLinkStateView(): LinkStateView | null {
  const [view, setView] = useState<LinkStateView | null>(null);
  useEffect(() => {
    let alive = true;
    const read = () =>
      linkState()
        .then((v) => {
          if (alive) setView(v);
        })
        .catch(() => undefined);
    void read();
    const id = window.setInterval(read, LINK_POLL_MS);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);
  return view;
}
