import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { errorMessage } from "@/lib/ipc/bridge";
import {
  riskGet,
  riskSetBalance,
  riskSetCloseOnStop,
  riskSetDailyLoss,
  riskSetLevel,
  type RiskLevel,
  type RiskState,
} from "@/lib/ipc/risk/risk";

export interface RiskController {
  state: RiskState | null;
  busy: boolean;
  /** The last failure (initial load or a change), shown in the selector. */
  error: string | null;
  /** Re-runs the initial load after it failed (`state` still null). */
  retry: () => void;
  selectLevel: (level: RiskLevel) => Promise<void>;
  setBalance: (balance: number) => Promise<void>;
  setCloseOnStop: (close: boolean) => Promise<void>;
  /** The user's own daily-loss tolerance; `null` clears it back to the level. */
  setDailyLoss: (pct: number | null) => Promise<void>;
}

/** Owns the global risk config: level, simulated balance, close-on-stop. */
export function useRisk(): RiskController {
  const { t } = useTranslation();
  const [state, setState] = useState<RiskState | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    riskGet()
      .then((s) => {
        if (!alive.current) return;
        setState(s);
        setError(null);
      })
      .catch((e: unknown) => {
        if (alive.current) setError(errorMessage(e, t("risk.loadFailed")));
      });
    return () => {
      alive.current = false;
    };
    // `t` is left out on purpose: a language switch must not re-fetch.
  }, [attempt]);

  const retry = useCallback(() => setAttempt((n) => n + 1), []);

  // A rejected change keeps the last good state (the server did not apply
  // it) and says why, instead of an unhandled rejection and a silent no-op.
  const run = useCallback(
    async (action: () => Promise<RiskState>) => {
      setBusy(true);
      setError(null);
      try {
        const next = await action();
        if (alive.current) setState(next);
      } catch (e) {
        if (alive.current) setError(errorMessage(e, t("risk.saveFailed")));
      } finally {
        if (alive.current) setBusy(false);
      }
    },
    [t],
  );

  const selectLevel = useCallback((level: RiskLevel) => run(() => riskSetLevel(level)), [run]);
  const setBalance = useCallback((balance: number) => run(() => riskSetBalance(balance)), [run]);
  const setCloseOnStop = useCallback(
    (close: boolean) => run(() => riskSetCloseOnStop(close)),
    [run],
  );

  const setDailyLoss = useCallback((pct: number | null) => run(() => riskSetDailyLoss(pct)), [run]);

  return { state, busy, error, retry, selectLevel, setBalance, setCloseOnStop, setDailyLoss };
}
