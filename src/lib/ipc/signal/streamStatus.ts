/**
 * One reading of the Sentinel stream's state for every surface (status bar,
 * sidebar badge, stream card, signals banner, alerts), driven by the Rust
 * `phase` rather than by whether `lastError` happens to hold text. A normal
 * connect is "connecting", never an error; an outage is raised only when the
 * supervisor says it needs the user (`down`) or has been failing for longer
 * than STREAM_ALERT_AFTER_SECS.
 */

import { localizeError } from "@/lib/errorText";
import type { SignalHealthInfo } from "./signal";

/** A stream out of "live" this long while (re)connecting is an outage, not a blip. */
export const STREAM_ALERT_AFTER_SECS = 60;

export type StreamTone = "success" | "warn" | "danger" | "muted";

const overdue = (h: SignalHealthInfo) => (h.notLiveSecs ?? 0) > STREAM_ALERT_AFTER_SECS;

/** Dot/chip tone. */
export function streamTone(h: SignalHealthInfo, loaded = true): StreamTone {
  if (!loaded) return "muted";
  switch (h.phase) {
    case "live":
      return "success";
    case "down":
      return "danger";
    case "retrying":
      return overdue(h) ? "danger" : "warn";
    case "connecting":
      return "warn";
    default:
      return "muted";
  }
}

/** i18n key of the short state label. */
export function streamLabelKey(h: SignalHealthInfo): string {
  switch (h.phase) {
    case "live":
      return "statusbar.streamConnected";
    case "connecting":
      return "statusbar.connecting";
    case "retrying":
      return "statusbar.streamReconnecting";
    case "down":
      return "statusbar.streamDown";
    default:
      return "statusbar.streamIdle";
  }
}

/** The failure to show, localized; only while retrying or down. */
export function streamErrorText(h: SignalHealthInfo): string | undefined {
  if ((h.phase !== "retrying" && h.phase !== "down") || !h.lastError) return undefined;
  return localizeError(h.lastError);
}

/**
 * Whether the stream deserves an alert, and how loud. `down` always (it will
 * not recover without the user); retrying/connecting only past the threshold.
 */
export function streamAlertSeverity(h: SignalHealthInfo): "danger" | "warn" | null {
  if (h.phase === "down") return "danger";
  if ((h.phase === "retrying" || h.phase === "connecting") && overdue(h)) {
    return h.phase === "retrying" ? "danger" : "warn";
  }
  return null;
}
