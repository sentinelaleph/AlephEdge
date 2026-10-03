import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { errorMessage } from "@/lib/ipc/bridge";
import {
  strategyArchive,
  strategyClose,
  strategyList,
  strategyNotes,
  strategyRiskGet,
  strategyRiskSet,
  strategyStart,
  strategyStop,
  type StrategyBotView,
  type StrategyNote,
  type StrategyRiskView,
} from "./strategy";

export type StrategyAction = "start" | "stop" | "close" | "archive";

export interface StrategyDeskController {
  /** Null until the first list arrives. */
  bots: StrategyBotView[] | null;
  risk: StrategyRiskView | null;
  notes: StrategyNote[];
  /** Raw error code of the last failed load (`strategy.errors.*` or text). */
  loadError: string | null;
  /** Bot id an action is running for. */
  busyId: string | null;
  refresh: () => Promise<void>;
  /** Resolves to the raw error code on failure, null on success. */
  act: (id: string, action: StrategyAction) => Promise<string | null>;
  rearmBreaker: () => Promise<string | null>;
}

const POLL_MS = 4000;

/** One poller for the strategy desk (list, risk, notes), owned by DeskProvider. */
export function useStrategyDesk(): StrategyDeskController {
  const [bots, setBots] = useState<StrategyBotView[] | null>(null);
  const [risk, setRisk] = useState<StrategyRiskView | null>(null);
  const [notes, setNotes] = useState<StrategyNote[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const alive = useRef(true);

  const refresh = useCallback(async () => {
    try {
      const [b, r, n] = await Promise.all([strategyList(), strategyRiskGet(), strategyNotes()]);
      if (!alive.current) return;
      setBots([...b].sort((x, y) => y.createdAt - x.createdAt));
      setRisk(r);
      setNotes(n);
      setLoadError(null);
    } catch (e) {
      if (alive.current) setLoadError(errorMessage(e, "storeReadFailed"));
    }
  }, []);

  useEffect(() => {
    alive.current = true;
    void refresh();
    const timer = window.setInterval(() => void refresh(), POLL_MS);
    return () => {
      alive.current = false;
      window.clearInterval(timer);
    };
  }, [refresh]);

  const act = useCallback(
    async (id: string, action: StrategyAction) => {
      setBusyId(id);
      try {
        if (action === "start") await strategyStart(id);
        else if (action === "stop") await strategyStop(id);
        else if (action === "close") await strategyClose(id);
        else await strategyArchive(id);
        await refresh();
        return null;
      } catch (e) {
        return errorMessage(e, "botUnknown");
      } finally {
        if (alive.current) setBusyId(null);
      }
    },
    [refresh],
  );

  const rearmBreaker = useCallback(async () => {
    try {
      const r = await strategyRiskSet({ rearm: true });
      if (alive.current) setRisk(r);
      return null;
    } catch (e) {
      return errorMessage(e, "portfolioDdTripped");
    }
  }, []);

  return useMemo(
    () => ({ bots, risk, notes, loadError, busyId, refresh, act, rearmBreaker }),
    [bots, risk, notes, loadError, busyId, refresh, act, rearmBreaker],
  );
}
