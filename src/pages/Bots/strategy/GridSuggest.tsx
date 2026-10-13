import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button/Button";
import { FactList, type Fact } from "@/components/ui/Panel/Panel";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPrice } from "@/lib/format";
import { errorMessage } from "@/lib/ipc/bridge";
import { gridSuggest, type BacktestBrief, type GridSuggestDto, type GridSuggestion } from "@/lib/ipc/strategy/backtest";
import type { StrategyConfig } from "@/lib/ipc/strategy/strategy";
import { backtestErrorText } from "@/lib/strategyText";

/** The form's config with the suggested range, grid count and geometric spacing. */
export function applySuggestion(cfg: StrategyConfig, s: GridSuggestion): StrategyConfig {
  if (cfg.params.kind !== "grid") return cfg;
  return {
    ...cfg,
    params: { ...cfg.params, range: { type: "relative", lowerPct: s.lowerPct, upperPct: s.upperPct }, nGrids: s.nGrids, spacing: "geom" },
  };
}

/** A result describes the config it was asked for, or the one its Apply produced. */
export function suggestionFits(cfg: StrategyConfig, asked: string, applied: string): boolean {
  const key = JSON.stringify(cfg);
  return key === asked || key === applied;
}

interface Held {
  dto: GridSuggestDto;
  asked: string;
  applied: string;
}

/**
 * "Suggest from volatility": Rust reads 30 days of hourly DataHub candles,
 * proposes a range (14-day low / high, at least 1.5 daily ATRs a side) and a
 * grid count (step about one hourly ATR, never under the fee floor), and
 * replays the current and the suggested settings over those 30 days. Nothing
 * is applied until Apply.
 */
export function GridSuggest({ cfg, disabled, onApply }: { cfg: StrategyConfig; disabled: boolean; onApply: (next: StrategyConfig) => void }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const [busy, setBusy] = useState(false);
  const [held, setHeld] = useState<Held | null>(null);
  const [error, setError] = useState<string | null>(null);
  const pct = (v: number, digits = 2) => `${formatNumber(v, locale, { maximumFractionDigits: digits })}%`;

  const ask = () => {
    const asked = JSON.stringify(cfg);
    setBusy(true);
    setError(null);
    gridSuggest(cfg)
      .then((dto) => setHeld({ dto, asked, applied: JSON.stringify(applySuggestion(cfg, dto.suggestion)) }))
      .catch((e: unknown) => {
        setHeld(null);
        setError(errorMessage(e, "historyNotAvailable"));
      })
      .finally(() => setBusy(false));
  };

  const shown = held && suggestionFits(cfg, held.asked, held.applied) ? held : null;
  const applied = shown !== null && JSON.stringify(cfg) === shown.applied;
  const s = shown?.dto.suggestion;

  const brief = (b: BacktestBrief | null, err?: string | null): string =>
    b
      ? t("strategy.suggest.briefValue", {
          dd: pct(b.maxDrawdownPct),
          pnl: `${b.totalPnlPct >= 0 ? "+" : ""}${pct(b.totalPnlPct)}`,
          fills: b.gridClosingFills ?? 0,
        }) + (b.liquidations > 0 ? ` · ${t("strategy.suggest.liquidated", { n: b.liquidations })}` : "")
      : err
        ? backtestErrorText(t, err)
        : t("strategy.suggest.notValid");

  const facts: Fact[] = s
    ? [
        {
          label: t("strategy.suggest.range"),
          value: `−${pct(s.lowerPct, 1)} / +${pct(s.upperPct, 1)} · ${formatPrice(s.price * (1 - s.lowerPct / 100), locale)} – ${formatPrice(s.price * (1 + s.upperPct / 100), locale)}`,
          info: t("strategy.suggest.rangeInfo", { low: formatPrice(s.windowLow, locale), high: formatPrice(s.windowHigh, locale), atr: pct(s.atrDayPct) }),
        },
        {
          label: t("strategy.suggest.grids"),
          value: t("strategy.suggest.gridsValue", { n: s.nGrids, step: pct(s.stepPct, 3), net: pct(s.netPerGridPct, 3) }),
          info: t("strategy.suggest.gridsInfo", { atr: pct(s.atrHourPct, 3) }),
        },
        { label: t("strategy.suggest.current"), value: brief(shown.dto.current), tone: shown.dto.current ? undefined : "muted" },
        {
          label: t("strategy.suggest.suggested"),
          value: brief(shown.dto.suggested, shown.dto.suggestedError),
          tone: shown.dto.suggested ? undefined : "warn",
        },
      ]
    : [];

  return (
    <div className="ae-sform__suggest" data-testid="grid-suggest">
      <div className="ae-sform__suggest-head">
        <Button variant="secondary" size="sm" onClick={ask} disabled={disabled || busy} disabledReason={busy ? t("strategy.suggest.busy") : undefined}>
          {busy ? t("strategy.suggest.busy") : t("strategy.suggest.button")}
        </Button>
        <span className="ae-subtle">{t("strategy.suggest.note")}</span>
      </div>
      {error ? (
        <p className="ae-error" role="alert">
          {backtestErrorText(t, error)}
        </p>
      ) : null}
      {s ? (
        <>
          <FactList rows={facts} />
          <p className="ae-subtle">
            {t("strategy.suggest.window", {
              from: new Date(shown.dto.startMs).toISOString().slice(0, 10),
              to: new Date(shown.dto.endMs).toISOString().slice(0, 10),
              bars: shown.dto.bars,
              source: shown.dto.source || "DataHub",
            })}
          </p>
          <div className="ae-sform__suggest-head">
            <Button
              variant="primary"
              size="sm"
              disabled={disabled || applied}
              disabledReason={applied ? t("strategy.suggest.applied") : undefined}
              onClick={() => onApply(applySuggestion(cfg, s))}
            >
              {applied ? t("strategy.suggest.applied") : t("strategy.suggest.apply")}
            </Button>
          </div>
        </>
      ) : null}
    </div>
  );
}
