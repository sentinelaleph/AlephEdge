import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Button } from "@/components/ui/Button/Button";
import { Chip } from "@/components/ui/Chip/Chip";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog/ConfirmDialog";
import { Meter } from "@/components/ui/KpiTile/KpiTile";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { localeForLanguage } from "@/i18n";
import { formatNumber } from "@/lib/format";
import { strategyErrorText } from "@/lib/strategyText";

/**
 * The strategy budget cap (share of the declared balance all DCA/Grid budgets
 * may reserve) and the portfolio drawdown breaker, as Rust reports them.
 */
export function StrategyRiskPanel() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { strategy } = useDeskContext();
  const [confirm, setConfirm] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const r = strategy.risk;
  const usdt = (v: number, signed = false) =>
    `${formatNumber(v, locale, { minimumFractionDigits: 2, maximumFractionDigits: 2, signDisplay: signed ? "exceptZero" : "auto" })} USDT`;

  if (!r) {
    return (
      <Panel title={t("strategy.risk.title")}>
        <p className="ae-subtle">{t("workspace.loading")}</p>
      </Panel>
    );
  }

  const cap = (r.balance * r.budgetCapPct) / 100;
  const available = Math.max(0, cap - r.reservedBudget);
  const ddUsed = r.peakPnlQuote - r.pnlQuote;
  // The breaker measures drawdown from the P&L peak of the bots inside it
  // against their budgets (engine: breaker_check(breaker_pnl, breaker_budget)).
  const ddLimit = (r.breakerBudget * r.portfolioDdPct) / 100;

  return (
    <Panel
      title={t("strategy.risk.title")}
      aside={r.tripped ? <Chip tone="error">{t("strategy.risk.tripped")}</Chip> : null}
    >
      <FactList
        rows={[
          { label: t("strategy.risk.balance"), value: usdt(r.balance) },
          {
            label: t("strategy.risk.cap"),
            value: `${formatNumber(r.budgetCapPct, locale, { maximumFractionDigits: 1 })}% · ${usdt(cap)}`,
          },
          { label: t("strategy.risk.reserved"), value: usdt(r.reservedBudget) },
          { label: t("strategy.risk.available"), value: usdt(available) },
          { label: t("strategy.risk.maxLeverage"), value: `${r.maxLeverage}x` },
          { label: t("strategy.risk.dayPnl"), value: usdt(r.dayPnlQuote, true), tone: r.dayPnlQuote < 0 ? "down" : r.dayPnlQuote > 0 ? "up" : undefined },
          {
            label: t("strategy.risk.breaker"),
            value: `${formatNumber(r.portfolioDdPct, locale, { maximumFractionDigits: 1 })}% · ${usdt(ddLimit)}`,
          },
          { label: t("strategy.risk.covered"), value: t("strategy.risk.coveredValue", { covered: r.breakerBots, total: r.botCount }) },
          { label: t("strategy.risk.fromPeak"), value: usdt(-ddUsed, true), tone: ddUsed > 0 ? "down" : undefined },
        ]}
      />
      {ddLimit > 0 ? <Meter fraction={ddUsed / ddLimit} label={t("strategy.risk.breakerMeter")} /> : null}
      {r.tripped ? (
        <>
          <p className="ae-error">{t("strategy.notes.portfolioDdTripped")}</p>
          <Button variant="secondary" size="sm" onClick={() => setConfirm(true)}>
            {t("strategy.risk.rearm")}
          </Button>
        </>
      ) : null}
      {error ? <p className="ae-error">{error}</p> : null}
      <ConfirmDialog
        open={confirm}
        title={t("strategy.risk.rearm")}
        body={<p>{t("strategy.risk.rearmBody")}</p>}
        confirmLabel={t("strategy.risk.rearm")}
        onCancel={() => setConfirm(false)}
        onConfirm={() => {
          setConfirm(false);
          void strategy.rearmBreaker().then((e) => setError(e ? strategyErrorText(t, e) : null));
        }}
      />
    </Panel>
  );
}
