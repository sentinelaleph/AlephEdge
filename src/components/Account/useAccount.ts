import { useCallback, useEffect, useRef, useState } from "react";
import {
  CLOSE_CONFIRMATION,
  exchangeAccount,
  exchangeCloseAll,
  exchangeClosePosition,
  type FuturesAccount,
} from "@/lib/ipc/exchange/exchange";
import { localizeError } from "@/lib/errorText";

const POLL_MS = 15000;

export interface AccountState {
  account: FuturesAccount | null;
  /** The LATEST refresh failure; null once a refresh succeeds again. */
  error: string | null;
  /** When `account` was last fetched successfully (UNIX ms). */
  updatedAt: number | null;
  loading: boolean;
  /** A close order is in flight (symbol, or "*" for close-all). Blocks the UI. */
  closing: string | null;
  /** Last close-order failure, surfaced next to the positions. */
  closeError: string | null;
  closePosition: (symbol: string) => Promise<void>;
  closeAll: () => Promise<void>;
}

/**
 * Polls the connected exchange's USDT-M futures account (balance + open
 * positions) and exposes the manual close controls. Keeps the last good
 * snapshot on a transient error so the panel doesn't flash empty, and surfaces
 * the error text for a persistent failure. The close actions place REAL,
 * reduce-only market orders — user-initiated, distinct from the simulation
 * bots — then refresh so the position list reflects the new reality.
 */
export function useAccount(exchangeId: string | null): AccountState {
  const [account, setAccount] = useState<FuturesAccount | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [updatedAt, setUpdatedAt] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);
  const [closing, setClosing] = useState<string | null>(null);
  const [closeError, setCloseError] = useState<string | null>(null);
  const alive = useRef(true);

  const refresh = useCallback(async () => {
    if (!exchangeId) return;
    setLoading(true);
    try {
      const a = await exchangeAccount(exchangeId);
      if (alive.current) {
        setAccount(a);
        setUpdatedAt(Date.now());
        setError(null);
      }
    } catch (e) {
      if (alive.current) setError(localizeError(e instanceof Error ? e.message : String(e)));
    } finally {
      if (alive.current) setLoading(false);
    }
  }, [exchangeId]);

  useEffect(() => {
    alive.current = true;
    if (!exchangeId) {
      setAccount(null);
      setUpdatedAt(null);
      setError(null);
      return;
    }
    let timer: number | undefined;
    const tick = async () => {
      await refresh();
      if (alive.current) timer = window.setTimeout(tick, POLL_MS);
    };
    void tick();
    return () => {
      alive.current = false;
      if (timer) window.clearTimeout(timer);
    };
  }, [exchangeId, refresh]);

  // Runs one close action, tagged by `tag` (a symbol, or "*" for close-all), so
  // the UI can disable just the row in flight. Refreshes on success AND failure
  // — a partial close still changed the account, so the list must re-sync.
  const runClose = useCallback(
    async (tag: string, action: () => Promise<unknown>) => {
      if (!exchangeId || closing) return;
      setCloseError(null);
      setClosing(tag);
      try {
        await action();
      } catch (e) {
        if (alive.current) setCloseError(localizeError(e instanceof Error ? e.message : String(e)));
      } finally {
        await refresh();
        if (alive.current) setClosing(null);
      }
    },
    [exchangeId, closing, refresh],
  );

  const closePosition = useCallback(
    (symbol: string) => runClose(symbol, () => exchangeClosePosition(exchangeId!, symbol, CLOSE_CONFIRMATION)),
    [exchangeId, runClose],
  );
  const closeAll = useCallback(
    () => runClose("*", () => exchangeCloseAll(exchangeId!, CLOSE_CONFIRMATION)),
    [exchangeId, runClose],
  );

  return { account, error, updatedAt, loading, closing, closeError, closePosition, closeAll };
}
