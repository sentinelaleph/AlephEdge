import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPrice, formatTableTime, NO_VALUE, pnlToneAttr } from "@/lib/format";
import { strategyLiveReport, type StrategyDiffReport, type StrategyDiffRow } from "@/lib/ipc/strategy/strategy";

const REFRESH_MS = 10_000;

/**
 * Real against simulated fills of one live DCA / Grid bot: what reaching
 * Binance cost compared with the simulation that decided the trade
 * (bot/strategy_live.rs diff_report). Slippage is positive when the real
 * fill was worse: paid more on a buy, received less on a sell.
 */
export function StrategyLiveReport({ botId }: { botId: string }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const [report, setReport] = useState<StrategyDiffReport | null>(null);

  useEffect(() => {
    let alive = true;
    const load = () =>
      strategyLiveReport(botId)
        .then((r) => alive && setReport(r))
        .catch(() => undefined);
    void load();
    const timer = window.setInterval(() => void load(), REFRESH_MS);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [botId]);

  const bps = (v: number | null) => (v === null ? NO_VALUE : `${v >= 0 ? "+" : ""}${formatNumber(v, locale, { maximumFractionDigits: 1 })} bps`);
  const usdt = (v: number | null) => (v === null ? NO_VALUE : `${v >= 0 ? "+" : ""}${formatNumber(v, locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 })} USDT`);
  const secs = (v: number | null) => (v === null ? NO_VALUE : `${formatNumber(v, locale, { maximumFractionDigits: 0 })} s`);

  const columns: DataColumn<StrategyDiffRow>[] = [
    { id: "time", header: t("table.time"), width: "1%", cell: (r) => formatTableTime(r.ts, locale) },
    { id: "cycle", header: t("strategy.report.cycle"), numeric: true, priority: 3, cell: (r) => `#${r.seq}` },
    { id: "side", header: t("table.side"), cell: (r) => t(`strategy.report.side.${r.side}`) },
    { id: "kind", header: t("strategy.report.kind"), priority: 2, cell: (r) => t(`strategy.report.kinds.${r.kind}`, { defaultValue: r.kind }) },
    { id: "qty", header: t("strategy.report.qty"), numeric: true, cell: (r) => formatNumber(r.realQty, locale, { maximumFractionDigits: 6 }) },
    { id: "real", header: t("strategy.report.real"), numeric: true, cell: (r) => formatPrice(r.realPrice, locale) },
    { id: "sim", header: t("strategy.report.sim"), numeric: true, cell: (r) => (r.simPrice === null ? NO_VALUE : formatPrice(r.simPrice, locale)) },
    // A positive slippage is a cost: shown with the "down" tone.
    { id: "bps", header: t("strategy.report.slippage"), numeric: true, keep: true, cell: (r) => bps(r.slippageBps), tone: (r) => pnlToneAttr(r.slippageBps === null ? 0 : -r.slippageBps) },
    { id: "usdt", header: t("strategy.report.cost"), numeric: true, priority: 2, cell: (r) => usdt(r.slippageUsdt) },
    { id: "delay", header: t("strategy.report.delay"), numeric: true, priority: 3, cell: (r) => secs(r.delayS) },
  ];

  if (!report) return <p className="ae-subtle">{t("workspace.loading")}</p>;
  const s = report.summary;
  return (
    <>
      <Panel title={t("strategy.report.title")}>
        <FactList
          rows={[
            { label: t("strategy.report.avgSlippage"), value: bps(s.avgSlippageBps), tone: s.avgSlippageBps !== null && s.avgSlippageBps > 0 ? "down" : undefined },
            { label: t("strategy.report.totalCost"), value: usdt(s.slippageUsdt) },
            { label: t("strategy.report.fees"), value: usdt(-s.feesEstUsdt) },
            { label: t("strategy.report.avgDelay"), value: secs(s.avgDelayS) },
            { label: t("strategy.report.matched"), value: `${s.matched} / ${s.matched + s.unmatched}` },
          ]}
        />
        <p className="ae-subtle">{t("strategy.report.note")}</p>
      </Panel>
      {report.rows.length === 0 ? (
        <EmptyState title={t("strategy.report.empty")} />
      ) : (
        <DataTable label={t("strategy.report.title")} rows={report.rows} columns={columns} rowKey={(r) => `${r.seq}-${r.ts}-${r.kind}-${r.realQty}`} compact />
      )}
    </>
  );
}
