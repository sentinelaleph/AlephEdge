import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Link } from "@/app/router/router";
import { Button } from "@/components/ui/Button/Button";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog/ConfirmDialog";
import { errorMessage } from "@/lib/ipc/bridge";
import { liveStopAll } from "@/lib/ipc/bot/bot";
import { liveBots, liveChipTooltipKey, liveRunningCount } from "@/lib/liveMode";
import { useStrategyLive } from "@/pages/Bots/strategy/useStrategyLive";
import "./LiveMode.css";

/**
 * The header's mode readout: how many Live bots are running, and the one
 * global "Stop all live". Every bot is Paper unless the user made it Live,
 * so the chip counts what trades real money now, never a build.
 */
export function LiveRunning() {
  const { t } = useTranslation();
  const { desk, strategy } = useDeskContext();
  const { views, refresh } = useStrategyLive();
  const [open, setOpen] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const box = useRef<HTMLDivElement>(null);
  const lines = liveBots(desk.status, views, strategy.bots);
  const running = liveRunningCount(lines);
  const holdsReal = desk.status.openPositions.some((p) => p.live) || views.some((v) => v.realQty !== 0);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (box.current && !box.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  if (!desk.loaded) return null;

  const stopAll = async () => {
    setBusy(true);
    setResult(null);
    try {
      const r = await liveStopAll();
      setResult(t("liveMode.header.stopped", { count: r.signalStopped + r.strategyStopped }));
    } catch (e) {
      setResult(errorMessage(e, t("bots.actionFailed")));
    } finally {
      await Promise.all([desk.refresh(), refresh(), strategy.refresh()]).catch(() => undefined);
      setBusy(false);
      setConfirm(false);
    }
  };

  return (
    <div className="ae-liverun" ref={box}>
      <button
        type="button"
        className="ae-header__scope"
        data-live={running > 0 || undefined}
        data-scope={running > 0 ? "live" : lines.length > 0 ? "liveIdle" : "paper"}
        aria-expanded={open}
        aria-haspopup="dialog"
        title={t(liveChipTooltipKey(running))}
        onClick={() => setOpen((o) => !o)}
      >
        {t("liveMode.header.chip", { count: running })}
      </button>
      {open ? (
        <div className="ae-liverun__menu" role="dialog" aria-label={t("liveMode.header.title")}>
          {lines.length === 0 ? (
            <p className="ae-subtle">{t("liveMode.header.none")}</p>
          ) : (
            <ul className="ae-liverun__list">
              {lines.map((l) => (
                <li key={l.id}>
                  <span>{l.nameKey ? t(l.nameKey) : l.name}</span>
                  <span className="ae-liverun__state" data-running={l.running || undefined}>
                    {l.running ? t("liveMode.header.running") : l.needsArm ? t("liveMode.header.needsArm") : t("liveMode.header.stoppedState")}
                  </span>
                </li>
              ))}
            </ul>
          )}
          {holdsReal ? <p className="ae-subtle">{t("liveMode.header.positionsStay")}</p> : null}
          {result ? (
            <p className="ae-subtle" role="status">
              {result}
            </p>
          ) : null}
          <div className="ae-liverun__foot">
            <Link to="/guide?s=real-money" className="ae-link" onClick={() => setOpen(false)}>
              {t("botCreate.mode.guide")}
            </Link>
            <Button variant="danger" size="xs" disabled={busy || lines.length === 0} onClick={() => setConfirm(true)}>
              {t("liveMode.header.stopAll")}
            </Button>
          </div>
        </div>
      ) : null}
      <ConfirmDialog
        open={confirm}
        live
        danger
        title={t("liveMode.header.stopAll")}
        body={<p>{t("liveMode.header.stopAllBody")}</p>}
        confirmLabel={t("liveMode.header.stopAll")}
        busy={busy}
        onConfirm={() => void stopAll()}
        onCancel={() => setConfirm(false)}
      />
    </div>
  );
}
