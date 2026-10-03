import { useCallback, useEffect, useRef, useState } from "react";
import {
  signalConnect,
  signalHealth,
  signalRecent,
  type Signal,
  type SignalHealthInfo,
} from "@/lib/ipc/signal/signal";

const POLL_MS = 3000;

export interface SignalFeedState {
  signals: Signal[];
  health: SignalHealthInfo;
  /** True once the first poll answered (until then the empty list means "not read yet"). */
  loaded: boolean;
  /** When the last poll succeeded (UNIX ms). */
  updatedAt: number | null;
  /** Asks the Rust client to (re)connect the stream (signal_connect). */
  reconnect: () => Promise<void>;
}

const EMPTY_HEALTH: SignalHealthInfo = { connected: false, phase: "idle" };

/**
 * The app-level Sentinel stream. Mounted ONCE, by DeskProvider, after the
 * membership and vault gates have passed, so moving between pages never
 * restarts the stream or its polling. It connects on mount; the stream is torn
 * down on sign-out (App), never on unmount.
 */
export function useSignalFeed(): SignalFeedState {
  const [signals, setSignals] = useState<Signal[]>([]);
  const [health, setHealth] = useState<SignalHealthInfo>(EMPTY_HEALTH);
  const [loaded, setLoaded] = useState(false);
  const [updatedAt, setUpdatedAt] = useState<number | null>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    void signalConnect();

    let timer: number | undefined;
    const tick = async () => {
      try {
        const [recent, h] = await Promise.all([signalRecent(), signalHealth()]);
        if (alive.current) {
          setSignals(recent);
          setHealth(h);
          setLoaded(true);
          setUpdatedAt(Date.now());
        }
      } catch (e) {
        console.warn("[aleph-edge signal] health poll threw:", e);
        /* keep last-good state on a transient error */
      }
      if (alive.current) timer = window.setTimeout(tick, POLL_MS);
    };
    void tick();

    return () => {
      alive.current = false;
      if (timer) window.clearTimeout(timer);
      // Intentionally NOT disconnecting: the signal client is an app-lifetime
      // singleton. A StrictMode remount's connect→disconnect→connect churn could
      // otherwise leave the supervisor's desired state false so it never
      // reconnects (the "stuck Disconnected with lastError: null" symptom).
      // The stream is torn down on sign-out, not on this panel unmounting.
    };
  }, []);

  const reconnect = useCallback(async () => {
    await signalConnect();
    const h = await signalHealth();
    if (alive.current) setHealth(h);
  }, []);

  return { signals, health, loaded, updatedAt, reconnect };
}
