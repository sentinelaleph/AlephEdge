import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  tradesExportCsv,
  tradesList,
  tradesStats,
  type PnlStats,
  type TradeRecord,
} from "@/lib/ipc/trades/trades";
import { errorMessage } from "@/lib/ipc/bridge";

const POLL_MS = 5000;

/** Which money the stats describe in a live build. */
export type PnlScope = "simulated" | "real";

export interface PnlController {
  /** Stats for `scope` when `split`, for everything otherwise. */
  stats: PnlStats | null;
  /**
   * True in a live build: stats are fetched per scope and never summed
   * across real and simulated money.
   */
  split: boolean;
  scope: PnlScope;
  setScope: (scope: PnlScope) => void;
  trades: TradeRecord[];
  /** Path of the last CSV export, shown after a successful export. */
  exportedTo: string | null;
  /** Set when the last export failed, so the button doesn't fail silently. */
  exportError: string | null;
  exportCsv: () => Promise<void>;
}

/**
 * Polls trade history + stats; exposes the one-click CSV export.
 *
 * `split` is the build's `liveTradingEnabled`, or null while the bot desk has
 * not reported it yet. Stats are not fetched while it is null: an unscoped
 * call in a live build would sum real and simulated money.
 */
export function usePnl(split: boolean | null): PnlController {
  const { t } = useTranslation();
  const [stats, setStats] = useState<PnlStats | null>(null);
  const [scope, setScope] = useState<PnlScope>("simulated");
  const [trades, setTrades] = useState<TradeRecord[]>([]);
  const [exportedTo, setExportedTo] = useState<string | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    // Restarted whenever the scope changes; the previous scope's figures
    // are cleared so they are never shown under the new label.
    let live = true;
    setStats(null);
    let timer: number | undefined;
    const statsCall = (): Promise<PnlStats | null> =>
      split === null ? Promise.resolve(null) : tradesStats(split ? scope === "real" : undefined);
    const tick = async () => {
      try {
        const [s, t] = await Promise.all([statsCall(), tradesList(50)]);
        if (alive.current && live) {
          setStats(s);
          setTrades(t);
        }
      } catch {
        /* keep last-good */
      }
      if (alive.current && live) timer = window.setTimeout(tick, POLL_MS);
    };
    void tick();
    return () => {
      live = false;
      if (timer) window.clearTimeout(timer);
    };
  }, [split, scope]);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const exportCsv = useCallback(async () => {
    setExportError(null);
    setExportedTo(null);
    try {
      const path = await tradesExportCsv();
      if (alive.current) setExportedTo(path);
    } catch (e) {
      if (alive.current) setExportError(errorMessage(e, t("pnl.exportFailed")));
    }
  }, [t]);

  return { stats, split: split === true, scope, setScope, trades, exportedTo, exportError, exportCsv };
}
