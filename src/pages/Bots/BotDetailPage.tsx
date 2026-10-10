import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { navigate, useQueryParam, useRouter } from "@/app/router/router";
import { Button } from "@/components/ui/Button/Button";
import { LiveChip, ManualChip, UntestedChip } from "@/components/ui/Chip/Chip";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { KpiGrid } from "@/components/ui/KpiGrid/KpiGrid";
import { KpiTile } from "@/components/ui/KpiTile/KpiTile";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { Tabs } from "@/components/ui/Tabs/Tabs";
import { localeForLanguage } from "@/i18n";
import { formatPnl, formatPrice, formatSignedPercent, formatTableTime, formatUsdt, NO_VALUE, pnlToneAttr } from "@/lib/format";
import type { OpenPosition, SkipNote } from "@/lib/ipc/bot/bot";
import { exchangeName } from "@/lib/ipc/exchange/exchange";
import type { TradeRecord } from "@/lib/ipc/trades/trades";
import { skipParams } from "@/lib/skipNotes";
import { PositionsTable } from "@/pages/Positions/PositionsTable";
import { isStrategyBotId } from "@/lib/ipc/strategy/strategy";
import { kindForBotId, signalBotRows } from "./botRows";
import { BotStateChip } from "./BotsTable";
import { SignalBotForm } from "./signal/SignalBotForm";
import { StrategyBotDetail } from "./strategy/StrategyBotDetail";
import "./bots.css";

const TABS = ["overview", "deals", "orders", "settings", "log"] as const;
type Tab = (typeof TABS)[number];

/**
 * #/bots/:botId. Signal bots are the three legacy kinds (signal-futures /
 * signal-spot / signal-pump); DCA and Grid bots are strategy instances
 * ("sb_..."). An unknown id is a "Bot not found" page.
 */
export function BotDetailPage({ botId }: { botId: string }) {
  if (isStrategyBotId(botId)) return <StrategyBotPage botId={botId} />;
  return <SignalBotDetailPage botId={botId} />;
}

function NotFound({ botId }: { botId: string }) {
  const { t } = useTranslation();
  return (
    <PageShell title={t("botDetail.notFound")} crumbs={[{ label: t("nav.groups.bots"), to: "/bots" }]} single>
      <EmptyState
        title={t("botDetail.notFound")}
        detail={botId}
        actions={
          <Button variant="secondary" size="sm" onClick={() => navigate("/bots")}>
            {t("nav.allBots")}
          </Button>
        }
      />
    </PageShell>
  );
}

function StrategyBotPage({ botId }: { botId: string }) {
  const { t } = useTranslation();
  const { strategy } = useDeskContext();
  if (strategy.bots === null) {
    return (
      <PageShell title={t("page.botDetail")} crumbs={[{ label: t("nav.groups.bots"), to: "/bots" }]} single>
        <p className="ae-subtle">{t("workspace.loading")}</p>
      </PageShell>
    );
  }
  const view = strategy.bots.find((b) => b.id === botId);
  return view ? <StrategyBotDetail view={view} /> : <NotFound botId={botId} />;
}

interface OrderLeg {
  position: OpenPosition;
  leg: "tp" | "sl";
}

