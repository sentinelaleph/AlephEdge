/**
 * The one state -> tone map for run and connection states. Every chip that
 * shows "is this thing working" reads its colour here, so "Not configured" is
 * never teal on one page and amber on another.
 *
 *   positive  running, connected, ok           (green)
 *   neutral   stopped, idle, paused by user     (grey)
 *   waiting   armed, connecting                 (aqua: working, not yet active)
 *   warning   not configured, needs setup, retrying, closing only, budget hold
 *   negative  error, dead, down, kill switch, disconnected
 */

export type StatusKind =
  | "running"
  | "connected"
  | "ok"
  | "stopped"
  | "idle"
  | "paused"
  | "armed"
  | "connecting"
  | "notConfigured"
  | "needsSetup"
  | "retrying"
  | "closingOnly"
  | "budget"
  | "underReview"
  | "error"
  | "dead"
  | "down"
  | "killSwitch"
  | "disconnected";

export type StatusTone = "positive" | "neutral" | "waiting" | "warning" | "negative";

export const STATUS_TONE: Record<StatusKind, StatusTone> = {
  running: "positive",
  connected: "positive",
  ok: "positive",
  stopped: "neutral",
  idle: "neutral",
  paused: "neutral",
  armed: "waiting",
  connecting: "waiting",
  notConfigured: "warning",
  needsSetup: "warning",
  retrying: "warning",
  closingOnly: "warning",
  budget: "warning",
  underReview: "warning",
  error: "negative",
  dead: "negative",
  down: "negative",
  killSwitch: "negative",
  disconnected: "negative",
};

export function statusTone(kind: StatusKind): StatusTone {
  return STATUS_TONE[kind];
}

/** i18n key of the default sentence-case label ("Not configured", never "NOT CONFIGURED"). */
export function statusLabelKey(kind: StatusKind): string {
  return `status.${kind}`;
}
