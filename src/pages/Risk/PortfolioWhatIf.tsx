import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "@/app/router/router";
import { Chip } from "@/components/ui/Chip/Chip";
import { FactList, Panel, type Fact } from "@/components/ui/Panel/Panel";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPnl, NO_VALUE } from "@/lib/format";
import type { TradePreview } from "@/lib/ipc/bot/bot";
import { errorMessage } from "@/lib/ipc/bridge";
import type { PreviewDto, StrategyConfig } from "@/lib/ipc/strategy/strategy";
import { riskviewWhatIf, type WhatIf, type WhatIfBrief, type WhatIfCandidate } from "@/lib/ipc/riskview/riskview";
import { riskviewErrorText } from "@/lib/riskViewText";

/** The New trade preview as a what-if candidate. */
export function tradeCandidate(p: TradePreview): WhatIfCandidate {
  return {
    type: "trade",
    trade: {
      botKind: p.botKind,
      symbol: p.symbol,
      direction: p.direction,
      entry: p.entry,
      takeProfit: p.takeProfit,
      stopLoss: p.stopLoss,
      capital: p.capital,
      notionalUsdt: p.notionalUsdt,
      effectiveLeverage: p.effectiveLeverage,
      leverage: p.leverage,
    },
  };
}

/** A DCA / Grid config at its preview's reference price, or null without a preview. */
export function strategyCandidate(config: StrategyConfig, preview: PreviewDto | null): WhatIfCandidate | null {
  return preview && preview.error === null && preview.referencePrice > 0 ? { type: "strategy", config, price: preview.referencePrice } : null;
}

/** After the form's own preview settles: one more pause before asking Rust. */
const DEBOUNCE_MS = 400;

/**
 * "Portfolio stress at BTC -20%: before X -> after Y" for a DCA / Grid bot
 * or a New trade about to start (riskview_whatif). The candidate joins the
 * paper book with its worst case walked like every open exposure. Null
 * candidate (no preview yet): nothing is asked.
 */
export function PortfolioWhatIf({ candidate }: { candidate: WhatIfCandidate | null }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const [result, setResult] = useState<WhatIf | null>(null);
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);
  const key = candidate ? JSON.stringify(candidate) : null;

  useEffect(() => {
    if (!candidate) {
      setResult(null);
      return;
    }
    const id = ++seq.current;
    const timer = window.setTimeout(() => {
      riskviewWhatIf(candidate)
        .then((r) => {
          if (id !== seq.current) return;
          setResult(r);
          setError(null);
        })
        .catch((e: unknown) => {
          if (id !== seq.current) return;
          setResult(null);
          setError(errorMessage(e, t("riskview.failed")));
        });
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
    // `key` stands for the candidate's content: a new object with the same
    // settings must not ask again.
  }, [key]);

  if (!candidate) return null;
  const brief = (b: WhatIfBrief | null) => {
    if (!b) return NO_VALUE;
    if (b.included === 0) return t("riskview.whatif.emptyBook");
    const pct = b.pctOfCapital === null ? NO_VALUE : formatPnl(b.pctOfCapital, locale, { unit: "percent", digits: 1 }).text;
    return `${formatPnl(b.pnl, locale).text} · ${pct}`;
  };
  const rows: Fact[] = result
    ? [
        {
          label: t(candidate.type === "trade" ? "riskview.whatif.candidateTrade" : "riskview.whatif.candidate"),
          value: result.candidatePnl === null ? NO_VALUE : formatPnl(result.candidatePnl, locale).text,
          tone: result.candidatePnl !== null && result.candidatePnl < 0 ? "down" : undefined,
        },
        {
          label: t("riskview.col.beta"),
          value: result.candidateBeta ? formatNumber(result.candidateBeta.beta, locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 }) : NO_VALUE,
        },
      ]
    : [];
  const excluded = (result?.after ?? result?.before)?.excluded ?? 0;
  return (
    <Panel title={t("riskview.whatif.title")} aside={result?.sample ? <Chip tone="accent">{t("riskview.sample")}</Chip> : null}>
      {error ? <p className="ae-error">{error}</p> : null}
      {!result && !error ? <p className="ae-subtle">{t("riskview.loading")}</p> : null}
      {result ? (
        <>
          <p className="ae-muted">
            {t("riskview.whatif.line", { before: brief(result.before), after: result.after ? brief(result.after) : t("riskview.level.unavailable") })}
          </p>
          <FactList rows={rows} />
          {result.error ? <p className="ae-subtle">{riskviewErrorText(t, result.error)}</p> : null}
          {excluded > 0 ? <p className="ae-subtle">{t("riskview.excludedNote", { count: excluded })}</p> : null}
          <Link to="/risk?s=portfolio" className="ae-link ae-muted">
            {t("safety.section.portfolio")}
          </Link>
        </>
      ) : null}
    </Panel>
  );
}