function SignalBotDetailPage({ botId }: { botId: string }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const ctx = useDeskContext();
  const { desk, risk, pnl, catalog } = ctx;
  const { from } = useRouter();
  const [tabParam, setTab] = useQueryParam("tab");
  const tab: Tab = (TABS as readonly string[]).includes(tabParam ?? "") ? (tabParam as Tab) : "overview";
  const kind = kindForBotId(botId);
  const fromAll = from === "/bots";
  const parent = kind
    ? fromAll
      ? { label: t("nav.allBots"), to: "/bots" }
      : { label: t("nav.signalBots"), to: "/bots/signal" }
    : { label: t("nav.allBots"), to: "/bots" };

  if (!kind) return <NotFound botId={botId} />;

  const name = t(`bots.kind.${kind}`);
  const crumbs = [{ label: t("nav.groups.bots"), to: "/bots" }, parent];

  if (!desk.loaded) {
    return (
      <PageShell title={name} crumbs={crumbs}>
        <p className="ae-subtle">{t("workspace.loading")}</p>
      </PageShell>
    );
  }

  const row = signalBotRows(desk.status, risk.state?.maxCapitalQuote ?? null).find((r) => r.kind === kind)!;
  const positions = desk.status.openPositions.filter((p) => p.botKind === kind);
  const trades = pnl.trades.filter((tr) => tr.botKind === kind);
  const s = desk.status;
  const maxLeverage = kind === "spot" ? 1 : risk.state?.limits.maxLeverage ?? null;
  const side = (direction: string) => t(`signalDesk.direction.${direction === "short" ? "short" : "long"}`);

  const primary = row.running ? null : (
    <Button
      size="sm"
      disabled={desk.busy || row.startBlockKey !== null}
      disabledReason={row.startBlockKey ? t(row.startBlockKey) : undefined}
      tooltipPlacement="bottom"
      tooltipAlign="end"
      onClick={() => row.config && void desk.configureAndStart(row.config)}
    >
      {t("bots.start")}
    </Button>
  );
  const secondary = row.running ? (
    <Button variant="secondary" size="sm" disabled={desk.busy} onClick={() => void desk.stop(kind)}>
      {t("bots.stop")}
    </Button>
  ) : null;

  const left = (
    <Panel title={t("botDetail.facts")} aside={row.untested ? <UntestedChip title={t("bots.pumpNote")} /> : undefined}>
      {row.untested ? <p className="ae-muted">{t("bots.pumpNote")}</p> : null}
      <FactList
        rows={[
          { label: t("table.type"), value: t("botsList.type.signal") },
          { label: t("table.market"), value: t(`filters.market.${row.market}`) },
          { label: t("table.exchange"), value: row.exchangeId ? exchangeName(row.exchangeId, catalog.exchanges) : NO_VALUE },
          { label: t("table.pairs"), value: row.config ? (row.symbols.length ? row.symbols.join(", ") : t("botsList.allPairs")) : NO_VALUE },
          ...(row.live ? [{ label: t("table.mode"), value: "LIVE", tone: "danger" as const }] : []),
          { label: t("table.leverage"), value: row.config ? `${row.config.leverage}x` : NO_VALUE },
          { label: t("table.capital"), value: row.capital !== null ? formatUsdt(row.capital, locale) : NO_VALUE },
          { label: t("table.maxPositions"), value: row.maxPositions ?? NO_VALUE },
        ]}
      />
    </Panel>
  );

  const right = (
    <Panel title={t("botDetail.stateTitle")} aside={<BotStateChip row={row} />}>
      <FactList
        rows={[
          { label: t("table.openPositions"), value: positions.length },
          { label: t("botDetail.killSwitch"), value: t(s.killSwitchTripped ? "safety.killSwitch.tripped" : "safety.killSwitch.ok"), tone: s.killSwitchTripped ? "danger" : undefined },
          { label: t("safety.regime.title"), value: t(`statusbar.regime.${s.btcRegime}`) },
        ]}
      />
      {desk.error ? <p className="ae-error">{desk.error}</p> : null}
    </Panel>
  );

  const tradeColumns: DataColumn<TradeRecord>[] = [
    { id: "closed", header: t("table.closedAt"), cell: (tr) => formatTableTime(tr.closedAt, locale) },
    {
      id: "symbol",
      header: t("table.symbol"),
      cell: (tr) => (
        <span className="ae-namecell">
          {tr.symbol}
          {tr.live ? <LiveChip /> : null}
          {tr.manual ? <ManualChip /> : null}
        </span>
      ),
    },
    { id: "side", header: t("table.side"), priority: 2, cell: (tr) => side(tr.direction) },
    { id: "entry", header: t("table.entry"), numeric: true, priority: 3, cell: (tr) => formatPrice(tr.entry, locale) },
    { id: "exit", header: t("table.exit"), numeric: true, priority: 3, cell: (tr) => formatPrice(tr.exit, locale) },
    {
      id: "pnlPct",
      header: t("table.pnlPct"),
      numeric: true,
      cell: (tr) => formatSignedPercent(tr.pnlPct, locale),
      tone: (tr) => pnlToneAttr(tr.pnlPct),
    },
    {
      id: "pnlUsdt",
      header: t("table.pnlUsdt"),
      numeric: true,
      cell: (tr) => formatPnl(tr.pnlUsdt, locale, { unit: "none" }).text,
      tone: (tr) => pnlToneAttr(tr.pnlUsdt),
    },
    { id: "reason", header: t("table.exitReason"), priority: 2, cell: (tr) => t(`pnl.exit.${tr.exitReason}`, { defaultValue: tr.exitReason }) },
  ];

  const orderColumns: DataColumn<OrderLeg>[] = [
    {
      id: "symbol",
      header: t("table.symbol"),
      cell: (o) => (
        <span className="ae-namecell">
          {o.position.symbol}
          {o.position.live ? <LiveChip /> : null}
          {o.position.manual ? <ManualChip /> : null}
        </span>
      ),
    },
    { id: "type", header: t("table.orderType"), cell: (o) => t(`botDetail.orderLeg.${o.leg}`) },
    { id: "side", header: t("table.side"), cell: (o) => t(o.position.direction === "short" ? "botDetail.closeShort" : "botDetail.closeLong") },
    { id: "price", header: t("table.price"), numeric: true, cell: (o) => formatPrice(o.leg === "tp" ? o.position.tp : o.position.sl, locale) },
  ];

  const logColumns: DataColumn<SkipNote>[] = [
    { id: "time", header: t("table.time"), width: "1%", cell: (n) => formatTableTime(n.atMs, locale) },
    { id: "symbol", header: t("table.symbol"), width: "1%", cell: (n) => n.symbol },
    {
      id: "reason",
      header: t("table.reason"),
      wrap: true,
      keep: true,
      cell: (n) => t(`bots.skipReasons.${n.reason}`, skipParams(n)),
    },
  ];

  return (
    <PageShell title={name} crumbs={crumbs} secondary={secondary} primary={primary} left={left} right={right}>
      <Tabs
        label={t("botDetail.tabsLabel")}
        value={tab}
        onChange={(v) => setTab(v === "overview" ? null : v)}
        tabs={TABS.map((id) => ({ id, label: t(`botDetail.tabs.${id}`) }))}
      >
        {tab === "overview" ? (
          <>
            <KpiGrid>
              <KpiTile label={t("table.openPositions")} value={positions.length} />
              <KpiTile label={t("botDetail.closedLoaded")} value={trades.length} note={t("botDetail.loadedWindow", { count: pnl.trades.length })} />
              <KpiTile label={t("table.capital")} value={row.capital !== null ? formatUsdt(row.capital, locale) : NO_VALUE} />
            </KpiGrid>
            <PositionsTable positions={positions} onClose={desk.closePosition} />
          </>
        ) : tab === "deals" ? (
          <>
            <div className="ae-toolbar">
              <span className="ae-muted">{t("botDetail.loadedWindow", { count: pnl.trades.length })}</span>
              <span className="ae-toolbar__spacer" />
              <Button variant="secondary" size="sm" onClick={() => void pnl.exportCsv()}>
                {t("pnl.exportCsv")}
              </Button>
            </div>
            {pnl.exportedTo ? <p className="ae-muted">{t("pnl.exportedTo", { path: pnl.exportedTo })}</p> : null}
            {pnl.exportError ? <p className="ae-error">{pnl.exportError}</p> : null}
            <DataTable
              label={t("botDetail.tabs.deals")}
              columns={tradeColumns}
              rows={trades}
              rowKey={(tr) => String(tr.id)}
              empty={<EmptyState title={t("botDetail.noTrades")} />}
            />
          </>
        ) : tab === "orders" ? (
          <DataTable
            label={t("botDetail.tabs.orders")}
            columns={orderColumns}
            rows={positions.flatMap((p) => (["tp", "sl"] as const).map((leg) => ({ position: p, leg })))}
            rowKey={(o) => `${o.position.signalId}:${o.leg}`}
            empty={<EmptyState title={t("botDetail.noOrders")} />}
          />
        ) : tab === "settings" ? (
          catalog.exchanges === null || maxLeverage === null ? (
            catalog.error ? (
              <p className="ae-error">{catalog.error}</p>
            ) : (
              <p className="ae-subtle">{t("workspace.loading")}</p>
            )
          ) : (
            <SignalBotForm
              kind={kind}
              config={row.config}
              running={row.running}
              busy={desk.busy}
              disabled={s.killSwitchTripped}
              exchangeLocked={row.live || positions.some((p) => p.live)}
              disabledReasonKey={row.startBlockKey}
              maxLeverage={maxLeverage}
              maxCapitalQuote={risk.state?.maxCapitalQuote ?? null}
              levelMaxPositions={risk.state?.limits.maxConcurrentPositions ?? null}
              exchanges={catalog.exchanges}
              live={
                kind === "futures" && s.liveTradingEnabled
                  ? {
                      binanceIsProduction: s.binanceIsProduction,
                      onSetLive: (enabled, confirmation) => desk.setLive("futures", enabled, confirmation),
                      pilotLeft: desk.status.pilotLeft ?? 0,
                      onEndPilot: () => desk.endPilot("futures"),
                      liveVenues: s.liveVenues,
                      venueSandbox: s.venueSandbox,
                    }
                  : undefined
              }
              onStart={(c) => void desk.configureAndStart(c)}
              onStop={() => void desk.stop(kind)}
            />
          )
        ) : (
          <>
            <p className="ae-muted">{t("botDetail.logScope")}</p>
            <DataTable
              label={t("botDetail.tabs.log")}
              columns={logColumns}
              rows={s.recentSkips}
              rowKey={(n) => `${n.atMs}:${n.symbol}:${n.reason}`}
              compact
              empty={<EmptyState title={t("bots.noSkips")} />}
            />
          </>
        )}
      </Tabs>
    </PageShell>
  );
}
