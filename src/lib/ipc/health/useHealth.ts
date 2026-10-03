import { useEffect, useRef, useState } from "react";
import { fetchHealthSnapshot, type HealthSnapshot } from "./health";

/** Polls get_health_snapshot. Mounted once, by DeskProvider. */
export function useHealth(intervalMs = 4000): HealthSnapshot | null {
  const [snapshot, setSnapshot] = useState<HealthSnapshot | null>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    let timer: number | undefined;

    const tick = async () => {
      try {
        const snap = await fetchHealthSnapshot();
        if (alive.current) setSnapshot(snap);
      } catch {
        /* keep last good snapshot on transient error */
      }
      if (alive.current) timer = window.setTimeout(tick, intervalMs);
    };
    void tick();

    return () => {
      alive.current = false;
      if (timer) window.clearTimeout(timer);
    };
  }, [intervalMs]);

  return snapshot;
}
