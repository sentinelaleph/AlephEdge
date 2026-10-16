import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { errorMessage } from "../bridge";
import { riskviewGet, type RiskViewSnapshot } from "./riskview";

/** How often the desk asks for the snapshot (Rust recomputes every 5 min, or when older than 1 min). */
const POLL_MS = 60_000;

export interface RiskViewController {
  snapshot: RiskViewSnapshot | null;
  loading: boolean;
  error: string | null;
  /** Recomputes now (new positions, a changed threshold). */
  refresh: () => Promise<void>;
}

/** Owns the portfolio risk view snapshot for the whole desk (alerts read it too). */
export function useRiskView(): RiskViewController {
  const [snapshot, setSnapshot] = useState<RiskViewSnapshot | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);

  const load = useCallback(async (refresh: boolean) => {
    setLoading(true);
    try {
      const s = await riskviewGet(refresh);
      if (!alive.current) return;
      setSnapshot(s);
      setError(null);
    } catch (e) {
      if (alive.current) setError(errorMessage(e, "riskviewFailed"));
    } finally {
      if (alive.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    alive.current = true;
    void load(false);
    const id = window.setInterval(() => void load(false), POLL_MS);
    return () => {
      alive.current = false;
      window.clearInterval(id);
    };
  }, [load]);

  const refresh = useCallback(() => load(true), [load]);
  return useMemo(() => ({ snapshot, loading, error, refresh }), [snapshot, loading, error, refresh]);
}
