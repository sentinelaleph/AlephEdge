import { useEffect } from "react";
import { vaultTouch } from "@/lib/ipc/vault/vault";

/** At most one touch per minute: the watchdog checks the budget once a minute. */
export const TOUCH_INTERVAL_MS = 60_000;

/**
 * True when activity at `now` should be reported: the first time, then once
 * per `intervalMs`. Pure, so the throttle is testable without a DOM.
 */
export function createActivityThrottle(intervalMs: number = TOUCH_INTERVAL_MS): (now: number) => boolean {
  let last = Number.NEGATIVE_INFINITY;
  return (now) => {
    if (now - last < intervalMs) return false;
    last = now;
    return true;
  };
}

/**
 * Reports pointer and key input to the vault, so auto-lock means the user
 * left, not that no exchange key was read (audit 2026-10-08, ux_global 3).
 * Mounted with the desk; a locked vault ignores the touch.
 */
export function useVaultActivity(): void {
  useEffect(() => {
    const due = createActivityThrottle();
    const onActivity = () => {
      if (due(Date.now())) void vaultTouch().catch(() => undefined);
    };
    window.addEventListener("pointerdown", onActivity, { capture: true, passive: true });
    window.addEventListener("keydown", onActivity, { capture: true, passive: true });
    window.addEventListener("wheel", onActivity, { capture: true, passive: true });
    return () => {
      window.removeEventListener("pointerdown", onActivity, { capture: true });
      window.removeEventListener("keydown", onActivity, { capture: true });
      window.removeEventListener("wheel", onActivity, { capture: true });
    };
  }, []);
}
