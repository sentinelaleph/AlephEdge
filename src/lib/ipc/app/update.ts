/**
 * Self-update bridge (src-tauri/src/app/update.rs). The Rust side checks the
 * signed feed and installs; the webview only asks and confirms.
 */

import { devMock, inTauri, invoke, listenEvent, type Unlisten } from "../bridge";

export interface AvailableUpdate {
  version: string;
  notes: string | null;
  /** Publication date from the feed, UNIX ms. */
  dateMs: number | null;
}

export interface UpdateCheck {
  current: string;
  available: AvailableUpdate | null;
  /** This build does not update itself (the TESTNET build). */
  off?: boolean;
}

export interface UpdateProgress {
  downloaded: number;
  total: number | null;
}

/** Sent with an install (`UPDATE_CONFIRMATION` in update.rs). */
export const UPDATE_CONFIRMATION = "UPDATE";

export function checkUpdate(): Promise<UpdateCheck> {
  return inTauri()
    ? invoke<UpdateCheck>("app_check_update")
    : devMock(
        async () => ({}),
        async () => ({ current: "dev", available: null }),
      );
}

export function installUpdate(): Promise<void> {
  return inTauri()
    ? invoke<void>("app_install_update", { confirmation: UPDATE_CONFIRMATION })
    : devMock(
        async () => ({}),
        async () => undefined,
      );
}

export function onUpdateProgress(handler: (p: UpdateProgress) => void): Promise<Unlisten> {
  return listenEvent<UpdateProgress>("app:update-progress", handler);
}
