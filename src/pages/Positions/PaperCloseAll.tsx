import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Button } from "@/components/ui/Button/Button";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog/ConfirmDialog";
import type { BotKind } from "@/lib/ipc/bot/bot";
import { errorMessage } from "@/lib/ipc/bridge";
import { isActive, strategyCloseAll } from "@/lib/ipc/strategy/strategy";
import { localizeError } from "@/lib/errorText";
import { useStrategyLive } from "@/pages/Bots/strategy/useStrategyLive";

const KINDS: BotKind[] = ["futures", "spot", "pump"];

/**
 * Paper tab emergency: stop every signal bot, close every simulated position
 * and every DCA / Grid cycle at market, and pause the DCA / Grid bots. Bots
 * are stopped first, so nothing reopens while the closes run. Real positions
 * are never touched here: they close on the Exchange tab.
 */
export function PaperCloseAll() {
  const { t } = useTranslation();
  const { desk, strategy } = useDeskContext();
  const strategyLive = useStrategyLive().anyLive;
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const s = desk.status;
  const running = KINDS.filter((k) => (k === "futures" ? s.futuresRunning : k === "spot" ? s.spotRunning : s.pumpRunning));
  const positions = s.openPositions.filter((p) => !p.live);
  const bots = strategy.bots ?? [];
  const cycles = bots.filter((b) => b.openCycle !== null).length;
  const activeStrategy = bots.filter(isActive).length;
  const total = positions.length + cycles;
  const anything = total > 0 || running.length > 0 || activeStrategy > 0;

  const run = async () => {
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      for (const k of running) await desk.stop(k);
      let closed = 0;
      let failed = 0;
      let firstError: string | null = null;
      for (const p of positions) {
        const e = await desk.closePosition(p.signalId, p.botKind);
        if (e) {
          failed += 1;
          firstError ??= e;
        } else closed += 1;
      }
      if (cycles > 0 || activeStrategy > 0) {
        const r = await strategyCloseAll("manual");
        closed += r.closed;
        failed += r.unpriced;
        await strategy.refresh();
      }
      setResult(t("positions.closeAllPaper.done", { closed, failed }));
      if (firstError) setError(localizeError(firstError));
    } catch (e) {
      setError(errorMessage(e, t("positions.closeAllPaper.failed")));
    } finally {
      setBusy(false);
      setOpen(false);
    }
  };

  return (
    <>
      <Button variant="danger" size="sm" disabled={!anything || busy} onClick={() => setOpen(true)}>
        {t("positions.closeAllPaper.button", { count: total })}
      </Button>
      {result ? (
        <span className="ae-subtle" role="status">
          {result}
        </span>
      ) : null}
      {error ? (
        <span className="ae-error" role="alert">
          {error}
        </span>
      ) : null}
      <ConfirmDialog
        open={open}
        title={t("positions.closeAllPaper.title")}
        body={
          <>
            <p>
              {t("positions.closeAllPaper.body", {
                bots: running.length,
                positions: positions.length,
                cycles,
                strategyBots: activeStrategy,
              })}
            </p>
            <p className="ae-subtle">{t("positions.closeAllPaper.realNote")}</p>
            {strategyLive ? <p className="ae-error">{t("positions.closeAllPaper.strategyLiveNote")}</p> : null}
          </>
        }
        word="CLOSE"
        confirmLabel={t("positions.closeAllPaper.confirm")}
        danger
        busy={busy}
        onCancel={() => setOpen(false)}
        onConfirm={() => void run()}
      />
    </>
  );
}
