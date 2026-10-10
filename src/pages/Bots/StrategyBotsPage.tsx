import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { navigate } from "@/app/router/router";
import { Button } from "@/components/ui/Button/Button";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { localeForLanguage } from "@/i18n";
import { formatSignedUsdt, formatUsdt, NO_VALUE, pnlToneAttr } from "@/lib/format";
import { isActive, strategyExportCsv, type StrategyKind } from "@/lib/ipc/strategy/strategy";
import { strategyErrorText } from "@/lib/strategyText";
import { SkeletonRows } from "./SkeletonRows";
import { StrategyBotsTable } from "./strategy/StrategyBotsTable";
import { StrategyRiskPanel } from "./strategy/StrategyRiskPanel";
import { useStrategyActions } from "./strategy/useStrategyActions";
import { MarketTrendPanel } from "@/components/Desk/MarketTrend";
import { modeKey } from "./BotNewPage";

/**
 * #/bots/dca and #/bots/grid. PAPER ONLY. Rows come from Rust's
 * strategy_list; every number is the engine's own accounting.
 */
export function StrategyBotsPage({ kind }: { kind: StrategyKind }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { strategy, desk } = useDeskContext();
  const liveBuild = desk.loaded && desk.status.liveTradingEnabled;
  const actions = useStrategyActions();
  const [exported, setExported] = useState<string | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);
  const newPath = `/bots/${kind}/new`;
  const rows = strategy.bots?.filter((b) => b.kind === kind) ?? null;

  const exportCsv = async () => {
    try {
      const p = await strategyExportCsv();
      setExported(`${p.cyclesPath} · ${p.fillsPath}`);
      setExportError(null);
    } catch (e) {
      setExportError(strategyErrorText(t, String(e)));
    }
  };

  const total = rows ? rows.reduce((s, b) => s + (b.equity - b.budget), 0) : 0;
  const right = (
    <>
      <MarketTrendPanel />
      <Panel title={t(`botsList.${kind}.facts`)}>
        <FactList
          rows={[
            { label: t("botsList.totals.bots"), value: rows ? rows.length : NO_VALUE },
            { label: t("botsList.totals.running"), value: rows ? rows.filter(isActive).length : NO_VALUE },
            { label: t("strategy.list.openCycles"), value: rows ? rows.filter((b) => b.openCycle).length : NO_VALUE },
            { label: t("strategy.list.budgetTotal"), value: rows ? formatUsdt(rows.reduce((s, b) => s + b.budget, 0), locale) : NO_VALUE },
            {
              label: t("strategy.col.totalPnl"),
              value: rows ? formatSignedUsdt(total, locale) : NO_VALUE,
              tone: rows ? pnlToneAttr(total) : undefined,
            },
            liveBuild
              ? { label: t("botsList.liveOrders"), value: t(modeKey(true)) }
              : { label: t("table.mode"), value: t(modeKey(false)) },
          ]}
        />
      </Panel>
      <StrategyRiskPanel />
    </>
  );

  return (
    <PageShell
      title={t(kind === "dca" ? "nav.dcaBots" : "nav.gridBots")}
      crumbs={[{ label: t("nav.groups.bots"), to: "/bots" }]}
      secondary={
        <>
          <Button variant="secondary" size="sm" onClick={() => navigate(`/presets?type=${kind}`)}>
            {t(`botsList.${kind}.presets`)}
          </Button>
          {rows && rows.length > 0 ? (
            <Button variant="secondary" size="sm" onClick={() => void exportCsv()}>
              {t("pnl.exportCsv")}
            </Button>
          ) : null}
        </>
      }
      primary={
        <Button size="sm" onClick={() => navigate(newPath)}>
          {t(`botsList.${kind}.new`)}
        </Button>
      }
      right={right}
    >
      {strategy.loadError ? (
        <p className="ae-banner" data-tone="danger" role="alert">
          {strategyErrorText(t, strategy.loadError)}
        </p>
      ) : null}
      {rows === null ? (
        strategy.loadError ? null : <SkeletonRows rows={4} />
      ) : rows.length === 0 ? (
        <EmptyState
          title={t(`botsList.${kind}.empty`)}
          detail={t(liveBuild ? "strategy.live.listLine" : modeKey(false))}
          actions={
            <Button variant="secondary" size="sm" onClick={() => navigate(`/presets?type=${kind}`)}>
              {t(`botsList.${kind}.presets`)}
            </Button>
          }
        />
      ) : (
        <>
          {exported ? <p className="ae-muted">{t("pnl.exportedTo", { path: exported })}</p> : null}
          {exportError ? <p className="ae-error">{exportError}</p> : null}
          <StrategyBotsTable rows={rows} actions={actions} />
        </>
      )}
      {actions.dialog}
    </PageShell>
  );
}
