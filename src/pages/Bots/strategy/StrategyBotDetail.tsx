import { useCallback, useEffect, useState } from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { navigate, useQueryParam } from "@/app/router/router";
import { Button } from "@/components/ui/Button/Button";
import { Chip } from "@/components/ui/Chip/Chip";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { KpiGrid } from "@/components/ui/KpiGrid/KpiGrid";
import { KpiTile } from "@/components/ui/KpiTile/KpiTile";
import { StatusChip } from "@/components/ui/StatusChip/StatusChip";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel, type Fact } from "@/components/ui/Panel/Panel";
import { Tabs } from "@/components/ui/Tabs/Tabs";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPnl, formatPrice, formatSignedPercent, formatTableTime, formatUsdt, NO_VALUE, pnlToneAttr } from "@/lib/format";
import { errorMessage } from "@/lib/ipc/bridge";
import {
  isActive,
  strategyCycles,
  strategyDetail,
  strategyEquity,
  strategyFills,
  strategyOrders,
  strategyPresets,
  strategyStats,
  strategyUpdate,
  type CycleRow,
  type EquityRow,
  type FillRow,
  type OrderRole,
  type OrderRow,
  type Preset,
  type StrategyNote,
  type StrategyBotView,
  type StrategyConfig,
  type StrategyDetail,
  type StrategyStats,
} from "@/lib/ipc/strategy/strategy";
import { cycleNetQuote, runActionKey, runStateText, sideText, strategyErrorText, strategyNoteText } from "@/lib/strategyText";
import { runStatusKind, totalPnlPct } from "./StrategyBotsTable";
import { EquityChart } from "./EquityChart";
import { StrategyForm, type FormPreview } from "./StrategyForm";
import { StrategyPreviewPanel } from "./StrategyPreviewPanel";
import { StrategyLivePanel } from "./StrategyLivePanel";
import { StrategyLiveReport } from "./StrategyLiveReport";
import { StrategyRiskPanel } from "./StrategyRiskPanel";
import { useStrategyActions } from "./useStrategyActions";
import { useStrategyLive } from "./useStrategyLive";
import "../bots.css";

const TABS = ["overview", "cycles", "orders", "fills", "real", "settings", "log"] as const;
type Tab = (typeof TABS)[number];

export function roleText(t: TFunction, r: OrderRole): string {
  return "index" in r ? t(`strategy.role.${r.role}`, { n: r.index }) : t(`strategy.role.${r.role}`);
}

