import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Button } from "@/components/ui/Button/Button";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog/ConfirmDialog";
import { errorMessage } from "@/lib/ipc/bridge";
import { isActive, strategyCloseAll } from "@/lib/ipc/strategy/strategy";
import { strategyErrorText } from "@/lib/strategyText";
import { useStrategyLive } from "./useStrategyLive";

/**
 * Risk page emergency row for DCA / Grid: pause every bot (no new cycles)
 * and close every open cycle at market (strategy_close_all). A bot on real
 * money has its Binance position closed by the mirror on the next pass.
 */
export function StrategyEmergency() {
  const { t } = useTranslation();
  const { strategy } = useDeskContext();
  const strategyLive = useStrategyLive().anyLive;
  const [dialog, setDialog] = useState<"pause" | "close" | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const bots = strategy.bots ?? [];
  const active = bots.filter(isActive);
  const open = bots.filter((b) => b.openCycle !== null);

  const run = async () => {
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      if (dialog === "pause") {
        const errs = (await Promise.all(active.map((b) => strategy.act(b.id, "stop")))).filter((e): e is string => !!e);
        if (errs.length) setError(strategyErrorText(t, errs[0]));
        else setResult(t("strategy.emergency.paused", { count: active.length }));
      } else {
        const r = await strategyCloseAll("manual");
        await strategy.refresh();
        setResult(t("strategy.emergency.closed", { closed: r.closed, unpriced: r.unpriced }));
      }
    } catch (e) {
      setError(strategyErrorText(t, errorMessage(e, "priceUnavailable")));
    } finally {
      setBusy(false);
      setDialog(null);
    }
  };

  return (
    <>
      <div className="ae-toolbar">
        <Button variant="secondary" size="sm" disabled={busy || active.length === 0} onClick={() => setDialog("pause")}>
          {t("strategy.emergency.pauseAll", { count: active.length })}
        </Button>
        <Button variant="secondary" size="sm" disabled={busy || open.length === 0} onClick={() => setDialog("close")}>
          {t("strategy.emergency.closeAll", { count: open.length })}
        </Button>
      </div>
      {result ? <p className="ae-muted">{result}</p> : null}
      {error ? <p className="ae-error">{error}</p> : null}
      <ConfirmDialog
        open={dialog !== null}
        title={t(dialog === "close" ? "strategy.emergency.closeAll" : "strategy.emergency.pauseAll", {
          count: dialog === "close" ? open.length : active.length,
        })}
        body={
          <>
            <p>{t(dialog === "close" ? "strategy.confirm.closeAll.body" : "strategy.confirm.stop.body")}</p>
            {dialog === "close" && strategyLive ? <p className="ae-error">{t("positions.closeAllPaper.strategyLiveNote")}</p> : null}
          </>
        }
        live={dialog === "close" && strategyLive}
        word={dialog === "close" ? "CLOSE" : undefined}
        confirmLabel={t(dialog === "close" ? "strategy.actions.close" : "strategy.actions.stop")}
        danger={dialog === "close"}
        busy={busy}
        onConfirm={() => void run()}
        onCancel={() => setDialog(null)}
      />
    </>
  );
}
