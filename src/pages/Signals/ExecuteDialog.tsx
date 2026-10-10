import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { Link, navigate } from "@/app/router/router";
import { withQuery } from "@/app/router/routes";
import { Button } from "@/components/ui/Button/Button";
import { localeForLanguage } from "@/i18n";
import { formatSignedPercent, NO_VALUE, pnlToneAttr } from "@/lib/format";
import { errorMessage } from "@/lib/ipc/bridge";
import type { Signal } from "@/lib/ipc/signal/signal";
import { strategyPresets, type Preset, type StrategyKind } from "@/lib/ipc/strategy/strategy";
import { presetName, strategyErrorText } from "@/lib/strategyText";
import { testSplit, useTemplateLauncher, VerdictChip } from "@/pages/Presets/presetUi";
import { signalBotsFor, TakeNow } from "./TakeNow";
import "./ExecuteDialog.css";

type SignalSide = Pick<Signal, "symbol" | "direction">;

/**
 * The templates that fit a signal: DCA on the signal's side; Grid long and
 * neutral for a long signal, short for a short one. A BTC-only template was
 * simulated on BTCUSDT alone and is offered for BTCUSDT only.
 */
export function executeTemplates(presets: Preset[], s: SignalSide): Record<StrategyKind, Preset[]> {
  const fits = (p: Preset) => p.universe.rule !== "btcOnly" || s.symbol === "BTCUSDT";
  const sideFits = (p: Preset) =>
    p.config.params.kind === "dca" ? p.config.side === s.direction : p.config.side === s.direction || (s.direction === "long" && p.config.side === "neutral");
  const ok = presets.filter((p) => fits(p) && sideFits(p));
  return { dca: ok.filter((p) => p.config.params.kind === "dca"), grid: ok.filter((p) => p.config.params.kind === "grid") };
}

export { signalBotsFor } from "./TakeNow";

/** The blank create form on the signal's pair. */
export function blankFormPath(kind: StrategyKind, symbol: string): string {
  return withQuery(`/bots/${kind}/new`, { symbol });
}