/** #/bots/sb_… — one DCA or Grid bot (paper). */
export function StrategyBotDetail({ view }: { view: StrategyBotView }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { strategy } = useDeskContext();
  const { liveIds, viewFor } = useStrategyLive();
  const hasReal = liveIds.has(view.id) || (viewFor(view.id)?.fills ?? 0) > 0;
  const [tabParam, setTab] = useQueryParam("tab");
  const [errParam, setErr] = useQueryParam("err");
  const tab: Tab = (TABS as readonly string[]).includes(tabParam ?? "") ? (tabParam as Tab) : "overview";
  const [detail, setDetail] = useState<StrategyDetail | null>(null);
  const [stats, setStats] = useState<StrategyStats | null>(null);
  const [equity, setEquity] = useState<EquityRow[] | null>(null);
  const [cycles, setCycles] = useState<CycleRow[] | null>(null);
  const [orders, setOrders] = useState<OrderRow[] | null>(null);
  const [fills, setFills] = useState<FillRow[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [summary, setSummary] = useState<FormPreview | null>(null);
  const [saved, setSaved] = useState(false);
  // The templates, so an edit of a preset bot runs the parity check (the
  // link is dropped once a simulated setting changes).
  const [presets, setPresets] = useState<Preset[] | null>(null);
  useEffect(() => {
    let alive = true;
    strategyPresets()
      .then((p) => alive && setPresets(p))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);
  const actions = useStrategyActions((action) => {
    if (action === "archive") navigate(`/bots/${view.kind}`);
  });
  const id = view.id;
  const listPath = `/bots/${view.kind}`;
  const usdt = (v: number, signed = false) => (signed ? formatPnl(v, locale).text : formatUsdt(v, locale));
  const when = (ms: number | null) => (ms ? formatTableTime(ms, locale) : NO_VALUE);

  const load = useCallback(async () => {
    try {
      const [d, s] = await Promise.all([strategyDetail(id), strategyStats("bot", id)]);
      setDetail(d);
      setStats(s);
      if (tab === "overview") setEquity(await strategyEquity(id));
      else if (tab === "cycles") setCycles(await strategyCycles(id, 500));
      else if (tab === "orders") setOrders(await strategyOrders(id));
      else if (tab === "fills") setFills(await strategyFills(id, 500));
      setLoadError(null);
    } catch (e) {
      setLoadError(strategyErrorText(t, errorMessage(e, "storeReadFailed")));
    }
  }, [id, tab, t]);

  useEffect(() => {
    void load();
    const timer = window.setInterval(() => void load(), 10_000);
    return () => window.clearInterval(timer);
  }, [load]);

  const onSave = useCallback(
    async (cfg: StrategyConfig): Promise<string | null> => {
      try {
        await strategyUpdate(id, cfg);
        await strategy.refresh();
        setDetail(await strategyDetail(id));
        setSaved(true);
        return null;
      } catch (e) {
        return errorMessage(e, "botUnknown");
      }
    },
    [id, strategy],
  );

  const onDirty = useCallback((d: boolean) => {
    if (d) setSaved(false);
  }, []);

  const busy = strategy.busyId === id;
  const active = isActive(view);
  const total = totalPnlPct(view);
  const totalQuote = view.equity - view.budget;
  const c = view.openCycle;

  // One primary per view: Start when the bot can start; everything else secondary.
  const primary =
    view.runState === "dead" || active ? null : (
      <Button size="sm" disabled={busy} onClick={() => actions.request(view, "start")}>
        {t(runActionKey(view))}
      </Button>
    );
  const secondary = (
    <>
      {view.runState !== "dead" && active ? (
        <Button variant="secondary" size="sm" disabled={busy} onClick={() => actions.request(view, "stop")}>
          {t("strategy.actions.stop")}
        </Button>
      ) : null}
      {c ? (
        <Button variant="secondary" size="sm" disabled={busy} onClick={() => actions.request(view, "close")}>
          {t("strategy.actions.close")}
        </Button>
      ) : null}
      <Button variant="secondary" size="sm" onClick={() => navigate(`${listPath}/new?from=${id}`)}>
        {t("strategy.actions.clone")}
      </Button>
      <Button variant="secondary" size="sm" disabled={busy || liveIds.has(view.id)} onClick={() => actions.request(view, "archive")}>
        {t("strategy.actions.archive")}
      </Button>
      {liveIds.has(view.id) ? <span className="ae-subtle">{t("strategy.errors.liveEditLocked")}</span> : null}
    </>
  );

  const facts: Fact[] = [
    { label: t("table.type"), value: t(`botsList.type.${view.kind}`) },
    { label: t("table.market"), value: t(`filters.market.${view.market}`) },
    { label: t("table.symbol"), value: view.symbol },
    { label: t("table.side"), value: sideText(t, view.side) },
    ...(view.market === "futures" ? [{ label: t("table.leverage"), value: `${view.leverage}x` }] : []),
    { label: t("strategy.field.budget"), value: usdt(view.budget) },
    { label: t("strategy.field.priceSource"), value: "Binance" },
    { label: t("strategy.detail.preset"), value: view.presetId ?? NO_VALUE },
    {
      label: t("strategy.risk.breaker"),
      value: detail ? t(detail.config.portfolioBreaker ? "strategy.preset.on" : "strategy.preset.off") : NO_VALUE,
    },
    { label: t("strategy.detail.created"), value: when(view.createdAt) },
    { label: t("strategy.detail.markPrice"), value: view.markPrice !== null ? formatPrice(view.markPrice, locale) : t("strategy.detail.noMark") },
    { label: t("strategy.detail.markTime"), value: when(view.markTs) },
  ];
  const ownNotes = strategy.notes.filter((n) => n.botId === id || n.botId === "").slice().reverse();

  const cycleColumns: DataColumn<CycleRow>[] = [
    { id: "seq", header: "#", numeric: true, width: "1%", cell: (r) => r.seq },
    { id: "opened", header: t("strategy.detail.opened"), priority: 2, cell: (r) => when(r.openedAt) },
    { id: "closed", header: t("table.closedAt"), cell: (r) => (r.closedAt ? when(r.closedAt) : t("strategy.detail.open")) },
    {
      id: "exit",
      header: t("table.exitReason"),
      cell: (r) => (r.exitReason ? t(`strategy.exit.${r.exitReason}`, { defaultValue: r.exitReason }) : NO_VALUE),
    },
    { id: "anchor", header: t("strategy.detail.anchor"), numeric: true, priority: 3, cell: (r) => formatPrice(r.anchorPrice, locale) },
    {
      id: "avg",
      header: t("strategy.preview.avgEntry"),
      numeric: true,
      priority: 2,
      cell: (r) => (r.avgEntry !== null ? formatPrice(r.avgEntry, locale) : NO_VALUE),
    },
    {
      id: "fills",
      header: t(view.kind === "dca" ? "strategy.detail.soFilled" : "strategy.detail.closingFills"),
      numeric: true,
      priority: 3,
      cell: (r) => (view.kind === "dca" ? r.soFilled : r.gridClosingFills) ?? NO_VALUE,
    },
    {
      id: "fees",
      header: t("strategy.detail.fees"),
      numeric: true,
      priority: 3,
      cell: (r) => formatNumber(r.feesQuote, locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 }),
    },
    {
      id: "funding",
      header: t("strategy.detail.funding"),
      numeric: true,
      priority: 3,
      tone: (r) => (r.fundingUnknown ? "muted" : undefined),
      cell: (r) => (
        <span title={r.fundingUnknown ? t("strategy.notes.fundingUnknown") : undefined}>
          {formatNumber(r.fundingQuote, locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
          {r.fundingUnknown ? "*" : ""}
        </span>
      ),
    },
    {
      id: "adverse",
      header: t("strategy.detail.maxAdverse"),
      numeric: true,
      priority: 3,
      cell: (r) => (r.maxAdversePct !== null ? formatSignedPercent(r.maxAdversePct, locale) : NO_VALUE),
    },
    {
      id: "pnlPct",
      header: t("table.pnlPct"),
      numeric: true,
      cell: (r) => (r.pnlPctBudget !== null ? formatSignedPercent(r.pnlPctBudget, locale, 3) : NO_VALUE),
      tone: (r) => pnlToneAttr(r.pnlPctBudget, 3),
    },
    {
      // The cycle's own money, never the stored % times today's budget.
      id: "pnlUsdt",
      header: t("table.pnlUsdt"),
      numeric: true,
      priority: 2,
      cell: (r) => {
        const v = cycleNetQuote(r);
        return v !== null ? formatPnl(v, locale, { unit: "none" }).text : NO_VALUE;
      },
      tone: (r) => {
        const v = cycleNetQuote(r);
        return v !== null ? pnlToneAttr(v) : undefined;
      },
    },
  ];

  const orderColumns: DataColumn<OrderRow>[] = [
    { id: "seq", header: "#", numeric: true, width: "1%", cell: (o) => o.seq },
    { id: "role", header: t("strategy.detail.role"), cell: (o) => roleText(t, o.order.role) },
    { id: "side", header: t("table.side"), cell: (o) => t(`strategy.orderSide.${o.order.side}`) },
    { id: "type", header: t("table.orderType"), priority: 2, cell: (o) => t(`strategy.orderType.${o.order.kind}`) },
    { id: "price", header: t("table.price"), numeric: true, cell: (o) => formatPrice(o.order.price, locale) },
    {
      id: "qty",
      header: t("strategy.detail.qty"),
      numeric: true,
      priority: 2,
      cell: (o) => formatNumber(o.order.qty, locale, { maximumSignificantDigits: 6 }),
    },
    {
      id: "state",
      header: t("table.state"),
      cell: (o) => (
        <Chip tone={o.order.state === "open" ? "waiting" : o.order.state === "filled" ? "success" : "stopped"}>
          {t(`strategy.orderState.${o.order.state}`)}
        </Chip>
      ),
    },
    { id: "updated", header: t("strategy.detail.updated"), priority: 3, cell: (o) => when(o.updatedAt) },
  ];

  const fillColumns: DataColumn<FillRow>[] = [
    { id: "time", header: t("table.time"), cell: (f) => when(f.ts) },
    { id: "seq", header: "#", numeric: true, width: "1%", priority: 3, cell: (f) => f.seq },
    { id: "kind", header: t("strategy.detail.fillKind"), cell: (f) => t(`strategy.fillKind.${f.kind}`, { defaultValue: f.kind }) },
    {
      id: "side",
      header: t("table.side"),
      priority: 2,
      cell: (f) => (f.kind === "funding" ? NO_VALUE : t(`strategy.orderSide.${f.qty >= 0 ? "buy" : "sell"}`)),
    },
    { id: "price", header: t("table.price"), numeric: true, cell: (f) => formatPrice(f.price, locale) },
    {
      id: "qty",
      header: t("strategy.detail.qty"),
      numeric: true,
      priority: 2,
      cell: (f) => formatNumber(Math.abs(f.qty), locale, { maximumSignificantDigits: 6 }),
    },
    {
      id: "liq",
      header: t("strategy.detail.liquidity"),
      priority: 3,
      cell: (f) => t(`strategy.liquidity.${f.liquidity}`, { defaultValue: f.liquidity }),
    },
    {
      id: "fees",
      header: t("strategy.detail.fees"),
      numeric: true,
      priority: 3,
      cell: (f) => formatNumber(f.feeQuote, locale, { maximumFractionDigits: 4 }),
    },
    {
      id: "realized",
      header: t("strategy.detail.realized"),
      numeric: true,
      cell: (f) => formatPnl(f.realizedQuote, locale, { unit: "none" }).text,
      tone: (f) => pnlToneAttr(f.realizedQuote),
    },
  ];

  const noteColumns: DataColumn<StrategyNote>[] = [
    { id: "time", header: t("table.time"), width: "1%", cell: (n) => when(n.atMs) },
    { id: "symbol", header: t("table.symbol"), width: "1%", cell: (n) => n.symbol },
    { id: "reason", header: t("table.reason"), wrap: true, keep: true, cell: (n) => strategyNoteText(t, n) },
  ];

  const right =
    tab === "settings" && detail ? (
      <StrategyPreviewPanel
        preview={summary?.preview ?? null}
        error={summary?.error ?? null}
        cfg={summary?.cfg ?? detail.config}
        available={summary?.available ?? null}
      />
    ) : (
      <>
        <Panel title={t("botDetail.stateTitle")} aside={<StatusChip status={runStatusKind(view)} label={runStateText(t, view)} />}>
          <FactList
            rows={[
              { label: t("strategy.detail.newCycles"), value: t(view.acceptingNewCycles ? "strategy.detail.allowed" : "strategy.detail.held") },
              { label: t("strategy.detail.openCycle"), value: c ? `#${c.seq}` : NO_VALUE },
              { label: t("strategy.detail.lastNote"), value: view.lastNote ? strategyNoteText(t, view.lastNote) : NO_VALUE },
            ]}
          />
          {actions.error ? <p className="ae-error">{actions.error.text}</p> : null}
        </Panel>
        <StrategyLivePanel view={view} config={detail?.config ?? null} />
        <StrategyRiskPanel />
      </>
    );

  return (
    <PageShell
      title={view.name}
      crumbs={[
        { label: t("nav.groups.bots"), to: "/bots" },
        { label: t(view.kind === "dca" ? "nav.dcaBots" : "nav.gridBots"), to: listPath },
      ]}
      secondary={secondary}
      primary={primary}
      left={
        <Panel title={t("botDetail.facts")}>
          <FactList rows={facts} />
        </Panel>
      }
      right={right}
    >
      {errParam ? (
        <p className="ae-banner" data-tone="danger" role="alert">
          {strategyErrorText(t, errParam)}
          <Button variant="ghost" size="sm" onClick={() => setErr(null)}>
            {t("strategy.detail.dismiss")}
          </Button>
        </p>
      ) : null}
      {loadError ? (
        <p className="ae-banner" data-tone="danger" role="alert">
          {loadError}
        </p>
      ) : null}
      {view.runState === "dead" ? (
        <p className="ae-banner" data-tone="danger">
          {runStateText(t, view)}
        </p>
      ) : null}
      <Tabs
        label={t("botDetail.tabsLabel")}
        value={tab}
        onChange={(v) => {
          setSaved(false);
          setTab(v === "overview" ? null : v);
        }}
        // "Real vs paper" only for a bot that has traded real money.
        tabs={TABS.filter((x) => x !== "real" || hasReal).map((x) => ({ id: x, label: t(`strategy.tabs.${x}`) }))}
      >
        {tab === "overview" ? (
          <>
            <KpiGrid>
              <KpiTile
                label={t("strategy.col.totalPnl")}
                value={formatSignedPercent(total, locale)}
                tone={pnlToneAttr(total)}
                note={usdt(totalQuote, true)}
              />
              <KpiTile
                label={t("strategy.col.realized")}
                value={formatSignedPercent(view.realizedPct, locale)}
                tone={pnlToneAttr(view.realizedPct)}
                note={usdt(view.realizedQuote, true)}
              />
              <KpiTile
                label={t("strategy.detail.floating")}
                value={formatSignedPercent(view.mtmPct, locale)}
                tone={pnlToneAttr(view.mtmPct)}
                note={c ? usdt(c.unrealizedQuote, true) : t("strategy.detail.noOpenCycle")}
              />
              <KpiTile
                label={t("strategy.col.drawdown")}
                value={formatSignedPercent(view.drawdownPct, locale)}
                note={t("strategy.detail.maxDrawdown", { value: formatSignedPercent(view.maxDrawdownPct, locale) })}
              />
              <KpiTile label={t("strategy.col.cycles")} value={view.cyclesDone} />
              <KpiTile
                label={t("strategy.detail.utilisation")}
                value={`${formatNumber(view.utilisationInPosition * 100, locale, { maximumFractionDigits: 1 })}%`}
                note={t("strategy.detail.committed", { value: `${formatNumber(view.utilisationCommitted * 100, locale, { maximumFractionDigits: 1 })}%` })}
              />
            </KpiGrid>

            <Panel title={t("strategy.detail.equity")}>
              {equity === null ? (
                <p className="ae-subtle">{t("workspace.loading")}</p>
              ) : equity.length < 2 ? (
                <p className="ae-subtle">{t("strategy.detail.noEquity")}</p>
              ) : (
                <>
                  <EquityChart rows={equity} budget={view.budget} label={t("strategy.detail.equity")} />
                  <p className="ae-subtle">
                    {when(equity[0].ts)} – {when(equity[equity.length - 1].ts)} · {t("strategy.detail.equityBase", { value: usdt(view.budget) })}
                  </p>
                </>
              )}
            </Panel>

            {c ? (
              <Panel title={t("strategy.detail.openCycleTitle", { seq: c.seq })}>
                <FactList
                  rows={[
                    { label: t("strategy.detail.opened"), value: when(c.openedAt) },
                    { label: t("strategy.detail.anchor"), value: formatPrice(c.anchorPrice, locale) },
                    { label: t("strategy.preview.avgEntry"), value: c.avgEntry !== null ? formatPrice(c.avgEntry, locale) : NO_VALUE },
                    { label: t("strategy.detail.position"), value: formatNumber(c.signedQty, locale, { maximumSignificantDigits: 6, signDisplay: "exceptZero" }) },
                    { label: t("strategy.detail.notional"), value: usdt(c.notional) },
                    { label: t("strategy.detail.margin"), value: usdt(c.margin) },
                    { label: t("strategy.detail.reserved"), value: usdt(c.reserved) },
                    { label: t("strategy.detail.unrealized"), value: usdt(c.unrealizedQuote, true), tone: pnlToneAttr(c.unrealizedQuote) },
                    { label: t("strategy.detail.fees"), value: usdt(c.feesQuote) },
                    {
                      label: t("strategy.detail.funding"),
                      value: c.fundingUnknown ? `${usdt(c.fundingQuote)} · ${t("strategy.notes.fundingUnknown")}` : usdt(c.fundingQuote),
                      tone: c.fundingUnknown ? "warn" : undefined,
                    },
                    ...(c.soFilled !== null ? [{ label: t("strategy.detail.soFilled"), value: c.soFilled }] : []),
                    ...(c.gridClosingFills !== null ? [{ label: t("strategy.detail.closingFills"), value: c.gridClosingFills }] : []),
                    { label: t("strategy.preview.liqPrice"), value: c.liqPrice !== null ? formatPrice(c.liqPrice, locale) : t("strategy.preview.noLiqAt", { leverage: view.leverage }), tone: c.liqPrice !== null ? "warn" : undefined },
                    { label: t("strategy.detail.maxAdverse"), value: formatSignedPercent(c.maxAdversePct, locale) },
                    ...(c.outOfRange ? [{ label: t("table.state"), value: t("strategy.notes.outOfRange"), tone: "warn" as const }] : []),
                  ]}
                />
              </Panel>
            ) : null}

            <Panel title={t("strategy.detail.stats")} aside={stats ? <span className="ae-muted">{t("strategy.detail.statsN", { n: stats.cycles })}</span> : null}>
              {!stats ? (
                <p className="ae-subtle">{t("workspace.loading")}</p>
              ) : stats.cycles === 0 ? (
                <p className="ae-subtle">{t("strategy.detail.noClosedCycles")}</p>
              ) : (
                <FactList
                  rows={[
                    { label: t("strategy.stats.winRate"), value: stats.winRate !== null ? `${formatNumber(stats.winRate * 100, locale, { maximumFractionDigits: 1 })}%` : NO_VALUE },
                    { label: t("strategy.stats.meanCycle"), value: stats.meanCycleReturnPct !== null ? formatSignedPercent(stats.meanCycleReturnPct, locale, 3) : NO_VALUE },
                    { label: t("strategy.stats.medianCycle"), value: stats.medianCycleReturnPct !== null ? formatSignedPercent(stats.medianCycleReturnPct, locale, 3) : NO_VALUE },
                    { label: t("strategy.stats.perDay"), value: stats.returnPerDayOfCapitalPct !== null ? formatSignedPercent(stats.returnPerDayOfCapitalPct, locale, 3) : NO_VALUE },
                    { label: t("strategy.stats.fees"), value: `${formatNumber(stats.feesPctOfBudget, locale, { maximumFractionDigits: 3 })}%` },
                    { label: t("strategy.stats.funding"), value: `${formatNumber(stats.fundingPctOfBudget, locale, { maximumFractionDigits: 3 })}%` },
                    ...(stats.safetyOrdersFilledMean !== null ? [{ label: t("strategy.stats.soMean"), value: formatNumber(stats.safetyOrdersFilledMean, locale, { maximumFractionDigits: 2 }) }] : []),
                    ...(stats.gridClosingFillsPerCycle !== null ? [{ label: t("strategy.stats.closingFills"), value: formatNumber(stats.gridClosingFillsPerCycle, locale, { maximumFractionDigits: 2 }) }] : []),
                    ...Object.entries(stats.exits).map(([k, n]) => ({ label: t("strategy.stats.exit", { reason: t(`strategy.exit.${k}`, { defaultValue: k }) }), value: n })),
                  ]}
                />
              )}
            </Panel>
          </>
        ) : tab === "cycles" ? (
          cycles === null ? (
            <p className="ae-subtle">{t("workspace.loading")}</p>
          ) : cycles.length === 0 ? (
            <EmptyState title={t("strategy.detail.noCycles")} />
          ) : (
            <DataTable label={t("strategy.tabs.cycles")} columns={cycleColumns} rows={cycles} rowKey={(r) => String(r.seq)} compact />
          )
        ) : tab === "orders" ? (
          orders === null ? (
            <p className="ae-subtle">{t("workspace.loading")}</p>
          ) : orders.length === 0 ? (
            <EmptyState title={t("botDetail.noOrders")} />
          ) : (
            <DataTable label={t("strategy.tabs.orders")} columns={orderColumns} rows={orders} rowKey={(o) => o.order.clientId} compact />
          )
        ) : tab === "fills" ? (
          fills === null ? (
            <p className="ae-subtle">{t("workspace.loading")}</p>
          ) : fills.length === 0 ? (
            <EmptyState title={t("strategy.detail.noFills")} />
          ) : (
            <DataTable label={t("strategy.tabs.fills")} columns={fillColumns} rows={fills} rowKey={(f) => `${f.clientId}:${f.ts}:${f.seq}:${f.kind}:${f.qty}`} compact />
          )
        ) : tab === "real" ? (
          <StrategyLiveReport botId={view.id} />
        ) : tab === "settings" ? (
          detail && liveIds.has(view.id) ? (
            // Real money is on: the checks made when it was switched on would
            // not hold after an edit. The form returns once it is back on paper.
            <>
              <p className="ae-banner" data-tone="warn">{t("strategy.errors.liveEditLocked")}</p>
              <StrategyLivePanel view={view} config={detail.config} />
            </>
          ) : detail ? (
            <>
              {saved ? <p className="ae-banner" data-tone="info">{t("strategy.form.saved")}</p> : null}
              <StrategyForm
                key={JSON.stringify(detail.config)}
                kind={view.kind}
                initial={detail.config}
                bot={view}
                preset={presets?.find((p) => p.id === detail.config.presetId) ?? null}
                onSubmit={(cfg) => onSave(cfg)}
                onCancel={() => setTab(null)}
                onPreview={setSummary}
                onDirty={onDirty}
              />
            </>
          ) : (
            <p className="ae-subtle">{t("workspace.loading")}</p>
          )
        ) : ownNotes.length === 0 ? (
          <EmptyState title={t("strategy.detail.noNotes")} />
        ) : (
          <DataTable
            label={t("strategy.tabs.log")}
            columns={noteColumns}
            rows={ownNotes}
            rowKey={(n) => `${n.atMs}:${n.key}:${n.symbol}`}
            compact
          />
        )}
      </Tabs>
      {actions.dialog}
    </PageShell>
  );
}