function TemplateList({ kind, rows, symbol, onOpen }: { kind: StrategyKind; rows: Preset[]; symbol: string; onOpen: (p: Preset) => void }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const title = t(`signalsPage.execute.${kind}Templates`);
  return (
    <section className="ae-exec__section" aria-label={title}>
      <h3 className="ae-section-title">{title}</h3>
      {rows.length === 0 ? (
        <p className="ae-subtle">{t("signalsPage.execute.noTemplates", { symbol })}</p>
      ) : (
        <ul className="ae-exec__list">
          {rows.map((p) => {
            const mean = testSplit(p)?.meanPerBotMonthPct;
            return (
              <li key={p.id} className="ae-exec__row" data-preset={p.id}>
                <span className="ae-exec__name">{presetName(t, p)}</span>
                <VerdictChip p={p} />
                <span className="ae-exec__mean tabular" data-tone={pnlToneAttr(mean)}>
                  <span className="ae-subtle">{t("strategy.preset.col.testMean")}</span> {mean === undefined ? NO_VALUE : formatSignedPercent(mean, locale)}
                </span>
                <Button variant="secondary" size="xs" className="ae-exec__open" onClick={() => onOpen(p)}>
                  {t("signalsPage.execute.openWith", { symbol })}
                </Button>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

/**
 * Execute on a signal. First: take this one signal now on a fitting signal
 * bot's paper book (TakeNow). Then the other ways to act on its pair: the
 * signal bots' settings (they take the feed by themselves), DCA and Grid
 * templates that fit the side (a failed one behind the same confirmation as
 * the Presets page), or a blank form. Real money is never opened here.
 */
export function ExecuteDialog({
  signal,
  onClose,
  pumpConfigured = false,
}: {
  signal: Signal | null;
  onClose: () => void;
  pumpConfigured?: boolean;
}) {
  const { t } = useTranslation();
  const titleId = useId();
  const boxRef = useRef<HTMLElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const returnTo = useRef<Element | null>(null);
  const closeCb = useRef(onClose);
  closeCb.current = onClose;
  const [presets, setPresets] = useState<Preset[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const open = signal !== null;
  const symbol = signal?.symbol ?? "";
  const { launch, dialog, holding } = useTemplateLauncher({ symbol, beforeGo: onClose });
  const holdingRef = useRef(holding);
  holdingRef.current = holding;

  useEffect(() => {
    if (!open || presets) return;
    let alive = true;
    setError(null);
    strategyPresets()
      .then((p) => alive && setPresets(p))
      .catch((e: unknown) => alive && setError(strategyErrorText(t, errorMessage(e, "storeReadFailed"))));
    return () => {
      alive = false;
    };
  }, [open, presets, t]);

  useEffect(() => {
    if (!open) return;
    returnTo.current = document.activeElement;
    requestAnimationFrame(() => closeRef.current?.focus());
    const onKey = (e: KeyboardEvent) => {
      // The failed-template confirmation on top owns the keyboard.
      if (holdingRef.current) return;
      if (e.key === "Escape") {
        e.stopPropagation();
        closeCb.current();
      } else if (e.key === "Tab" && boxRef.current) {
        const items = boxRef.current.querySelectorAll<HTMLElement>("a[href], button:not([disabled])");
        if (items.length === 0) return;
        const first = items[0];
        const last = items[items.length - 1];
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    };
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("keydown", onKey, true);
      (returnTo.current as HTMLElement | null)?.focus?.();
    };
  }, [open]);

  if (!signal) return null;
  const side = t(`signalDesk.direction.${signal.direction}`);
  const fit = presets ? executeTemplates(presets, signal) : null;
  const bots = signalBotsFor(signal);
  const openBlank = (kind: StrategyKind) => {
    onClose();
    navigate(blankFormPath(kind, symbol));
  };

  return createPortal(
    <div className="ae-dialog">
      <button type="button" className="ae-dialog__scrim" aria-label={t("signalsPage.execute.close")} tabIndex={-1} onClick={onClose} />
      <section ref={boxRef} className="ae-dialog__box ae-exec" role="dialog" aria-modal="true" aria-labelledby={titleId}>
        <header className="ae-exec__head">
          <h2 id={titleId} className="ae-dialog__title">
            {t("signalsPage.execute.title", { symbol, side })}
          </h2>
          <Button ref={closeRef} variant="ghost" size="xs" onClick={onClose}>
            {t("signalsPage.execute.close")}
          </Button>
        </header>

        <TakeNow signal={signal} pumpConfigured={pumpConfigured} onClose={onClose} />

        <section className="ae-exec__section" aria-label={t("signalsPage.execute.signalBot")}>
          <h3 className="ae-section-title">{t("signalsPage.execute.signalBot")}</h3>
          <p className="ae-subtle">{t("signalsPage.execute.signalBotLine")}</p>
          <p className="ae-exec__links">
            <Link to="/bots/signal" className="ae-link" onClick={onClose}>
              {t("nav.signalBots")}
            </Link>
            {bots.map((k) => (
              <Link key={k} to={withQuery("/bots/signal", { bot: k === "futures" ? null : k })} className="ae-link" onClick={onClose}>
                {t("signalsPage.execute.botSettings", { bot: t(`bots.kind.${k}`) })}
              </Link>
            ))}
          </p>
        </section>

        {error ? (
          <p className="ae-banner" data-tone="danger" role="alert">
            {error}
          </p>
        ) : !fit ? (
          <p className="ae-subtle">{t("workspace.loading")}</p>
        ) : (
          <>
            <TemplateList kind="dca" rows={fit.dca} symbol={symbol} onOpen={launch} />
            <TemplateList kind="grid" rows={fit.grid} symbol={symbol} onOpen={launch} />
          </>
        )}

        <section className="ae-exec__section" aria-label={t("signalsPage.execute.blank")}>
          <h3 className="ae-section-title">{t("signalsPage.execute.blank")}</h3>
          <div className="ae-exec__blank">
            <Button variant="secondary" size="xs" onClick={() => openBlank("dca")}>
              {t("signalsPage.execute.blankDca", { symbol })}
            </Button>
            <Button variant="secondary" size="xs" onClick={() => openBlank("grid")}>
              {t("signalsPage.execute.blankGrid", { symbol })}
            </Button>
          </div>
        </section>

        <p className="ae-subtle ae-exec__note">{t("signalsPage.execute.note", { symbol })}</p>
      </section>
      {dialog}
    </div>,
    document.body,
  );
}
